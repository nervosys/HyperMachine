//! Event system for AI agent operations
//!
//! This module provides an event streaming and subscription system for
//! real-time monitoring of agent activities and VM state changes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Event result type
pub type EventResult<T> = Result<T, EventError>;

/// Event system errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    /// Subscriber not found
    SubscriberNotFound(u64),
    /// Channel closed
    ChannelClosed,
    /// Buffer overflow - events dropped
    BufferOverflow(usize),
    /// Invalid filter
    InvalidFilter(String),
    /// Event not found
    EventNotFound(u64),
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventError::SubscriberNotFound(id) => write!(f, "Subscriber {} not found", id),
            EventError::ChannelClosed => write!(f, "Event channel closed"),
            EventError::BufferOverflow(dropped) => {
                write!(f, "Buffer overflow: {} events dropped", dropped)
            }
            EventError::InvalidFilter(msg) => write!(f, "Invalid filter: {}", msg),
            EventError::EventNotFound(id) => write!(f, "Event {} not found", id),
        }
    }
}

impl std::error::Error for EventError {}

/// Event category for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventCategory {
    /// VM lifecycle events
    Lifecycle,
    /// Resource usage events
    Resource,
    /// Security events
    Security,
    /// Performance events
    Performance,
    /// Script execution events
    Script,
    /// Network events
    Network,
    /// Storage events
    Storage,
    /// User/input events
    User,
    /// System events
    System,
    /// Custom category
    Custom,
}

impl std::fmt::Display for EventCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventCategory::Lifecycle => write!(f, "lifecycle"),
            EventCategory::Resource => write!(f, "resource"),
            EventCategory::Security => write!(f, "security"),
            EventCategory::Performance => write!(f, "performance"),
            EventCategory::Script => write!(f, "script"),
            EventCategory::Network => write!(f, "network"),
            EventCategory::Storage => write!(f, "storage"),
            EventCategory::User => write!(f, "user"),
            EventCategory::System => write!(f, "system"),
            EventCategory::Custom => write!(f, "custom"),
        }
    }
}

/// Event severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EventSeverity {
    /// Debug level
    Debug,
    /// Informational
    Info,
    /// Notice - normal but significant
    Notice,
    /// Warning
    Warning,
    /// Error
    Error,
    /// Critical
    Critical,
    /// Alert - immediate action needed
    Alert,
    /// Emergency - system unusable
    Emergency,
}

impl std::fmt::Display for EventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSeverity::Debug => write!(f, "DEBUG"),
            EventSeverity::Info => write!(f, "INFO"),
            EventSeverity::Notice => write!(f, "NOTICE"),
            EventSeverity::Warning => write!(f, "WARNING"),
            EventSeverity::Error => write!(f, "ERROR"),
            EventSeverity::Critical => write!(f, "CRITICAL"),
            EventSeverity::Alert => write!(f, "ALERT"),
            EventSeverity::Emergency => write!(f, "EMERGENCY"),
        }
    }
}

/// VM event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmEvent {
    /// Unique event ID
    pub id: u64,
    /// Event timestamp
    pub timestamp: SystemTime,
    /// Event category
    pub category: EventCategory,
    /// Event severity
    pub severity: EventSeverity,
    /// Event source
    pub source: String,
    /// Event name/type
    pub name: String,
    /// Human-readable message
    pub message: String,
    /// VM ID (if applicable)
    pub vm_id: Option<String>,
    /// Additional data
    pub data: HashMap<String, serde_json::Value>,
    /// Correlation ID for related events
    pub correlation_id: Option<u64>,
}

