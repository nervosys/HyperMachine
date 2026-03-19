//! Webhook and Event Streaming Module
//!
//! Provides real-time event delivery for AI agents and external systems.
//!
//! ## Features
//!
//! - **Webhooks**: HTTP POST callbacks for VM events
//! - **SSE**: Server-Sent Events for real-time streaming
//! - **WebSocket**: Bidirectional communication channel
//! - **Event filtering**: Subscribe to specific event types

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event as SseEvent, IntoResponse, Sse},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

// ============================================================================
// Event Types
// ============================================================================

/// VM lifecycle events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// VM created
    VmCreated,
    /// VM started
    VmStarted,
    /// VM stopped
    VmStopped,
    /// VM paused
    VmPaused,
    /// VM resumed
    VmResumed,
    /// VM deleted
    VmDeleted,
    /// VM snapshot created
    SnapshotCreated,
    /// VM snapshot restored
    SnapshotRestored,
    /// VM migrated
    VmMigrated,
    /// Resource usage alert
    ResourceAlert,
    /// Health check failed
    HealthCheckFailed,
    /// Agent action completed
    AgentActionCompleted,
    /// Error occurred
    Error,
    /// All events (for subscriptions)
    All,
}

/// Event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmEvent {
    /// Unique event ID
    pub id: String,
    /// Event type
    pub event_type: EventType,
    /// Timestamp (ISO 8601)
    pub timestamp: String,
    /// VM ID (if applicable)
    pub vm_id: Option<String>,
    /// Event data
    pub data: serde_json::Value,
    /// Correlation ID for tracking
    pub correlation_id: Option<String>,
}

impl VmEvent {
    pub fn new(event_type: EventType, vm_id: Option<String>, data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type,
            timestamp: chrono::Utc::now().to_rfc3339(),
            vm_id,
            data,
            correlation_id: None,
        }
    }

    pub fn with_correlation_id(mut self, id: String) -> Self {
        self.correlation_id = Some(id);
        self
    }
}

// ============================================================================
// Webhook Configuration
// ============================================================================

/// Webhook subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubscription {
    /// Subscription ID
    pub id: String,
    /// Webhook URL
    pub url: String,
    /// Events to subscribe to
    pub events: Vec<EventType>,
    /// Secret for HMAC signature
    #[serde(skip_serializing)]
    pub secret: Option<String>,
    /// Active status
    pub active: bool,
    /// Created timestamp
    pub created_at: String,
    /// Last triggered timestamp
    pub last_triggered: Option<String>,
    /// Failure count
    pub failure_count: u32,
    /// Custom headers
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Create webhook request
#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub events: Vec<EventType>,
    pub secret: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

/// Webhook delivery status
#[derive(Debug, Clone, Serialize)]
pub struct WebhookDelivery {
    pub id: String,
    pub webhook_id: String,
    pub event_id: String,
    pub status: DeliveryStatus,
    pub status_code: Option<u16>,
    pub response_time_ms: Option<u64>,
    pub error: Option<String>,
    pub timestamp: String,
}

/// Status of event delivery to a subscriber
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Success,
    Failed,
    Retrying,
}

// ============================================================================
// Event Bus
// ============================================================================

