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
use std::net::{IpAddr, SocketAddr};
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
    /// Secret for the HMAC-SHA256 delivery signature (see [`SIGNATURE_HEADER`])
    #[serde(skip_serializing)]
    pub secret: Option<String>,
    /// Active status
    pub active: bool,
    /// Created timestamp
    pub created_at: String,
    /// Last triggered timestamp
    pub last_triggered: Option<String>,
    /// Consecutive failed deliveries; returns to zero on a success.
    pub failure_count: u32,
    /// Custom headers. Values are redacted (`[redacted]`) whenever a
    /// subscription is serialised, since they usually carry credentials.
    #[serde(default, serialize_with = "redact_header_values")]
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
    /// Let webhooks target loopback/private/link-local addresses. Off by
    /// default: the API server would otherwise be a request relay into its
    /// own network.
    allow_private_webhooks: bool,
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
            allow_private_webhooks: false,
        }
    }

    /// Allow (or, the default, refuse) webhook URLs that point at
    /// non-public addresses. See `ServerConfig::allow_private_webhooks`.
    #[must_use]
    pub fn with_allow_private_webhooks(mut self, allow: bool) -> Self {
        self.allow_private_webhooks = allow;
        self
    }

    /// Whether webhooks may target non-public addresses.
    pub fn allow_private_webhooks(&self) -> bool {
        self.allow_private_webhooks
    }

    /// Publish an event
    pub fn publish(&self, event: VmEvent) {
        let _ = self.sender.send(event.clone());

        // Trigger webhooks in background
        let webhooks = self.webhooks.read().clone();
        let store = Arc::clone(&self.webhooks);
        let allow_private = self.allow_private_webhooks;
        let event_clone = event.clone();
        tokio::spawn(async move {
            for webhook in webhooks.values() {
                if webhook.active
                    && (webhook.events.contains(&event_clone.event_type)
                        || webhook.events.contains(&EventType::All))
                {
                    // Deliver webhook with error logging
                    let outcome = deliver_webhook(webhook, &event_clone, allow_private).await;
                    record_delivery(&store, &webhook.id, outcome.is_ok());
                    if let Err(e) = outcome {
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

/// Header carrying the delivery signature: `sha256=<hex>`, where hex is
/// HMAC-SHA256, keyed by the webhook's secret, over the exact request body.
/// Absent when the webhook has no secret. Same format as GitHub's
/// `X-Hub-Signature-256`, so a receiver verifies it the same way.
pub const SIGNATURE_HEADER: &str = "X-HyperMachine-Signature";

/// How long one delivery may take, connect included.
const DELIVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// `sha256=<hex>`: HMAC-SHA256 of `payload` under `secret`.
fn compute_hmac_signature(secret: &str, payload: &str) -> String {
    format!(
        "sha256={}",
        crate::middleware::hmac_sha256(secret.as_bytes(), payload.as_bytes())
    )
}

/// Headers the server sets itself; a subscription may not override them.
/// Letting it would let a caller forge `X-HyperMachine-Signature` or confuse
/// the framing of the request.
fn is_reserved_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("x-hypermachine-")
        || matches!(
            name.as_str(),
            "host" | "content-length" | "content-type" | "transfer-encoding" | "connection"
        )
}

/// Whether `ip` is an address on the public internet -- not loopback,
/// private, link-local (the cloud metadata service), shared (CGNAT),
/// multicast, documentation, benchmarking, unspecified, or a tunnel prefix
/// (6to4, Teredo, NAT64) that reaches whatever its relay can. IPv4-mapped
/// IPv6 addresses are judged as the IPv4 address they carry.
///
/// A local copy of `hv2_cluster::events::is_global`: that crate depends on
/// this one, so it cannot be shared from here without moving it down.
#[must_use]
pub fn is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b)) // shared, RFC 6598
                || (a == 192 && b == 0)
                || (a == 198 && (b & 0xfe) == 18) // benchmarking, 198.18.0.0/15
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_global(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            let first = seg[0];
            // NAT64 (RFC 6052): the IPv4 destination rides in the last 32 bits.
            if first == 0x0064 && seg[1] == 0xff9b {
                if seg[2..6] == [0, 0, 0, 0] {
                    let [a, b] = seg[6].to_be_bytes();
                    let [c, d] = seg[7].to_be_bytes();
                    return is_global(IpAddr::V4(std::net::Ipv4Addr::new(a, b, c, d)));
                }
                return false;
            }
            // 6to4 and Teredo tunnel to addresses the name's owner picks.
            if first == 0x2002 || (first == 0x2001 && seg[1] == 0) {
                return false;
            }
            // Deprecated IPv4-compatible (::a.b.c.d) addresses too.
            if seg[..6] == [0, 0, 0, 0, 0, 0] {
                return false;
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80 // link-local
                || (first == 0x2001 && seg[1] == 0x0db8))
        }
    }
}

/// Resolve `host` and return its addresses, or an error if the name has none
/// or *any* of them is not public. Refusing a mixed answer outright means a
/// name cannot be steered to an internal address by adding one record.
async fn resolve_public(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("{host} resolves to no address"));
    }
    if let Some(bad) = addrs.iter().find(|a| !is_global(a.ip())) {
        return Err(format!(
            "{host} resolves to {}, which is not a public address",
            bad.ip()
        ));
    }
    Ok(addrs)
}