impl VmEvent {
    /// Create a new event
    pub fn new(
        category: EventCategory,
        severity: EventSeverity,
        source: impl Into<String>,
        name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        static EVENT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: EVENT_ID.fetch_add(1, Ordering::Relaxed),
            timestamp: SystemTime::now(),
            category,
            severity,
            source: source.into(),
            name: name.into(),
            message: message.into(),
            vm_id: None,
            data: HashMap::new(),
            correlation_id: None,
        }
    }

    /// Set VM ID
    pub fn with_vm_id(mut self, vm_id: impl Into<String>) -> Self {
        self.vm_id = Some(vm_id.into());
        self
    }

    /// Add data field
    pub fn with_data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }

    /// Set correlation ID
    pub fn with_correlation(mut self, correlation_id: u64) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Create a lifecycle event
    pub fn lifecycle(source: &str, name: &str, message: &str) -> Self {
        Self::new(
            EventCategory::Lifecycle,
            EventSeverity::Info,
            source,
            name,
            message,
        )
    }

    /// Create a resource event
    pub fn resource(source: &str, name: &str, message: &str) -> Self {
        Self::new(
            EventCategory::Resource,
            EventSeverity::Info,
            source,
            name,
            message,
        )
    }

    /// Create a security event
    pub fn security(source: &str, name: &str, message: &str, severity: EventSeverity) -> Self {
        Self::new(EventCategory::Security, severity, source, name, message)
    }

    /// Create a performance event
    pub fn performance(source: &str, name: &str, message: &str) -> Self {
        Self::new(
            EventCategory::Performance,
            EventSeverity::Info,
            source,
            name,
            message,
        )
    }

    /// Create a script event
    pub fn script(source: &str, name: &str, message: &str) -> Self {
        Self::new(
            EventCategory::Script,
            EventSeverity::Info,
            source,
            name,
            message,
        )
    }
}

/// Event filter for subscriptions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventFilter {
    /// Filter by categories
    pub categories: Option<Vec<EventCategory>>,
    /// Minimum severity
    pub min_severity: Option<EventSeverity>,
    /// Filter by source pattern
    pub source_pattern: Option<String>,
    /// Filter by VM ID
    pub vm_id: Option<String>,
    /// Filter by event name pattern
    pub name_pattern: Option<String>,
}

impl EventFilter {
    /// Create a new filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by category
    pub fn with_category(mut self, category: EventCategory) -> Self {
        self.categories.get_or_insert_with(Vec::new).push(category);
        self
    }

    /// Filter by multiple categories
    pub fn with_categories(mut self, categories: Vec<EventCategory>) -> Self {
        self.categories = Some(categories);
        self
    }

    /// Filter by minimum severity
    pub fn with_min_severity(mut self, severity: EventSeverity) -> Self {
        self.min_severity = Some(severity);
        self
    }

    /// Filter by source
    pub fn with_source(mut self, pattern: impl Into<String>) -> Self {
        self.source_pattern = Some(pattern.into());
        self
    }

    /// Filter by VM ID
    pub fn with_vm_id(mut self, vm_id: impl Into<String>) -> Self {
        self.vm_id = Some(vm_id.into());
        self
    }

    /// Filter by event name
    pub fn with_name(mut self, pattern: impl Into<String>) -> Self {
        self.name_pattern = Some(pattern.into());
        self
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &VmEvent) -> bool {
        // Check category
        if let Some(categories) = &self.categories {
            if !categories.contains(&event.category) {
                return false;
            }
        }

        // Check severity
        if let Some(min_severity) = &self.min_severity {
            if event.severity < *min_severity {
                return false;
            }
        }

        // Check source pattern
        if let Some(pattern) = &self.source_pattern {
            if !event.source.contains(pattern) {
                return false;
            }
        }

        // Check VM ID
        if let Some(vm_id) = &self.vm_id {
            match &event.vm_id {
                Some(event_vm_id) if event_vm_id == vm_id => {}
                _ => return false,
            }
        }

        // Check name pattern
        if let Some(pattern) = &self.name_pattern {
            if !event.name.contains(pattern) {
                return false;
            }
        }

        true
    }
}

/// Subscription info
#[derive(Debug)]
struct Subscription {
    id: u64,
    filter: EventFilter,
    created_at: SystemTime,
    event_count: AtomicU64,
}