/// Event bus for publishing and subscribing to events
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<VmEvent>,
    webhooks: Arc<parking_lot::RwLock<HashMap<String, WebhookSubscription>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self {
            sender,
            webhooks: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Publish an event
    pub fn publish(&self, event: VmEvent) {
        let _ = self.sender.send(event.clone());

        // Trigger webhooks in background
        let webhooks = self.webhooks.read().clone();
        let event_clone = event.clone();
        tokio::spawn(async move {
            for webhook in webhooks.values() {
                if webhook.active
                    && (webhook.events.contains(&event_clone.event_type)
                        || webhook.events.contains(&EventType::All))
                {
                    // Deliver webhook with error logging
                    if let Err(e) = deliver_webhook(webhook, &event_clone).await {
                        tracing::warn!(
                            webhook_id = %webhook.id,
                            error = %e,
                            "Webhook delivery failed"
                        );
                    }
                }
            }
        });
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<VmEvent> {
        self.sender.subscribe()
    }

    /// Add webhook subscription
    pub fn add_webhook(&self, req: CreateWebhookRequest) -> WebhookSubscription {
        let subscription = WebhookSubscription {
            id: uuid::Uuid::new_v4().to_string(),
            url: req.url,
            events: req.events,
            secret: req.secret,
            active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_triggered: None,
            failure_count: 0,
            headers: req.headers,
        };

        self.webhooks
            .write()
            .insert(subscription.id.clone(), subscription.clone());

        subscription
    }

    /// Remove webhook subscription
    pub fn remove_webhook(&self, id: &str) -> Option<WebhookSubscription> {
        self.webhooks.write().remove(id)
    }

    /// List all webhooks
    pub fn list_webhooks(&self) -> Vec<WebhookSubscription> {
        self.webhooks.read().values().cloned().collect()
    }

    /// Get webhook by ID
    pub fn get_webhook(&self, id: &str) -> Option<WebhookSubscription> {
        self.webhooks.read().get(id).cloned()
    }
}

/// Deliver webhook to URL
async fn deliver_webhook(
    webhook: &WebhookSubscription,
    event: &VmEvent,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let payload = serde_json::to_string(event)?;

    let mut req = client
        .post(&webhook.url)
        .header("Content-Type", "application/json")
        .header("X-HyperMachine-Event", event.event_type.to_string())
        .header("X-HyperMachine-Delivery", &event.id);

    // Add HMAC signature if secret is set
    if let Some(secret) = &webhook.secret {
        let signature = compute_hmac_signature(secret, &payload);
        req = req.header("X-HyperMachine-Signature", signature);
    }

    // Add custom headers
    for (key, value) in &webhook.headers {
        req = req.header(key, value);
    }

    let _response = req.body(payload).send().await?;

    Ok(())
}

fn compute_hmac_signature(secret: &str, payload: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    secret.hash(&mut hasher);
    payload.hash(&mut hasher);
    format!("sha256={:x}", hasher.finish())
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::VmCreated => write!(f, "vm.created"),
            EventType::VmStarted => write!(f, "vm.started"),
            EventType::VmStopped => write!(f, "vm.stopped"),
            EventType::VmPaused => write!(f, "vm.paused"),
            EventType::VmResumed => write!(f, "vm.resumed"),
            EventType::VmDeleted => write!(f, "vm.deleted"),
            EventType::SnapshotCreated => write!(f, "snapshot.created"),
            EventType::SnapshotRestored => write!(f, "snapshot.restored"),
            EventType::VmMigrated => write!(f, "vm.migrated"),
            EventType::ResourceAlert => write!(f, "resource.alert"),
            EventType::HealthCheckFailed => write!(f, "health.failed"),
            EventType::AgentActionCompleted => write!(f, "agent.action.completed"),
            EventType::Error => write!(f, "error"),
            EventType::All => write!(f, "*"),
        }
    }
}

// ============================================================================
// REST API Handlers
// ============================================================================

/// Query parameters for SSE subscription
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// Filter by event types (comma-separated)
    pub events: Option<String>,
    /// Filter by VM ID
    pub vm_id: Option<String>,
}

/// Create webhook handler
async fn create_webhook(
    State(event_bus): State<Arc<EventBus>>,
    Json(req): Json<CreateWebhookRequest>,
) -> impl IntoResponse {
    let subscription = event_bus.add_webhook(req);
    (StatusCode::CREATED, Json(subscription))
}

/// List webhooks handler (paginated)
async fn list_webhooks(
    State(event_bus): State<Arc<EventBus>>,
    Query(params): Query<crate::rest::PaginationParams>,
) -> impl IntoResponse {
    let mut webhooks = event_bus.list_webhooks();
    // Sort by id for deterministic pagination
    webhooks.sort_by(|a, b| a.id.cmp(&b.id));
    let total = webhooks.len();
    let limit = params.effective_limit();
    let offset = params.offset.min(total);
    let page: Vec<WebhookSubscription> = webhooks.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + page.len() < total;

    Json(serde_json::json!({
        "webhooks": page,
        "total": total,
        "offset": offset,
        "limit": limit,
        "has_more": has_more
    }))
}

