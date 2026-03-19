//! Request Gateway
//!
//! Routes inbound agent requests to the appropriate VM session.
//! Supports session affinity (sticky routing), round-robin, and
//! least-connections strategies.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Gateway operation result
pub type GatewayResult<T> = Result<T, GatewayError>;

/// Gateway errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    /// No route found for the request
    #[error("No route for session: {0}")]
    NoRoute(String),

    /// Route already exists
    #[error("Route already exists: {session_id} -> {vm_id}")]
    RouteExists { session_id: String, vm_id: String },

    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// VM not available
    #[error("VM not available: {0}")]
    VmUnavailable(String),

    /// Rate limited
    #[error("Rate limited: {session_id} ({requests}/{limit} in window)")]
    RateLimited {
        session_id: String,
        requests: u64,
        limit: u64,
    },
}

/// Session affinity mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SessionAffinity {
    /// No affinity — any VM can serve any request
    None,
    /// Sticky — once assigned, a session always routes to the same VM
    #[default]
    Sticky,
    /// Soft affinity — prefer the same VM but allow fallback
    Preferred,
}

/// Route policy for load balancing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RoutePolicy {
    /// Round-robin across available VMs
    RoundRobin,
    /// Route to VM with fewest active sessions
    #[default]
    LeastConnections,
    /// Route based on consistent hashing of session ID
    ConsistentHash,
}

/// Gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Session affinity mode
    pub affinity: SessionAffinity,
    /// Routing policy for new sessions
    pub route_policy: RoutePolicy,
    /// Maximum sessions per VM
    pub max_sessions_per_vm: usize,
    /// Session idle timeout
    pub session_idle_timeout: Duration,
    /// Rate limit: requests per window per session
    pub rate_limit: u64,
    /// Rate limit window
    pub rate_limit_window: Duration,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            affinity: SessionAffinity::Sticky,
            route_policy: RoutePolicy::LeastConnections,
            max_sessions_per_vm: 10,
            session_idle_timeout: Duration::from_secs(1800), // 30 min
            rate_limit: 1000,
            rate_limit_window: Duration::from_secs(60),
        }
    }
}

/// A route binding a session to a VM
#[derive(Debug, Clone)]
pub struct Route {
    /// Session ID
    pub session_id: String,
    /// Target VM ID
    pub vm_id: String,
    /// When the route was created
    pub created_at: SystemTime,
    /// When the route was last used
    pub last_used: Instant,
    /// Total requests routed
    pub request_count: u64,
}

/// A routing decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Session ID
    pub session_id: String,
    /// Selected VM ID
    pub vm_id: String,
    /// Whether an existing route was used
    pub from_affinity: bool,
    /// Policy that made the decision
    pub policy: RoutePolicy,
}

/// Request gateway
///
/// Routes agent sessions to VMs and enforces rate limits.
pub struct Gateway {
    /// Configuration
    config: GatewayConfig,
    /// Active routes: session_id -> Route
    routes: RwLock<HashMap<String, Route>>,
    /// VM session counts for least-connections routing
    vm_session_counts: RwLock<HashMap<String, usize>>,
    /// Round-robin index
    rr_index: RwLock<usize>,
    /// Rate limiter: session_id -> (window_start, count)
    rate_limiter: RwLock<HashMap<String, (Instant, u64)>>,
}