/// Event bus for publishing and subscribing to events
#[derive(Debug)]
pub struct EventBus {
    /// Broadcast sender
    sender: broadcast::Sender<VmEvent>,
    /// Subscriptions
    subscriptions: RwLock<HashMap<u64, Subscription>>,
    /// Next subscription ID
    next_sub_id: AtomicU64,
    /// Event history
    history: Mutex<Vec<VmEvent>>,
    /// Maximum history size
    max_history: usize,
    /// Total events published
    total_events: AtomicU64,
}

impl EventBus {
    /// Create a new event bus
    pub fn new(channel_capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(channel_capacity);
        Self {
            sender,
            subscriptions: RwLock::new(HashMap::new()),
            next_sub_id: AtomicU64::new(1),
            history: Mutex::new(Vec::new()),
            max_history: 1000,
            total_events: AtomicU64::new(0),
        }
    }

    /// Create with custom history size
    pub fn with_history_size(mut self, size: usize) -> Self {
        self.max_history = size;
        self
    }

    /// Publish an event
    pub fn publish(&self, event: VmEvent) {
        self.total_events.fetch_add(1, Ordering::Relaxed);

        // Store in history
        {
            let mut history = self.history.lock().unwrap();
            if history.len() >= self.max_history {
                history.remove(0);
            }
            history.push(event.clone());
        }

        // Update subscription counts for matching events
        let subs = self.subscriptions.read().unwrap();
        for sub in subs.values() {
            if sub.filter.matches(&event) {
                sub.event_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Broadcast to all receivers
        let _ = self.sender.send(event);
    }

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> (u64, EventReceiver) {
        let id = self.next_sub_id.fetch_add(1, Ordering::Relaxed);
        let receiver = self.sender.subscribe();

        let subscription = Subscription {
            id,
            filter: filter.clone(),
            created_at: SystemTime::now(),
            event_count: AtomicU64::new(0),
        };

        self.subscriptions.write().unwrap().insert(id, subscription);

        (
            id,
            EventReceiver {
                id,
                filter,
                receiver,
            },
        )
    }

    /// Unsubscribe
    pub fn unsubscribe(&self, id: u64) -> EventResult<()> {
        if self.subscriptions.write().unwrap().remove(&id).is_none() {
            return Err(EventError::SubscriberNotFound(id));
        }
        Ok(())
    }

    /// Get subscription count
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().unwrap().len()
    }

    /// Get total event count
    pub fn total_events(&self) -> u64 {
        self.total_events.load(Ordering::Relaxed)
    }

    /// Get event history
    pub fn history(&self) -> Vec<VmEvent> {
        self.history.lock().unwrap().clone()
    }

    /// Get filtered history
    pub fn filtered_history(&self, filter: &EventFilter) -> Vec<VmEvent> {
        self.history
            .lock()
            .unwrap()
            .iter()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect()
    }

    /// Clear history
    pub fn clear_history(&self) {
        self.history.lock().unwrap().clear();
    }

    /// Get recent events
    pub fn recent(&self, count: usize) -> Vec<VmEvent> {
        let history = self.history.lock().unwrap();
        history.iter().rev().take(count).cloned().collect()
    }

    /// Find event by ID
    pub fn find_event(&self, id: u64) -> Option<VmEvent> {
        self.history
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.id == id)
            .cloned()
    }

    /// Get events by correlation ID
    pub fn correlated_events(&self, correlation_id: u64) -> Vec<VmEvent> {
        self.history
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.correlation_id == Some(correlation_id))
            .cloned()
            .collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Event receiver for subscriptions
#[derive(Debug)]
pub struct EventReceiver {
    /// Subscription ID
    pub id: u64,
    /// Filter for this subscription
    filter: EventFilter,
    /// Broadcast receiver
    receiver: broadcast::Receiver<VmEvent>,
}

impl EventReceiver {
    /// Receive the next matching event
    pub async fn recv(&mut self) -> EventResult<VmEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Ok(event);
                    }
                    // Event didn't match filter, continue waiting
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(EventError::ChannelClosed);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    return Err(EventError::BufferOverflow(n as usize));
                }
            }
        }
    }

    /// Try to receive without blocking
    pub fn try_recv(&mut self) -> EventResult<Option<VmEvent>> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Ok(Some(event));
                    }
                    // Continue checking for more events
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    return Ok(None);
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    return Err(EventError::ChannelClosed);
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    return Err(EventError::BufferOverflow(n as usize));
                }
            }
        }
    }
}

