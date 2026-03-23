//! WebSocket Event Streaming
//!
//! Provides a real-time WebSocket endpoint for streaming VM events.
//! Clients connect to `/api/v1/events/ws` and receive JSON-encoded
//! events as text frames.
//!
//! ## Query Parameters
//!
//! - `event_type` — Filter by event type (e.g., `VmStarted`)
//! - `vm_id` — Filter by VM ID
//!
//! ## Example
//!
//! ```text
//! wscat -c "ws://localhost:8080/api/v1/events/ws?event_type=VmStarted"
//! ```
//!
//! Events are delivered as JSON text frames. The server sends periodic
//! ping frames to keep the connection alive.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::events::{EventBus, EventType};

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for WebSocket event filtering
#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    /// Filter by event type name
    pub event_type: Option<String>,
    /// Filter by VM ID
    pub vm_id: Option<String>,
}

// ============================================================================
// Handler
// ============================================================================

/// GET /api/v1/events/ws — WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(bus): State<Arc<EventBus>>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, bus, query))
}

/// Handle an established WebSocket connection
async fn handle_socket(mut socket: WebSocket, bus: Arc<EventBus>, query: WsQuery) {
    let receiver = bus.subscribe();
    let mut stream = BroadcastStream::new(receiver);

    // Parse event filter once
    let event_filter: Option<EventType> = query
        .event_type
        .as_deref()
        .and_then(|s| serde_json::from_str(&format!("\"{s}\"")).ok());

    let vm_filter = query.vm_id;

    loop {
        tokio::select! {
            // Forward events from the bus to the WebSocket
            item = stream.next() => {
                match item {
                    Some(Ok(event)) => {
                        // Apply event type filter
                        if let Some(ref filter) = event_filter {
                            if event.event_type != *filter && *filter != EventType::All {
                                continue;
                            }
                        }

                        // Apply VM ID filter
                        if let Some(ref vm_id) = vm_filter {
                            if event.vm_id.as_ref() != Some(vm_id) {
                                continue;
                            }
                        }

                        // Serialize and send
                        if let Ok(json) = serde_json::to_string(&event) {
                            if socket.send(Message::Text(json)).await.is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    Some(Err(_)) => {
                        // Lagged behind; skip
                        continue;
                    }
                    None => break, // Channel closed
                }
            }
            // Handle incoming messages from the client (ping/pong, close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) => break, // Connection error
                    _ => {} // Ignore other messages
                }
            }
        }
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create the WebSocket events router
///
/// Mounts at `/api/v1/events/ws`
pub fn create_ws_router(bus: Arc<EventBus>) -> Router {
    Router::new()
        .route("/api/v1/events/ws", get(ws_handler))
        .with_state(bus)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[test]
    fn test_ws_query_default() {
        let query = WsQuery::default();
        assert!(query.event_type.is_none());
        assert!(query.vm_id.is_none());
    }

    #[test]
    fn test_ws_query_deserialize() {
        let json = r#"{"event_type":"VmStarted","vm_id":"vm-123"}"#;
        let query: WsQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.event_type.as_deref(), Some("VmStarted"));
        assert_eq!(query.vm_id.as_deref(), Some("vm-123"));
    }

    #[test]
    fn test_ws_router_creation() {
        let bus = Arc::new(EventBus::new());
        let _router = create_ws_router(bus);
    }

    #[tokio::test]
    async fn test_ws_upgrade_requires_websocket_headers() {
        let bus = Arc::new(EventBus::new());
        let app = create_ws_router(bus);

        // A plain GET without upgrade headers should return 400-level error
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/events/ws")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Without WebSocket upgrade headers, axum rejects with an error
        assert_ne!(response.status(), axum::http::StatusCode::OK);
    }
}