impl Gateway {
    /// Create a new gateway
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            routes: RwLock::new(HashMap::new()),
            vm_session_counts: RwLock::new(HashMap::new()),
            rr_index: RwLock::new(0),
            rate_limiter: RwLock::new(HashMap::new()),
        }
    }

    /// Route a session to a VM
    ///
    /// If affinity is enabled and the session has an existing route,
    /// returns the same VM. Otherwise, applies the routing policy.
    pub fn route(
        &self,
        session_id: &str,
        available_vms: &[String],
    ) -> GatewayResult<RoutingDecision> {
        // Check rate limit
        self.check_rate_limit(session_id)?;

        // Check affinity
        if self.config.affinity != SessionAffinity::None {
            let mut routes = self.routes.write();
            if let Some(route) = routes.get_mut(session_id) {
                // Check if the VM is still available
                if available_vms.contains(&route.vm_id) {
                    route.last_used = Instant::now();
                    route.request_count += 1;
                    return Ok(RoutingDecision {
                        session_id: session_id.to_string(),
                        vm_id: route.vm_id.clone(),
                        from_affinity: true,
                        policy: self.config.route_policy,
                    });
                } else if self.config.affinity == SessionAffinity::Sticky {
                    // Sticky but VM gone — need new route
                    let _vm_id = route.vm_id.clone();
                    drop(routes);
                    self.remove_route(session_id);
                    // Fall through to new routing
                }
            }
        }

        // No affinity match — pick a VM
        if available_vms.is_empty() {
            return Err(GatewayError::NoRoute(session_id.to_string()));
        }

        let vm_id = match self.config.route_policy {
            RoutePolicy::LeastConnections => self.pick_least_connections(available_vms),
            RoutePolicy::RoundRobin => self.pick_round_robin(available_vms),
            RoutePolicy::ConsistentHash => self.pick_consistent_hash(session_id, available_vms),
        };

        // Create route
        let route = Route {
            session_id: session_id.to_string(),
            vm_id: vm_id.clone(),
            created_at: SystemTime::now(),
            last_used: Instant::now(),
            request_count: 1,
        };
        self.routes.write().insert(session_id.to_string(), route);
        *self
            .vm_session_counts
            .write()
            .entry(vm_id.clone())
            .or_insert(0) += 1;

        Ok(RoutingDecision {
            session_id: session_id.to_string(),
            vm_id,
            from_affinity: false,
            policy: self.config.route_policy,
        })
    }

    /// Remove a route (session ended)
    pub fn remove_route(&self, session_id: &str) -> Option<Route> {
        let route = self.routes.write().remove(session_id);
        if let Some(ref r) = route {
            let mut counts = self.vm_session_counts.write();
            if let Some(count) = counts.get_mut(&r.vm_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(&r.vm_id);
                }
            }
        }
        route
    }

    /// Remove all routes for a specific VM (VM being recycled/terminated)
    pub fn remove_vm_routes(&self, vm_id: &str) -> Vec<String> {
        let mut routes = self.routes.write();
        let affected: Vec<String> = routes
            .iter()
            .filter(|(_, r)| r.vm_id == vm_id)
            .map(|(k, _)| k.clone())
            .collect();
        for session_id in &affected {
            routes.remove(session_id);
        }
        drop(routes);

        self.vm_session_counts.write().remove(vm_id);
        affected
    }

    /// Get route count
    pub fn route_count(&self) -> usize {
        self.routes.read().len()
    }

    /// Get sessions for a specific VM
    pub fn sessions_for_vm(&self, vm_id: &str) -> Vec<String> {
        self.routes
            .read()
            .iter()
            .filter(|(_, r)| r.vm_id == vm_id)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Get the VM a session is routed to
    pub fn get_route(&self, session_id: &str) -> Option<String> {
        self.routes.read().get(session_id).map(|r| r.vm_id.clone())
    }

    /// Expire idle routes
    pub fn expire_idle(&self) -> Vec<String> {
        let mut routes = self.routes.write();
        let expired: Vec<String> = routes
            .iter()
            .filter(|(_, r)| r.last_used.elapsed() > self.config.session_idle_timeout)
            .map(|(k, _)| k.clone())
            .collect();
        for session_id in &expired {
            if let Some(route) = routes.remove(session_id) {
                let mut counts = self.vm_session_counts.write();
                if let Some(count) = counts.get_mut(&route.vm_id) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        expired
    }

    /// Get gateway configuration
    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    fn check_rate_limit(&self, session_id: &str) -> GatewayResult<()> {
        if self.config.rate_limit == 0 {
            return Ok(());
        }

        let mut limiter = self.rate_limiter.write();
        let now = Instant::now();
        let entry = limiter.entry(session_id.to_string()).or_insert((now, 0));

        if now.duration_since(entry.0) > self.config.rate_limit_window {
            // Reset window
            *entry = (now, 1);
            return Ok(());
        }

        entry.1 += 1;
        if entry.1 > self.config.rate_limit {
            return Err(GatewayError::RateLimited {
                session_id: session_id.to_string(),
                requests: entry.1,
                limit: self.config.rate_limit,
            });
        }

        Ok(())
    }

    fn pick_least_connections(&self, vms: &[String]) -> String {
        let counts = self.vm_session_counts.read();
        vms.iter()
            .min_by_key(|vm| counts.get(*vm).copied().unwrap_or(0))
            .cloned()
            .unwrap_or_else(|| vms[0].clone())
    }

    fn pick_round_robin(&self, vms: &[String]) -> String {
        let mut idx = self.rr_index.write();
        let vm = vms[*idx % vms.len()].clone();
        *idx = (*idx + 1) % vms.len();
        vm
    }

    fn pick_consistent_hash(&self, session_id: &str, vms: &[String]) -> String {
        // Simple hash: sum of bytes mod number of VMs
        let hash: usize = session_id.bytes().map(|b| b as usize).sum();
        vms[hash % vms.len()].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vms() -> Vec<String> {
        vec!["vm-1".to_string(), "vm-2".to_string(), "vm-3".to_string()]
    }

    #[test]
    fn test_route_new_session() {
        let gw = Gateway::new(GatewayConfig::default());
        let decision = gw.route("session-1", &test_vms()).unwrap();

        assert_eq!(decision.session_id, "session-1");
        assert!(!decision.from_affinity);
        assert!(test_vms().contains(&decision.vm_id));
    }

    #[test]
    fn test_sticky_affinity() {
        let gw = Gateway::new(GatewayConfig {
            affinity: SessionAffinity::Sticky,
            ..Default::default()
        });
        let first = gw.route("session-1", &test_vms()).unwrap();
        let second = gw.route("session-1", &test_vms()).unwrap();

        assert_eq!(first.vm_id, second.vm_id);
        assert!(second.from_affinity);
    }

    #[test]
    fn test_no_affinity() {
        let gw = Gateway::new(GatewayConfig {
            affinity: SessionAffinity::None,
            ..Default::default()
        });
        let first = gw.route("session-1", &test_vms()).unwrap();
        // Without affinity, no route is stored
        assert!(!first.from_affinity);
    }

    #[test]
    fn test_least_connections() {
        let gw = Gateway::new(GatewayConfig {
            route_policy: RoutePolicy::LeastConnections,
            affinity: SessionAffinity::None,
            ..Default::default()
        });
        let vms = test_vms();

        // Route several sessions — should spread across VMs
        for i in 0..6 {
            gw.route(&format!("s{i}"), &vms).unwrap();
        }

        let counts = gw.vm_session_counts.read();
        // Each VM should have ~2 sessions
        for vm in &vms {
            let count = counts.get(vm).copied().unwrap_or(0);
            assert!(count <= 3, "VM {vm} has {count} sessions, expected <= 3");
        }
    }

    #[test]
    fn test_round_robin() {
        let gw = Gateway::new(GatewayConfig {
            route_policy: RoutePolicy::RoundRobin,
            affinity: SessionAffinity::None,
            ..Default::default()
        });
        let vms = test_vms();

        let d1 = gw.route("s1", &vms).unwrap();
        let d2 = gw.route("s2", &vms).unwrap();
        let d3 = gw.route("s3", &vms).unwrap();

        // Should cycle through VMs
        assert_eq!(d1.vm_id, "vm-1");
        assert_eq!(d2.vm_id, "vm-2");
        assert_eq!(d3.vm_id, "vm-3");
    }

    #[test]
    fn test_remove_route() {
        let gw = Gateway::new(GatewayConfig::default());
        gw.route("s1", &test_vms()).unwrap();
        assert_eq!(gw.route_count(), 1);

        gw.remove_route("s1");
        assert_eq!(gw.route_count(), 0);
    }

    #[test]
    fn test_remove_vm_routes() {
        let gw = Gateway::new(GatewayConfig {
            affinity: SessionAffinity::Sticky,
            route_policy: RoutePolicy::RoundRobin,
            ..Default::default()
        });
        let vms = test_vms();

        gw.route("s1", &vms).unwrap(); // -> vm-1
        gw.route("s2", &vms).unwrap(); // -> vm-2
        gw.route("s3", &vms).unwrap(); // -> vm-3

        let affected = gw.remove_vm_routes("vm-1");
        assert_eq!(affected, vec!["s1"]);
        assert_eq!(gw.route_count(), 2);
    }

    #[test]
    fn test_no_available_vms() {
        let gw = Gateway::new(GatewayConfig::default());
        let err = gw.route("s1", &[]).unwrap_err();
        assert!(matches!(err, GatewayError::NoRoute(_)));
    }

    #[test]
    fn test_rate_limiting() {
        let gw = Gateway::new(GatewayConfig {
            rate_limit: 2,
            rate_limit_window: Duration::from_secs(60),
            affinity: SessionAffinity::Sticky,
            ..Default::default()
        });
        let vms = test_vms();

        gw.route("s1", &vms).unwrap();
        gw.route("s1", &vms).unwrap();
        let err = gw.route("s1", &vms).unwrap_err();
        assert!(matches!(err, GatewayError::RateLimited { .. }));
    }

    #[test]
    fn test_sessions_for_vm() {
        let gw = Gateway::new(GatewayConfig {
            route_policy: RoutePolicy::RoundRobin,
            ..Default::default()
        });
        let vms = test_vms();

        gw.route("s1", &vms).unwrap(); // vm-1
        gw.route("s2", &vms).unwrap(); // vm-2

        let sessions = gw.sessions_for_vm("vm-1");
        assert_eq!(sessions, vec!["s1"]);
    }

    #[test]
    fn test_get_route() {
        let gw = Gateway::new(GatewayConfig::default());
        assert!(gw.get_route("s1").is_none());

        gw.route("s1", &test_vms()).unwrap();
        assert!(gw.get_route("s1").is_some());
    }
}
