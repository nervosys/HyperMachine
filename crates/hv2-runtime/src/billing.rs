//! Billing and Metering
//!
//! Tracks resource consumption per agent session for usage-based billing.
//! Meters CPU-seconds, memory-hours, GPU-minutes, network bytes, and
//! storage operations. Produces invoices and usage summaries.

use std::collections::HashMap;
use std::time::SystemTime;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Billing operation result
pub type BillingResult<T> = Result<T, BillingError>;

/// Billing errors
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BillingError {
    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Meter not found
    #[error("Meter not found: {0}")]
    MeterNotFound(String),

    /// Budget exceeded
    #[error("Budget exceeded for session {session_id}: {used} / {budget} {unit}")]
    BudgetExceeded {
        session_id: String,
        used: f64,
        budget: f64,
        unit: String,
    },

    /// Invalid amount
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),
}

/// Billing tier for rate calculation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BillingTier {
    /// Free tier (development, testing)
    Free,
    /// Standard tier
    #[default]
    Standard,
    /// Premium tier (priority scheduling, dedicated VMs)
    Premium,
    /// Enterprise tier (custom rates)
    Enterprise,
}

/// Billing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingConfig {
    /// Enable billing
    pub enabled: bool,
    /// Default billing tier
    pub default_tier: BillingTier,
    /// Rates per unit (tier -> resource -> rate_per_unit)
    pub rates: HashMap<String, f64>,
    /// Free tier allowances
    pub free_allowances: HashMap<String, f64>,
    /// Budget enforcement
    pub enforce_budgets: bool,
}

impl Default for BillingConfig {
    fn default() -> Self {
        let mut rates = HashMap::new();
        rates.insert("cpu_seconds".to_string(), 0.00001); // $0.01 per 1000 CPU-seconds
        rates.insert("memory_gb_hours".to_string(), 0.005); // $0.005 per GB-hour
        rates.insert("gpu_minutes".to_string(), 0.01); // $0.01 per GPU-minute
        rates.insert("network_gb".to_string(), 0.09); // $0.09 per GB transfer
        rates.insert("storage_ops".to_string(), 0.000004); // $0.004 per 1000 ops
        rates.insert("workflow_executions".to_string(), 0.001); // $0.001 per workflow

        let mut free_allowances = HashMap::new();
        free_allowances.insert("cpu_seconds".to_string(), 3600.0); // 1 hour
        free_allowances.insert("memory_gb_hours".to_string(), 4.0); // 4 GB-hours
        free_allowances.insert("gpu_minutes".to_string(), 0.0);
        free_allowances.insert("network_gb".to_string(), 1.0);
        free_allowances.insert("storage_ops".to_string(), 10000.0);
        free_allowances.insert("workflow_executions".to_string(), 100.0);

        Self {
            enabled: true,
            default_tier: BillingTier::Standard,
            rates,
            free_allowances,
            enforce_budgets: false,
        }
    }
}

/// A resource meter tracking one dimension of consumption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeter {
    /// Resource name (e.g., "cpu_seconds")
    pub resource: String,
    /// Total consumed
    pub total: f64,
    /// Unit label
    pub unit: String,
    /// Last updated
    pub last_updated: SystemTime,
}

impl ResourceMeter {
    /// Create a new meter
    pub fn new(resource: impl Into<String>, unit: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            total: 0.0,
            unit: unit.into(),
            last_updated: SystemTime::now(),
        }
    }

    /// Record consumption
    pub fn record(&mut self, amount: f64) {
        self.total += amount;
        self.last_updated = SystemTime::now();
    }
}

/// A meter reading snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterReading {
    /// Resource name
    pub resource: String,
    /// Amount consumed
    pub amount: f64,
    /// Unit
    pub unit: String,
    /// Rate per unit
    pub rate: f64,
    /// Cost (amount * rate)
    pub cost: f64,
}

/// A billing event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingEvent {
    /// Session that incurred the charge
    pub session_id: String,
    /// Resource consumed
    pub resource: String,
    /// Amount consumed
    pub amount: f64,
    /// Unit
    pub unit: String,
    /// When the consumption occurred
    pub timestamp: SystemTime,
}

/// Line item in an invoice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineItem {
    /// Resource description
    pub description: String,
    /// Quantity consumed
    pub quantity: f64,
    /// Unit
    pub unit: String,
    /// Rate per unit
    pub rate: f64,
    /// Total cost
    pub total: f64,
}

/// Invoice for a billing period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    /// Session or account ID
    pub session_id: String,
    /// Billing period start
    pub period_start: SystemTime,
    /// Billing period end
    pub period_end: SystemTime,
    /// Line items
    pub items: Vec<LineItem>,
    /// Subtotal
    pub subtotal: f64,
    /// Tier
    pub tier: BillingTier,
}

impl Invoice {
    /// Total amount due
    pub fn total(&self) -> f64 {
        self.subtotal
    }
}