/// Event aggregator for collecting event statistics
#[derive(Debug, Default)]
pub struct EventAggregator {
    /// Events by category
    by_category: RwLock<HashMap<EventCategory, u64>>,
    /// Events by severity
    by_severity: RwLock<HashMap<EventSeverity, u64>>,
    /// Events by source
    by_source: RwLock<HashMap<String, u64>>,
    /// Total events processed
    total: AtomicU64,
}

impl EventAggregator {
    /// Create a new aggregator
    pub fn new() -> Self {
        Self::default()
    }

    /// Process an event
    pub fn process(&self, event: &VmEvent) {
        self.total.fetch_add(1, Ordering::Relaxed);

        *self
            .by_category
            .write()
            .unwrap()
            .entry(event.category)
            .or_insert(0) += 1;

        *self
            .by_severity
            .write()
            .unwrap()
            .entry(event.severity)
            .or_insert(0) += 1;

        *self
            .by_source
            .write()
            .unwrap()
            .entry(event.source.clone())
            .or_insert(0) += 1;
    }

    /// Get count by category
    pub fn count_by_category(&self, category: EventCategory) -> u64 {
        *self
            .by_category
            .read()
            .unwrap()
            .get(&category)
            .unwrap_or(&0)
    }

    /// Get count by severity
    pub fn count_by_severity(&self, severity: EventSeverity) -> u64 {
        *self
            .by_severity
            .read()
            .unwrap()
            .get(&severity)
            .unwrap_or(&0)
    }

    /// Get count by source
    pub fn count_by_source(&self, source: &str) -> u64 {
        *self.by_source.read().unwrap().get(source).unwrap_or(&0)
    }

    /// Get total count
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Get all category counts
    pub fn category_counts(&self) -> HashMap<EventCategory, u64> {
        self.by_category.read().unwrap().clone()
    }

    /// Get all severity counts
    pub fn severity_counts(&self) -> HashMap<EventSeverity, u64> {
        self.by_severity.read().unwrap().clone()
    }

    /// Get all source counts
    pub fn source_counts(&self) -> HashMap<String, u64> {
        self.by_source.read().unwrap().clone()
    }

    /// Reset all counts
    pub fn reset(&self) {
        self.by_category.write().unwrap().clear();
        self.by_severity.write().unwrap().clear();
        self.by_source.write().unwrap().clear();
        self.total.store(0, Ordering::Relaxed);
    }
}

/// Event stream processor for custom handling
pub trait EventProcessor: Send + Sync {
    /// Process an event
    fn process(&self, event: &VmEvent);

    /// Get processor name
    fn name(&self) -> &str;
}

/// Logging event processor
#[derive(Debug)]
pub struct LoggingProcessor {
    name: String,
    min_severity: EventSeverity,
}

impl LoggingProcessor {
    /// Create a new logging processor
    pub fn new(name: impl Into<String>, min_severity: EventSeverity) -> Self {
        Self {
            name: name.into(),
            min_severity,
        }
    }
}