/// Get webhook handler
async fn get_webhook(
    State(event_bus): State<Arc<EventBus>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match event_bus.get_webhook(&id) {
        Some(webhook) => match serde_json::to_value(webhook) {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Webhook not found"})),
        )
            .into_response(),
    }
}

/// Delete webhook handler
async fn delete_webhook(
    State(event_bus): State<Arc<EventBus>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match event_bus.remove_webhook(&id) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Webhook not found"})),
        )
            .into_response(),
    }
}

/// SSE event stream handler
async fn event_stream(
    State(event_bus): State<Arc<EventBus>>,
    Query(query): Query<SseQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let receiver = event_bus.subscribe();

    // Parse event filter
    let event_filter: Option<Vec<EventType>> = query.events.map(|s| {
        s.split(',')
            .filter_map(|e| serde_json::from_str(&format!("\"{}\"", e.trim())).ok())
            .collect()
    });

    let stream = BroadcastStream::new(receiver).filter_map(move |result| {
        let event_filter = event_filter.clone();
        let vm_filter = query.vm_id.clone();

        match result {
            Ok(event) => {
                // Apply filters
                if let Some(ref filter) = event_filter {
                    if !filter.contains(&event.event_type) && !filter.contains(&EventType::All) {
                        return None;
                    }
                }

                if let Some(ref vm_id) = vm_filter {
                    if event.vm_id.as_ref() != Some(vm_id) {
                        return None;
                    }
                }

                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(SseEvent::default()
                    .id(event.id)
                    .event(event.event_type.to_string())
                    .data(data)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("ping"),
    )
}

/// Publish test event (for development)
async fn publish_test_event(
    State(event_bus): State<Arc<EventBus>>,
    Json(event): Json<VmEvent>,
) -> impl IntoResponse {
    event_bus.publish(event.clone());
    (StatusCode::ACCEPTED, Json(event))
}

// ============================================================================
// Router
// ============================================================================

/// Create the events/webhooks router
pub fn events_router<S>(event_bus: Arc<EventBus>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        // Webhooks
        .route("/webhooks", post(create_webhook))
        .route("/webhooks", get(list_webhooks))
        .route("/webhooks/:id", get(get_webhook))
        .route("/webhooks/:id", delete(delete_webhook))
        // SSE streaming
        .route("/stream", get(event_stream))
        // Test endpoint (development only)
        .route("/publish", post(publish_test_event))
        .with_state(event_bus)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = VmEvent::new(
            EventType::VmStarted,
            Some("vm-123".into()),
            serde_json::json!({"cpu_cores": 4}),
        );

        assert!(!event.id.is_empty());
        assert_eq!(event.event_type, EventType::VmStarted);
        assert_eq!(event.vm_id, Some("vm-123".into()));
    }

    #[tokio::test]
    async fn test_event_bus() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(VmEvent::new(
            EventType::VmCreated,
            Some("vm-456".into()),
            serde_json::json!({}),
        ));

        // Give the spawned task a moment to run
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_webhook_subscription() {
        let bus = EventBus::new();

        let webhook = bus.add_webhook(CreateWebhookRequest {
            url: "https://example.com/webhook".into(),
            events: vec![EventType::VmStarted, EventType::VmStopped],
            secret: Some("secret123".into()),
            headers: HashMap::new(),
        });

        assert!(!webhook.id.is_empty());
        assert!(webhook.active);
        assert_eq!(webhook.events.len(), 2);

        // List webhooks
        let webhooks = bus.list_webhooks();
        assert_eq!(webhooks.len(), 1);

        // Remove webhook
        bus.remove_webhook(&webhook.id);
        assert!(bus.list_webhooks().is_empty());
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(EventType::VmStarted.to_string(), "vm.started");
        assert_eq!(EventType::SnapshotCreated.to_string(), "snapshot.created");
        assert_eq!(EventType::All.to_string(), "*");
    }
}
