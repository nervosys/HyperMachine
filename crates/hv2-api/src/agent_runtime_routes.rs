//! Agent Runtime REST API
//!
//! Operate the [`AgentRuntime`] over HTTP: spawn copy-on-write agents from a
//! warm baseline, list/release/reap them, and read fleet memory stats.
//!
//! | Method | Path                              | Description                 |
//! |--------|-----------------------------------|-----------------------------|
//! | POST   | `/api/v1/agents`                  | Spawn an agent              |
//! | GET    | `/api/v1/agents`                  | List live agents            |
//! | GET    | `/api/v1/agents/fleet`            | Fleet memory / count stats  |
//! | POST   | `/api/v1/agents/reap`             | Reap idle agents            |
//! | DELETE | `/api/v1/agents/{session_id}`     | Release an agent            |

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use hv2_agent::{AgentCapabilities, AgentRuntime};

/// Shared state for the agent-runtime routes.
pub struct AgentRuntimeAppState {
    /// The runtime that owns the agent fleet.
    pub runtime: Arc<AgentRuntime>,
}

impl AgentRuntimeAppState {
    /// Build state over a warm baseline image.
    pub fn new(baseline: &[u8]) -> Self {
        Self {
            runtime: Arc::new(AgentRuntime::new(baseline)),
        }
    }
}

// ── DTOs ──

#[derive(Debug, Deserialize)]
pub struct SpawnAgentRequest {
    /// Caller-chosen agent identifier.
    pub agent_id: String,
    /// Capability set: `full`, `operator` (default), `read_only`, or `none`.
    pub capabilities: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub session_id: String,
    pub agent_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub agents: Vec<String>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FleetResponse {
    pub live_agents: usize,
    pub baseline_bytes: usize,
    pub fleet_memory_bytes: usize,
}

#[derive(Debug, Deserialize)]
pub struct ReapRequest {
    pub max_idle_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReapResponse {
    pub reaped: usize,
}

fn parse_capabilities(spec: Option<&str>) -> Result<AgentCapabilities, String> {
    Ok(match spec {
        None | Some("operator") => AgentCapabilities::operator(),
        Some("full") => AgentCapabilities::full(),
        Some("read_only") | Some("readonly") => AgentCapabilities::read_only(),
        Some("none") => AgentCapabilities::none(),
        Some(other) => return Err(format!("unknown capabilities: {other}")),
    })
}

// ── Handlers ──

/// POST /api/v1/agents — spawn an agent.
async fn spawn_agent(
    State(state): State<Arc<AgentRuntimeAppState>>,
    Json(req): Json<SpawnAgentRequest>,
) -> impl IntoResponse {
    let caps = match parse_capabilities(req.capabilities.as_deref()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    };
    match state.runtime.spawn_agent(&req.agent_id, caps) {
        Ok(handle) => (
            StatusCode::CREATED,
            Json(AgentResponse {
                session_id: handle.session_id().to_string(),
                agent_id: req.agent_id,
            }),
        )
            .into_response(),
        // At capacity (after idle reclamation) — back-pressure to the caller.
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))).into_response(),
    }
}

/// GET /api/v1/agents — list live agents.
async fn list_agents(State(state): State<Arc<AgentRuntimeAppState>>) -> impl IntoResponse {
    let agents = state.runtime.agent_session_ids();
    let total = agents.len();
    Json(AgentListResponse { agents, total })
}

/// GET /api/v1/agents/fleet — fleet memory / count stats.
async fn fleet_stats(State(state): State<Arc<AgentRuntimeAppState>>) -> impl IntoResponse {
    Json(FleetResponse {
        live_agents: state.runtime.live_agents(),
        baseline_bytes: state.runtime.baseline_bytes(),
        fleet_memory_bytes: state.runtime.fleet_memory_bytes(),
    })
}

/// POST /api/v1/agents/reap — reap agents idle beyond `max_idle_secs`.
async fn reap_idle(
    State(state): State<Arc<AgentRuntimeAppState>>,
    Json(req): Json<ReapRequest>,
) -> impl IntoResponse {
    let reaped = state
        .runtime
        .reap_idle(Duration::from_secs(req.max_idle_secs));
    Json(ReapResponse { reaped })
}

/// DELETE /api/v1/agents/{session_id} — release an agent.
async fn release_agent(
    State(state): State<Arc<AgentRuntimeAppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if state.runtime.release_agent(&session_id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        )
            .into_response()
    }
}

/// Build the agent-runtime API router.
pub fn create_agent_runtime_router(state: Arc<AgentRuntimeAppState>) -> Router {
    Router::new()
        .route("/api/v1/agents", post(spawn_agent).get(list_agents))
        .route("/api/v1/agents/fleet", get(fleet_stats))
        .route("/api/v1/agents/reap", post(reap_idle))
        .route("/api/v1/agents/{session_id}", delete(release_agent))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> Router {
        // Small baseline keeps the test light.
        let state = Arc::new(AgentRuntimeAppState::new(&vec![0u8; 4096]));
        create_agent_runtime_router(state)
    }

    async fn post(app: &Router, uri: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    async fn get(app: &Router, uri: &str) -> (StatusCode, Vec<u8>) {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    #[tokio::test]
    async fn spawn_list_release_lifecycle() {
        let app = test_app();

        // Spawn two agents.
        let (s1, b1) = post(&app, "/api/v1/agents", json!({ "agent_id": "a1" })).await;
        assert_eq!(s1, StatusCode::CREATED);
        let a1: AgentResponse = serde_json::from_slice(&b1).unwrap();
        let (s2, _) = post(&app, "/api/v1/agents", json!({ "agent_id": "a2" })).await;
        assert_eq!(s2, StatusCode::CREATED);

        // List shows both.
        let (sl, bl) = get(&app, "/api/v1/agents").await;
        assert_eq!(sl, StatusCode::OK);
        let list: AgentListResponse = serde_json::from_slice(&bl).unwrap();
        assert_eq!(list.total, 2);

        // Fleet: two idle agents share one 4 KiB baseline.
        let (sf, bf) = get(&app, "/api/v1/agents/fleet").await;
        assert_eq!(sf, StatusCode::OK);
        let fleet: FleetResponse = serde_json::from_slice(&bf).unwrap();
        assert_eq!(fleet.live_agents, 2);
        assert_eq!(fleet.fleet_memory_bytes, 4096);

        // Release one.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/agents/{}", a1.session_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let (_, bl2) = get(&app, "/api/v1/agents").await;
        let list2: AgentListResponse = serde_json::from_slice(&bl2).unwrap();
        assert_eq!(list2.total, 1);
    }

    #[tokio::test]
    async fn reap_releases_idle_agents() {
        let app = test_app();
        post(&app, "/api/v1/agents", json!({ "agent_id": "a" })).await;
        post(&app, "/api/v1/agents", json!({ "agent_id": "b" })).await;

        let (s, b) = post(&app, "/api/v1/agents/reap", json!({ "max_idle_secs": 0 })).await;
        assert_eq!(s, StatusCode::OK);
        let reap: ReapResponse = serde_json::from_slice(&b).unwrap();
        assert_eq!(reap.reaped, 2);
    }

    #[tokio::test]
    async fn unknown_capabilities_is_bad_request() {
        let app = test_app();
        let (s, _) = post(
            &app,
            "/api/v1/agents",
            json!({ "agent_id": "a", "capabilities": "wizard" }),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