impl EventProcessor for LoggingProcessor {
    fn process(&self, event: &VmEvent) {
        if event.severity >= self.min_severity {
            println!(
                "[{}] {} {} - {} - {}",
                event.severity, event.category, event.source, event.name, event.message
            );
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_error_display() {
        let err = EventError::SubscriberNotFound(123);
        assert!(format!("{}", err).contains("123"));
    }

    #[test]
    fn test_event_category_display() {
        assert_eq!(format!("{}", EventCategory::Lifecycle), "lifecycle");
        assert_eq!(format!("{}", EventCategory::Security), "security");
    }

    #[test]
    fn test_event_severity_display() {
        assert_eq!(format!("{}", EventSeverity::Info), "INFO");
        assert_eq!(format!("{}", EventSeverity::Critical), "CRITICAL");
    }

    #[test]
    fn test_event_severity_ordering() {
        assert!(EventSeverity::Debug < EventSeverity::Info);
        assert!(EventSeverity::Warning < EventSeverity::Error);
        assert!(EventSeverity::Critical < EventSeverity::Emergency);
    }

    #[test]
    fn test_vm_event_creation() {
        let event = VmEvent::new(
            EventCategory::Lifecycle,
            EventSeverity::Info,
            "agent",
            "vm_started",
            "VM has started",
        );

        assert!(event.id > 0);
        assert_eq!(event.category, EventCategory::Lifecycle);
        assert_eq!(event.severity, EventSeverity::Info);
        assert_eq!(event.source, "agent");
    }

    #[test]
    fn test_vm_event_builder() {
        let event = VmEvent::new(
            EventCategory::Resource,
            EventSeverity::Warning,
            "monitor",
            "memory_high",
            "Memory usage high",
        )
        .with_vm_id("vm-123")
        .with_data("usage", serde_json::json!(85))
        .with_correlation(999);

        assert_eq!(event.vm_id, Some("vm-123".to_string()));
        assert_eq!(event.data.get("usage"), Some(&serde_json::json!(85)));
        assert_eq!(event.correlation_id, Some(999));
    }

    #[test]
    fn test_vm_event_helpers() {
        let lifecycle = VmEvent::lifecycle("agent", "start", "Starting");
        assert_eq!(lifecycle.category, EventCategory::Lifecycle);

        let resource = VmEvent::resource("monitor", "cpu", "CPU usage");
        assert_eq!(resource.category, EventCategory::Resource);

        let security =
            VmEvent::security("guard", "violation", "Access denied", EventSeverity::Error);
        assert_eq!(security.category, EventCategory::Security);
        assert_eq!(security.severity, EventSeverity::Error);

        let perf = VmEvent::performance("metrics", "latency", "High latency");
        assert_eq!(perf.category, EventCategory::Performance);

        let script = VmEvent::script("engine", "executed", "Script ran");
        assert_eq!(script.category, EventCategory::Script);
    }

    #[test]
    fn test_event_filter_category() {
        let filter = EventFilter::new().with_category(EventCategory::Security);

        let security_event = VmEvent::new(
            EventCategory::Security,
            EventSeverity::Warning,
            "test",
            "test",
            "test",
        );
        let lifecycle_event = VmEvent::new(
            EventCategory::Lifecycle,
            EventSeverity::Info,
            "test",
            "test",
            "test",
        );

        assert!(filter.matches(&security_event));
        assert!(!filter.matches(&lifecycle_event));
    }

    #[test]
    fn test_event_filter_severity() {
        let filter = EventFilter::new().with_min_severity(EventSeverity::Warning);

        let warning = VmEvent::new(
            EventCategory::System,
            EventSeverity::Warning,
            "test",
            "test",
            "test",
        );
        let info = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "test",
            "test",
            "test",
        );
        let error = VmEvent::new(
            EventCategory::System,
            EventSeverity::Error,
            "test",
            "test",
            "test",
        );

        assert!(filter.matches(&warning));
        assert!(!filter.matches(&info));
        assert!(filter.matches(&error));
    }

    #[test]
    fn test_event_filter_source() {
        let filter = EventFilter::new().with_source("agent");

        let agent_event = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "agent-1",
            "test",
            "test",
        );
        let other_event = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "monitor",
            "test",
            "test",
        );