/// The resolver the delivery client connects through. The addresses it hands
/// back are the ones that were checked, and the client connects to exactly
/// those: there is no second lookup between the check and the connection for
/// a DNS-rebinding name to answer differently.
struct PublicOnlyResolver;

impl reqwest::dns::Resolve for PublicOnlyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addrs = resolve_public(name.as_str(), 0)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            let it: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(it)
        })
    }
}

/// Check a webhook URL: http or https, a host, and -- unless `allow_private`
/// -- a host that is, or resolves only to, public addresses.
///
/// Registration runs this for an early 400; delivery runs the same checks
/// again (and pins the connection to what it checked), because a name can
/// change its answer after registration.
pub async fn validate_webhook_url(url: &str, allow_private: bool) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid webhook url: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "webhook url scheme must be http or https, not {}",
            parsed.scheme()
        ));
    }
    if allow_private {
        return Ok(());
    }
    let host = parsed.host_str().ok_or("webhook url has no host")?;
    // The url crate has already normalised decimal/hex/octal IPv4 forms.
    if let Ok(ip) = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
    {
        return check_literal(ip);
    }
    let port = parsed.port_or_known_default().unwrap_or(0);
    match resolve_public(host, port).await {
        Ok(_) => Ok(()),
        // A name that does not resolve yet is not refused here: it is
        // checked again, and pinned, when it is delivered to.
        Err(e) if e.starts_with("cannot resolve") => Ok(()),
        Err(e) => Err(e),
    }
}

fn check_literal(ip: IpAddr) -> Result<(), String> {
    if is_global(ip) {
        Ok(())
    } else {
        Err(format!("webhook url host {ip} is not a public address"))
    }
}

/// Deliver webhook to URL. Returns the response status on a 2xx; any other
/// status, a refused address, or a transport error is an `Err`.
async fn deliver_webhook(
    webhook: &WebhookSubscription,
    event: &VmEvent,
    allow_private: bool,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    // Redirects are never followed: a public URL that answers 302 to
    // http://169.254.169.254/ would otherwise walk the request past the check.
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(DELIVERY_TIMEOUT);
    if !allow_private {
        validate_webhook_url(&webhook.url, false).await?;
        builder = builder
            .dns_resolver(Arc::new(PublicOnlyResolver))
            // A proxy would resolve the name itself, past the resolver above.
            .no_proxy();
    }
    let client = builder.build()?;
    let payload = serde_json::to_string(event)?;

    let mut req = client
        .post(&webhook.url)
        .header("Content-Type", "application/json")
        .header("X-HyperMachine-Event", event.event_type.to_string())
        .header("X-HyperMachine-Delivery", &event.id);

    // Sign the exact bytes sent, if there is a secret.
    if let Some(secret) = webhook.secret.as_deref().filter(|s| !s.is_empty()) {
        req = req.header(SIGNATURE_HEADER, compute_hmac_signature(secret, &payload));
    }

    // Add custom headers
    for (key, value) in &webhook.headers {
        if !is_reserved_header(key) {
            req = req.header(key, value);
        }
    }

    let response = req.body(payload).send().await?;
    let status = response.status();
    if status.is_success() {
        Ok(status.as_u16())
    } else {
        Err(format!("receiver answered {status}").into())
    }
}

