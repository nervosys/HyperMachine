//! End-to-end agent runtime.
//!
//! [`AgentRuntime`] composes the agent-runtime primitives into a single service:
//!
//! - **Spawn** an agent → an MCP session ([`McpServer`]) plus an O(1)
//!   copy-on-write memory sandbox from a warm baseline ([`SandboxPool`]).
//! - **Release / reap** an agent → closing or expiring the session fires the
//!   server's teardown hook, which drops the agent's sandbox.
//! - **Observe** the fleet → live agent count and fleet-wide memory cost
//!   (one shared baseline plus each agent's private, copied-on-write pages).
//!
//! This is the seam a deployment builds on: many short-lived agents spawn
//! instantly from a warm image, share it until they diverge, and have their
//! resources reclaimed automatically when their session ends.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::mcp::{AgentCapabilities, AgentSession, McpConfig, McpServer};
use hv2_core::{Sandbox, SandboxPool};

/// A live agent: its MCP session and its copy-on-write memory sandbox.
pub struct AgentHandle {
    /// The agent's MCP session (tool dispatch, capabilities, state).
    pub session: Arc<AgentSession>,
    /// The agent's copy-on-write memory, spawned from the warm baseline.
    pub sandbox: Sandbox,
}

impl AgentHandle {
    /// The agent's session id.
    pub fn session_id(&self) -> &str {
        &self.session.id
    }
}

/// Runtime that spawns and reclaims CoW-backed agents over an MCP server.
pub struct AgentRuntime {
    server: McpServer,
    pool: SandboxPool,
    sandboxes: Arc<Mutex<HashMap<String, Sandbox>>>,
}

impl AgentRuntime {
    /// Build a runtime over a warm baseline memory image.
    pub fn new(baseline: &[u8]) -> Self {
        Self::with_config(baseline, McpConfig::default())
    }

    /// Build a runtime with a custom MCP configuration.
    pub fn with_config(baseline: &[u8], config: McpConfig) -> Self {
        let server = McpServer::with_config(config);
        let pool = SandboxPool::from_bytes(baseline);
        let sandboxes: Arc<Mutex<HashMap<String, Sandbox>>> = Arc::new(Mutex::new(HashMap::new()));

        // Releasing or reaping a session drops its sandbox, reclaiming the
        // agent's private pages back to the fleet.
        let tracked = Arc::clone(&sandboxes);
        server.on_session_teardown(move |t| {
            tracked.lock().remove(&t.session_id);
        });

        Self {
            server,
            pool,
            sandboxes,
        }
    }

    /// Spawn an agent: a new session plus an O(1) CoW sandbox from the baseline.
    pub fn spawn_agent(
        &self,
        agent_id: &str,
        capabilities: AgentCapabilities,
    ) -> Result<AgentHandle, String> {
        let session = self.server.create_session(agent_id, capabilities)?;
        let sandbox = self.pool.spawn();
        self.sandboxes
            .lock()
            .insert(session.id.clone(), sandbox.clone());
        Ok(AgentHandle { session, sandbox })
    }

    /// Release an agent by session id (fires teardown, dropping its sandbox).
    /// Returns `true` if the session existed.
    pub fn release_agent(&self, session_id: &str) -> bool {
        self.server.close_session(session_id)
    }

    /// Reap agents idle longer than `max_idle`, returning the number reclaimed.
    pub fn reap_idle(&self, max_idle: Duration) -> usize {
        self.server.expire_idle_sessions(max_idle)
    }

    /// Number of live agent sessions.
    pub fn live_agents(&self) -> usize {
        self.server.session_count()
    }

    /// Fleet-wide memory cost: the single shared baseline plus every agent's
    /// private (copied-on-write) pages.
    pub fn fleet_memory_bytes(&self) -> usize {
        self.pool.total_bytes()
    }

    /// Bytes of the shared baseline image (counted once for the whole fleet).
    pub fn baseline_bytes(&self) -> usize {
        self.pool.baseline_bytes()
    }

    /// Access the underlying MCP server (tool discovery, dispatch, audit).
    pub fn server(&self) -> &McpServer {
        &self.server
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: usize = 4096;

    #[tokio::test]
    async fn spawn_and_release_agent() {
        let rt = AgentRuntime::new(&vec![0u8; 4 * PAGE]);
        let handle = rt.spawn_agent("a1", AgentCapabilities::full()).unwrap();
        assert_eq!(rt.live_agents(), 1);
        assert_eq!(rt.baseline_bytes(), 4 * PAGE);

        let sid = handle.session_id().to_string();
        assert!(rt.release_agent(&sid));
        // Session is gone; teardown removed the runtime's sandbox tracking.
        assert_eq!(rt.live_agents(), 0);
        assert!(!rt.release_agent(&sid));
    }

    #[tokio::test]
    async fn idle_agents_share_one_baseline() {
        let rt = AgentRuntime::new(&vec![7u8; 8 * PAGE]);
        let _a = rt.spawn_agent("a", AgentCapabilities::full()).unwrap();
        let b = rt.spawn_agent("b", AgentCapabilities::full()).unwrap();
        assert_eq!(rt.live_agents(), 2);
        // Two idle agents cost one shared baseline, not two.
        assert_eq!(rt.fleet_memory_bytes(), 8 * PAGE);

        // When an agent writes, only its private page adds to the fleet cost.
        b.sandbox.write(0, &[0xFF; 16]).unwrap();
        assert_eq!(rt.fleet_memory_bytes(), 8 * PAGE + PAGE);
    }

    #[tokio::test]
    async fn idle_agents_are_reaped() {
        let rt = AgentRuntime::new(&vec![0u8; PAGE]);
        let _a = rt.spawn_agent("a", AgentCapabilities::full()).unwrap();
        let _b = rt.spawn_agent("b", AgentCapabilities::full()).unwrap();
        assert_eq!(rt.live_agents(), 2);
        // Zero idle threshold reclaims both.
        assert_eq!(rt.reap_idle(Duration::from_secs(0)), 2);
        assert_eq!(rt.live_agents(), 0);
    }
}