/// Usage summary for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    /// Session ID
    pub session_id: String,
    /// Per-resource readings
    pub readings: Vec<MeterReading>,
    /// Total cost
    pub total_cost: f64,
    /// Billing tier
    pub tier: BillingTier,
}

/// Metering engine
///
/// Tracks resource consumption per session and produces invoices.
pub struct MeteringEngine {
    /// Configuration
    config: BillingConfig,
    /// Per-session meters: session_id -> resource -> meter
    meters: RwLock<HashMap<String, HashMap<String, ResourceMeter>>>,
    /// Session tiers
    tiers: RwLock<HashMap<String, BillingTier>>,
    /// Session budgets: session_id -> max_cost
    budgets: RwLock<HashMap<String, f64>>,
    /// Event log
    events: RwLock<Vec<BillingEvent>>,
}

impl MeteringEngine {
    /// Create a new metering engine
    pub fn new(config: BillingConfig) -> Self {
        Self {
            config,
            meters: RwLock::new(HashMap::new()),
            tiers: RwLock::new(HashMap::new()),
            budgets: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
        }
    }

    /// Register a session for metering
    pub fn register_session(&self, session_id: &str, tier: BillingTier) {
        let mut meters = self.meters.write();
        let session_meters = meters.entry(session_id.to_string()).or_default();

        // Initialize standard meters
        session_meters
            .entry("cpu_seconds".to_string())
            .or_insert_with(|| ResourceMeter::new("cpu_seconds", "seconds"));
        session_meters
            .entry("memory_gb_hours".to_string())
            .or_insert_with(|| ResourceMeter::new("memory_gb_hours", "GB-hours"));
        session_meters
            .entry("gpu_minutes".to_string())
            .or_insert_with(|| ResourceMeter::new("gpu_minutes", "minutes"));
        session_meters
            .entry("network_gb".to_string())
            .or_insert_with(|| ResourceMeter::new("network_gb", "GB"));
        session_meters
            .entry("storage_ops".to_string())
            .or_insert_with(|| ResourceMeter::new("storage_ops", "operations"));
        session_meters
            .entry("workflow_executions".to_string())
            .or_insert_with(|| ResourceMeter::new("workflow_executions", "executions"));

        self.tiers.write().insert(session_id.to_string(), tier);
    }

    /// Set a budget for a session
    pub fn set_budget(&self, session_id: &str, max_cost: f64) {
        self.budgets
            .write()
            .insert(session_id.to_string(), max_cost);
    }

    /// Record resource consumption
    pub fn record(&self, session_id: &str, resource: &str, amount: f64) -> BillingResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        if amount < 0.0 {
            return Err(BillingError::InvalidAmount(format!(
                "Negative amount: {amount}"
            )));
        }

        // Check budget before recording
        if self.config.enforce_budgets {
            if let Some(&budget) = self.budgets.read().get(session_id) {
                let current_cost = self.session_cost(session_id);
                let rate = self.config.rates.get(resource).copied().unwrap_or(0.0);
                let projected = current_cost + (amount * rate);
                if projected > budget {
                    return Err(BillingError::BudgetExceeded {
                        session_id: session_id.to_string(),
                        used: projected,
                        budget,
                        unit: "USD".to_string(),
                    });
                }
            }
        }

        let mut meters = self.meters.write();
        let session_meters = meters
            .get_mut(session_id)
            .ok_or_else(|| BillingError::SessionNotFound(session_id.to_string()))?;

        let meter = session_meters
            .get_mut(resource)
            .ok_or_else(|| BillingError::MeterNotFound(resource.to_string()))?;

        meter.record(amount);

        // Log event
        self.events.write().push(BillingEvent {
            session_id: session_id.to_string(),
            resource: resource.to_string(),
            amount,
            unit: meter.unit.clone(),
            timestamp: SystemTime::now(),
        });