/// Record a delivery attempt on the stored subscription: `last_triggered` is
/// the time of the attempt; `failure_count` counts consecutive failures and
/// returns to zero on a success.
fn record_delivery(
    webhooks: &parking_lot::RwLock<HashMap<String, WebhookSubscription>>,
    id: &str,
    succeeded: bool,
) {
    if let Some(w) = webhooks.write().get_mut(id) {
        w.last_triggered = Some(chrono::Utc::now().to_rfc3339());
        w.failure_count = if succeeded {
            0
        } else {
            w.failure_count.saturating_add(1)
        };
    }
}

/// Serialise header values as `[redacted]`: they are typically credentials
/// (`Authorization`), and a read route has no business returning them.
fn redact_header_values<S: serde::Serializer>(
    headers: &HashMap<String, String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(headers.len()))?;
    for name in headers.keys() {
        map.serialize_entry(name, "[redacted]")?;
    }
    map.end()
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
    if let Err(error) = validate_request(&req, event_bus.allow_private_webhooks).await {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response();
    }
    let subscription = event_bus.add_webhook(req);
    (StatusCode::CREATED, Json(subscription)).into_response()
}

/// Registration-time checks: the URL, the secret, and the custom headers.
async fn validate_request(req: &CreateWebhookRequest, allow_private: bool) -> Result<(), String> {
    validate_webhook_url(&req.url, allow_private).await?;
    if req.secret.as_deref() == Some("") {
        return Err("secret must not be empty; omit it for an unsigned webhook".to_string());
    }
    for (name, value) in &req.headers {
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("invalid header name {name:?}"))?;
        reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("invalid value for header {name}"))?;
        if is_reserved_header(name) {
            return Err(format!("header {name} is set by the server"));
        }
    }
    Ok(())
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
        .route("/webhooks/{id}", get(get_webhook))
        .route("/webhooks/{id}", delete(delete_webhook))
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

    // ------------------------------------------------------------------
    // Webhook delivery: signature, address checks, redirects, bookkeeping
    // ------------------------------------------------------------------

    use axum::http::HeaderMap;

    /// What a loopback receiver saw.
    type Seen = Arc<parking_lot::Mutex<Vec<(HeaderMap, String)>>>;

    /// A receiver on 127.0.0.1 that records every request to `/hook`, answers
    /// `/redirect` with a 302 to `/hook`, and `/fail` with a 500.
    async fn receiver() -> (SocketAddr, Seen) {
        let seen: Seen = Arc::default();
        let sink = Arc::clone(&seen);
        let app = Router::new()
            .route(
                "/hook",
                post(move |headers: HeaderMap, body: String| {
                    let sink = Arc::clone(&sink);
                    async move {
                        sink.lock().push((headers, body));
                        StatusCode::OK
                    }
                }),
            )
            .route(
                "/redirect",
                post(|| async { (StatusCode::FOUND, [("location", "/hook")]) }),
            )
            .route(
                "/fail",
                post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, seen)
    }

    fn subscription(url: String, secret: Option<String>) -> WebhookSubscription {
        let bus = EventBus::new();
        bus.add_webhook(CreateWebhookRequest {
            url,
            events: vec![EventType::All],
            secret,
            headers: HashMap::new(),
        })
    }

    fn event() -> VmEvent {
        VmEvent::new(
            EventType::VmStarted,
            Some("vm-1".into()),
            serde_json::json!({"n": 1}),
        )
    }

    /// RFC 4231 test case 2 -- the published vector, not one computed by the
    /// code under test.
    #[test]
    fn signature_matches_rfc_4231_case_2() {
        assert_eq!(
            compute_hmac_signature("Jefe", "what do ya want for nothing?"),
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// The header a receiver gets verifies against the body it got, the way
    /// a receiver would: recompute over the bytes received, compare.
    #[tokio::test]
    async fn delivered_signature_verifies_against_delivered_body() {
        let (addr, seen) = receiver().await;
        let secret = "s".repeat(32);
        let hook = subscription(format!("http://{addr}/hook"), Some(secret.clone()));
        deliver_webhook(&hook, &event(), true).await.unwrap();

        let seen = seen.lock();
        let (headers, body) = &seen[0];
        let header = headers.get(SIGNATURE_HEADER).unwrap().to_str().unwrap();
        let hex = header.strip_prefix("sha256=").expect("sha256= prefix");
        assert_eq!(hex.len(), 64);
        assert_eq!(
            hex,
            crate::middleware::hmac_sha256(secret.as_bytes(), body.as_bytes())
        );
        // A different secret does not verify.
        assert_ne!(
            hex,
            crate::middleware::hmac_sha256("t".repeat(32).as_bytes(), body.as_bytes())
        );
    }

    #[tokio::test]
    async fn no_secret_means_no_signature_header() {
        let (addr, seen) = receiver().await;
        let hook = subscription(format!("http://{addr}/hook"), None);
        deliver_webhook(&hook, &event(), true).await.unwrap();
        assert!(seen.lock()[0].0.get(SIGNATURE_HEADER).is_none());
    }

    /// A subscription's own headers cannot override the signature.
    #[tokio::test]
    async fn custom_headers_cannot_forge_the_signature() {
        let (addr, seen) = receiver().await;
        let mut hook = subscription(format!("http://{addr}/hook"), Some("k".repeat(32)));
        hook.headers
            .insert("X-HyperMachine-Signature".into(), "sha256=forged".into());
        hook.headers.insert("X-Custom".into(), "yes".into());
        deliver_webhook(&hook, &event(), true).await.unwrap();
        let seen = seen.lock();
        let all: Vec<_> = seen[0].0.get_all(SIGNATURE_HEADER).iter().collect();
        assert_eq!(all.len(), 1);
        assert_ne!(all[0], "sha256=forged");
        assert_eq!(seen[0].0.get("x-custom").unwrap(), "yes");
    }

    #[test]
    fn only_public_addresses_are_global() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
        ] {
            assert!(!is_global(ip.parse().unwrap()), "{ip} must be refused");
        }
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_global(ip.parse().unwrap()), "{ip} must be allowed");
        }
    }

    #[tokio::test]
    async fn url_validation() {
        for url in [
            "ftp://example.com/x",
            "file:///etc/passwd",
            "gopher://example.com/",
            "not a url",
            "http://127.0.0.1/hook",
            "http://127.1/hook",
            "http://2130706433/hook",
            "http://0x7f.0.0.1/hook",
            "http://[::1]/hook",
            "http://[::ffff:10.0.0.1]/hook",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost/hook",
        ] {
            assert!(
                validate_webhook_url(url, false).await.is_err(),
                "{url} must be refused"
            );
        }
        assert!(validate_webhook_url("https://8.8.8.8/hook", false)
            .await
            .is_ok());
        // The opt-out lifts the address check, not the scheme check.
        assert!(validate_webhook_url("http://127.0.0.1/hook", true)
            .await
            .is_ok());
        assert!(validate_webhook_url("ftp://127.0.0.1/hook", true)
            .await
            .is_err());
    }

    /// A name that resolves to a loopback address is refused when the
    /// resolver the client connects through is asked for it -- the check the
    /// connection itself goes through, not only the one before it.
    #[tokio::test]
    async fn the_connecting_resolver_refuses_non_public_answers() {
        use reqwest::dns::Resolve;
        let name: reqwest::dns::Name = "localhost".parse().unwrap();
        assert!(PublicOnlyResolver.resolve(name).await.is_err());
    }

    #[tokio::test]
    async fn delivery_to_a_private_address_is_refused_and_nothing_is_sent() {
        let (addr, seen) = receiver().await;
        for host in [addr.to_string(), format!("localhost:{}", addr.port())] {
            let hook = subscription(format!("http://{host}/hook"), None);
            assert!(
                deliver_webhook(&hook, &event(), false).await.is_err(),
                "{host}"
            );
        }
        assert!(seen.lock().is_empty());
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let (addr, seen) = receiver().await;
        let hook = subscription(format!("http://{addr}/redirect"), None);
        let err = deliver_webhook(&hook, &event(), true).await.unwrap_err();
        assert!(err.to_string().contains("302"), "{err}");
        assert!(seen.lock().is_empty(), "the redirect target was requested");
    }

    #[test]
    fn header_values_are_redacted_but_names_are_kept() {
        let bus = EventBus::new();
        let hook = bus.add_webhook(CreateWebhookRequest {
            url: "https://example.com/hook".into(),
            events: vec![EventType::All],
            secret: Some("k".repeat(32)),
            headers: HashMap::from([("Authorization".to_string(), "Bearer abc".to_string())]),
        });
        let json = serde_json::to_value(&hook).unwrap();
        assert_eq!(json["headers"]["Authorization"], "[redacted]");
        assert!(!json.to_string().contains("Bearer abc"));
        assert!(json.get("secret").is_none());
    }

    #[tokio::test]
    async fn delivery_updates_failure_count_and_last_triggered() {
        let (addr, _seen) = receiver().await;
        let bus = EventBus::new().with_allow_private_webhooks(true);
        let bad = bus.add_webhook(CreateWebhookRequest {
            url: format!("http://{addr}/fail"),
            events: vec![EventType::All],
            secret: None,
            headers: HashMap::new(),
        });
        let good = bus.add_webhook(CreateWebhookRequest {
            url: format!("http://{addr}/hook"),
            events: vec![EventType::All],
            secret: None,
            headers: HashMap::new(),
        });
        bus.publish(event());
        bus.publish(event());
        for _ in 0..100 {
            let b = bus.get_webhook(&bad.id).unwrap();
            let g = bus.get_webhook(&good.id).unwrap();
            if b.failure_count == 2 && g.last_triggered.is_some() {
                assert_eq!(g.failure_count, 0);
                assert!(b.last_triggered.is_some());
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("delivery bookkeeping never updated");
    }

    /// Registration answers 400 for an internal target and 201 for a public
    /// one (a literal address, so no DNS), and the opt-out admits the former.
    #[tokio::test]
    async fn registration_validates_url_and_headers() {
        async fn register(bus: Arc<EventBus>, body: serde_json::Value) -> u16 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app: Router = events_router(bus);
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            reqwest::Client::new()
                .post(format!("http://{addr}/webhooks"))
                .json(&body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
        let hook = |url: &str| serde_json::json!({"url": url, "events": ["all"]});
        let bus = Arc::new(EventBus::new());
        assert_eq!(
            register(bus.clone(), hook("http://127.0.0.1:9/h")).await,
            400
        );
        assert_eq!(
            register(bus.clone(), hook("http://169.254.169.254/")).await,
            400
        );
        assert_eq!(register(bus.clone(), hook("file:///etc/passwd")).await, 400);
        assert_eq!(register(bus.clone(), hook("https://8.8.8.8/h")).await, 201);
        let mut with_forged = hook("https://8.8.8.8/h");
        with_forged["headers"] = serde_json::json!({"X-HyperMachine-Signature": "x"});
        assert_eq!(register(bus.clone(), with_forged).await, 400);
        let mut empty_secret = hook("https://8.8.8.8/h");
        empty_secret["secret"] = "".into();
        assert_eq!(register(bus.clone(), empty_secret).await, 400);
        let open = Arc::new(EventBus::new().with_allow_private_webhooks(true));
        assert_eq!(register(open, hook("http://127.0.0.1:9/h")).await, 201);
    }
}