        assert!(filter.matches(&agent_event));
        assert!(!filter.matches(&other_event));
    }

    #[test]
    fn test_event_filter_vm_id() {
        let filter = EventFilter::new().with_vm_id("vm-123");

        let matching = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "test",
            "test",
            "test",
        )
        .with_vm_id("vm-123");
        let not_matching = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "test",
            "test",
            "test",
        )
        .with_vm_id("vm-456");
        let no_vm = VmEvent::new(
            EventCategory::System,
            EventSeverity::Info,
            "test",
            "test",
            "test",
        );

        assert!(filter.matches(&matching));
        assert!(!filter.matches(&not_matching));
        assert!(!filter.matches(&no_vm));
    }

    #[test]
    fn test_event_filter_combined() {
        let filter = EventFilter::new()
            .with_category(EventCategory::Security)
            .with_min_severity(EventSeverity::Warning)
            .with_source("guard");

        let matching = VmEvent::new(
            EventCategory::Security,
            EventSeverity::Error,
            "guard-1",
            "violation",
            "test",
        );
        let wrong_category = VmEvent::new(
            EventCategory::Lifecycle,
            EventSeverity::Error,
            "guard-1",
            "violation",
            "test",
        );
        let wrong_severity = VmEvent::new(
            EventCategory::Security,
            EventSeverity::Info,
            "guard-1",
            "violation",
            "test",
        );
        let wrong_source = VmEvent::new(
            EventCategory::Security,
            EventSeverity::Error,
            "monitor",
            "violation",
            "test",
        );

        assert!(filter.matches(&matching));
        assert!(!filter.matches(&wrong_category));
        assert!(!filter.matches(&wrong_severity));
        assert!(!filter.matches(&wrong_source));
    }

    #[test]
    fn test_event_bus_publish() {
        let bus = EventBus::new(100);

        let event = VmEvent::lifecycle("test", "start", "Started");
        bus.publish(event);

        assert_eq!(bus.total_events(), 1);
        assert_eq!(bus.history().len(), 1);
    }

    #[test]
    fn test_event_bus_history() {
        let bus = EventBus::new(100).with_history_size(5);

        for i in 0..10 {
            bus.publish(VmEvent::lifecycle("test", &format!("event-{}", i), "test"));
        }

        let history = bus.history();
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_event_bus_filtered_history() {
        let bus = EventBus::new(100);

        bus.publish(VmEvent::lifecycle("agent", "start", "Started"));
        bus.publish(VmEvent::security(
            "guard",
            "violation",
            "Blocked",
            EventSeverity::Error,
        ));
        bus.publish(VmEvent::lifecycle("agent", "stop", "Stopped"));

        let filter = EventFilter::new().with_category(EventCategory::Lifecycle);
        let filtered = bus.filtered_history(&filter);

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_event_bus_recent() {
        let bus = EventBus::new(100);

        for i in 0..5 {
            bus.publish(VmEvent::lifecycle("test", &format!("event-{}", i), "test"));
        }

        let recent = bus.recent(2);
        assert_eq!(recent.len(), 2);
        assert!(recent[0].name.contains("4")); // Most recent first
    }

    #[test]
    fn test_event_bus_find_event() {
        let bus = EventBus::new(100);

        let event = VmEvent::lifecycle("test", "findme", "test");
        let id = event.id;
        bus.publish(event);

        let found = bus.find_event(id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "findme");

        let not_found = bus.find_event(99999);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_event_bus_correlated_events() {
        let bus = EventBus::new(100);

        let corr_id = 12345u64;
        bus.publish(VmEvent::lifecycle("test", "event1", "test").with_correlation(corr_id));
        bus.publish(VmEvent::lifecycle("test", "event2", "test")); // No correlation
        bus.publish(VmEvent::lifecycle("test", "event3", "test").with_correlation(corr_id));

        let correlated = bus.correlated_events(corr_id);
        assert_eq!(correlated.len(), 2);
    }

    #[test]
    fn test_event_bus_subscribe() {
        let bus = EventBus::new(100);
        let filter = EventFilter::new();

        let (id, _receiver) = bus.subscribe(filter);
        assert!(id > 0);
        assert_eq!(bus.subscription_count(), 1);
    }

    #[test]
    fn test_event_bus_unsubscribe() {
        let bus = EventBus::new(100);
        let (id, _) = bus.subscribe(EventFilter::new());

        assert!(bus.unsubscribe(id).is_ok());
        assert_eq!(bus.subscription_count(), 0);

        assert!(matches!(
            bus.unsubscribe(id),
            Err(EventError::SubscriberNotFound(_))
        ));
    }

    #[test]
    fn test_event_aggregator() {
        let aggregator = EventAggregator::new();

        aggregator.process(&VmEvent::lifecycle("test", "start", "test"));
        aggregator.process(&VmEvent::lifecycle("test", "stop", "test"));
        aggregator.process(&VmEvent::security(
            "guard",
            "alert",
            "test",
            EventSeverity::Warning,
        ));

        assert_eq!(aggregator.total(), 3);
        assert_eq!(aggregator.count_by_category(EventCategory::Lifecycle), 2);
        assert_eq!(aggregator.count_by_category(EventCategory::Security), 1);
        assert_eq!(aggregator.count_by_source("test"), 2);
        assert_eq!(aggregator.count_by_source("guard"), 1);
    }

    #[test]
    fn test_event_aggregator_counts() {
        let aggregator = EventAggregator::new();

        aggregator.process(&VmEvent::new(
            EventCategory::Lifecycle,
            EventSeverity::Info,
            "src1",
            "test",
            "test",
        ));
        aggregator.process(&VmEvent::new(
            EventCategory::Security,
            EventSeverity::Error,
            "src2",
            "test",
            "test",
        ));

        let category_counts = aggregator.category_counts();
        assert_eq!(category_counts.len(), 2);

        let severity_counts = aggregator.severity_counts();
        assert_eq!(severity_counts.len(), 2);

        let source_counts = aggregator.source_counts();
        assert_eq!(source_counts.len(), 2);
    }

    #[test]
    fn test_event_aggregator_reset() {
        let aggregator = EventAggregator::new();

        aggregator.process(&VmEvent::lifecycle("test", "test", "test"));
        aggregator.reset();

        assert_eq!(aggregator.total(), 0);
        assert!(aggregator.category_counts().is_empty());
    }

    #[test]
    fn test_logging_processor() {
        let processor = LoggingProcessor::new("test-logger", EventSeverity::Warning);
        assert_eq!(processor.name(), "test-logger");

        // Just ensure it doesn't panic
        processor.process(&VmEvent::new(
            EventCategory::System,
            EventSeverity::Error,
            "test",
            "test",
            "test message",
        ));
    }

    #[test]
    fn test_event_error_variants() {
        let errors = vec![
            EventError::SubscriberNotFound(1),
            EventError::ChannelClosed,
            EventError::BufferOverflow(100),
            EventError::InvalidFilter("bad".to_string()),
            EventError::EventNotFound(1),
        ];

        for err in errors {
            assert!(!format!("{}", err).is_empty());
        }
    }

    #[test]
    fn test_filter_multiple_categories() {
        let filter = EventFilter::new()
            .with_categories(vec![EventCategory::Lifecycle, EventCategory::Security]);

        let lifecycle = VmEvent::lifecycle("test", "test", "test");
        let security = VmEvent::security("test", "test", "test", EventSeverity::Info);
        let resource = VmEvent::resource("test", "test", "test");

        assert!(filter.matches(&lifecycle));
        assert!(filter.matches(&security));
        assert!(!filter.matches(&resource));
    }

    #[test]
    fn test_clear_history() {
        let bus = EventBus::new(100);

        bus.publish(VmEvent::lifecycle("test", "test", "test"));
        assert_eq!(bus.history().len(), 1);

        bus.clear_history();
        assert!(bus.history().is_empty());
    }
}