        Ok(())
    }

    /// Get total cost for a session
    pub fn session_cost(&self, session_id: &str) -> f64 {
        let meters = self.meters.read();
        let session_meters = match meters.get(session_id) {
            Some(m) => m,
            None => return 0.0,
        };

        session_meters
            .values()
            .map(|meter| {
                let rate = self
                    .config
                    .rates
                    .get(&meter.resource)
                    .copied()
                    .unwrap_or(0.0);
                meter.total * rate
            })
            .sum()
    }

    /// Get usage summary for a session
    pub fn usage_summary(&self, session_id: &str) -> BillingResult<UsageSummary> {
        let meters = self.meters.read();
        let session_meters = meters
            .get(session_id)
            .ok_or_else(|| BillingError::SessionNotFound(session_id.to_string()))?;

        let tier = self
            .tiers
            .read()
            .get(session_id)
            .copied()
            .unwrap_or(self.config.default_tier);

        let readings: Vec<MeterReading> = session_meters
            .values()
            .map(|meter| {
                let rate = self
                    .config
                    .rates
                    .get(&meter.resource)
                    .copied()
                    .unwrap_or(0.0);
                MeterReading {
                    resource: meter.resource.clone(),
                    amount: meter.total,
                    unit: meter.unit.clone(),
                    rate,
                    cost: meter.total * rate,
                }
            })
            .collect();

        let total_cost = readings.iter().map(|r| r.cost).sum();

        Ok(UsageSummary {
            session_id: session_id.to_string(),
            readings,
            total_cost,
            tier,
        })
    }

    /// Generate an invoice for a session
    pub fn generate_invoice(&self, session_id: &str) -> BillingResult<Invoice> {
        let summary = self.usage_summary(session_id)?;
        let items: Vec<LineItem> = summary
            .readings
            .iter()
            .filter(|r| r.amount > 0.0)
            .map(|r| LineItem {
                description: r.resource.clone(),
                quantity: r.amount,
                unit: r.unit.clone(),
                rate: r.rate,
                total: r.cost,
            })
            .collect();

        let subtotal = items.iter().map(|i| i.total).sum();

        Ok(Invoice {
            session_id: session_id.to_string(),
            period_start: SystemTime::now(),
            period_end: SystemTime::now(),
            items,
            subtotal,
            tier: summary.tier,
        })
    }

    /// Unregister a session
    pub fn unregister_session(&self, session_id: &str) {
        self.meters.write().remove(session_id);
        self.tiers.write().remove(session_id);
        self.budgets.write().remove(session_id);
    }

    /// Number of tracked sessions
    pub fn session_count(&self) -> usize {
        self.meters.read().len()
    }

    /// Get engine configuration
    pub fn config(&self) -> &BillingConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine() -> MeteringEngine {
        MeteringEngine::new(BillingConfig::default())
    }

    #[test]
    fn test_register_session() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);
        assert_eq!(engine.session_count(), 1);
    }

    #[test]
    fn test_record_consumption() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);

        engine.record("s1", "cpu_seconds", 100.0).unwrap();
        engine.record("s1", "cpu_seconds", 50.0).unwrap();

        let summary = engine.usage_summary("s1").unwrap();
        let cpu = summary
            .readings
            .iter()
            .find(|r| r.resource == "cpu_seconds")
            .unwrap();
        assert!((cpu.amount - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_session_cost() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);

        engine.record("s1", "cpu_seconds", 1000.0).unwrap(); // $0.01
        engine.record("s1", "network_gb", 1.0).unwrap(); // $0.09

        let cost = engine.session_cost("s1");
        assert!((cost - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_generate_invoice() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);
        engine.record("s1", "cpu_seconds", 3600.0).unwrap();
        engine.record("s1", "network_gb", 10.0).unwrap();

        let invoice = engine.generate_invoice("s1").unwrap();
        assert!(!invoice.items.is_empty());
        assert!(invoice.total() > 0.0);
    }

    #[test]
    fn test_unknown_session() {
        let engine = test_engine();
        let err = engine.record("unknown", "cpu_seconds", 10.0).unwrap_err();
        assert!(matches!(err, BillingError::SessionNotFound(_)));
    }

    #[test]
    fn test_unknown_meter() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);
        let err = engine.record("s1", "nonexistent", 10.0).unwrap_err();
        assert!(matches!(err, BillingError::MeterNotFound(_)));
    }

    #[test]
    fn test_negative_amount() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);
        let err = engine.record("s1", "cpu_seconds", -10.0).unwrap_err();
        assert!(matches!(err, BillingError::InvalidAmount(_)));
    }

    #[test]
    fn test_budget_enforcement() {
        let engine = MeteringEngine::new(BillingConfig {
            enforce_budgets: true,
            ..Default::default()
        });
        engine.register_session("s1", BillingTier::Standard);
        engine.set_budget("s1", 0.001); // Very small budget

        // First recording is fine
        engine.record("s1", "cpu_seconds", 10.0).unwrap();

        // Large recording should exceed budget
        let err = engine.record("s1", "cpu_seconds", 1_000_000.0).unwrap_err();
        assert!(matches!(err, BillingError::BudgetExceeded { .. }));
    }

    #[test]
    fn test_disabled_billing() {
        let engine = MeteringEngine::new(BillingConfig {
            enabled: false,
            ..Default::default()
        });
        // Recording succeeds silently even without registration
        engine.record("any", "anything", 999.0).unwrap();
    }

    #[test]
    fn test_unregister() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Standard);
        engine.unregister_session("s1");
        assert_eq!(engine.session_count(), 0);
    }

    #[test]
    fn test_billing_tiers() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Premium);
        let summary = engine.usage_summary("s1").unwrap();
        assert_eq!(summary.tier, BillingTier::Premium);
    }

    #[test]
    fn test_zero_consumption_invoice() {
        let engine = test_engine();
        engine.register_session("s1", BillingTier::Free);
        let invoice = engine.generate_invoice("s1").unwrap();
        assert!(invoice.items.is_empty());
        assert!((invoice.total() - 0.0).abs() < f64::EPSILON);
    }
}
