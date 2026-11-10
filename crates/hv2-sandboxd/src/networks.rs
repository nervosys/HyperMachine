//! `/networks`: create, list, inspect and remove a node-local network, and
//! connect or disconnect a sandbox to one -- the HTTP surface over
//! [`crate::local_network::Registry`]. See that module for what a network
//! actually does; this one only validates requests and shapes JSON.

use super::{api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Response, State};
use crate::local_network::{Error, NetworkInfo};
use axum::http::StatusCode;
use serde_json::json;

fn status_for(error: Error) -> StatusCode {
    match error {
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::AlreadyExists
        | Error::AliasTaken
        | Error::AlreadyMember
        | Error::NotAMember
        | Error::NotEmpty => StatusCode::CONFLICT,
        Error::Exhausted => StatusCode::INSUFFICIENT_STORAGE,
    }
}

fn describe(info: &NetworkInfo) -> serde_json::Value {
    json!({
        "name": info.name,
        "members": info.members.iter().map(|m| json!({
            "sandboxID": m.sandbox_id,
            "alias": m.alias,
            "address": m.address.to_string(),
        })).collect::<Vec<_>>(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateNetwork {
    name: String,
}

/// `POST /networks`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Json(asked): Json<CreateNetwork>,
) -> Response {
    if asked.name.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    match state.local_networks.create(&asked.name) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({"name": asked.name, "members": []})),
        )
            .into_response(),
        Err(e) => api_error(status_for(e), e),
    }
}

/// `GET /networks`.
pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Response {
    Json(
        state
            .local_networks
            .list()
            .iter()
            .map(describe)
            .collect::<Vec<_>>(),
    )
    .into_response()
}

/// `GET /networks/{name}`.
pub(crate) async fn inspect(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match state.local_networks.inspect(&name) {
        Some(info) => Json(describe(&info)).into_response(),
        None => api_error(StatusCode::NOT_FOUND, "no such network"),
    }
}

/// `DELETE /networks/{name}`.
pub(crate) async fn remove(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match state.local_networks.remove(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(status_for(e), e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connect {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    alias: String,
}

/// `POST /networks/{name}/connect`. The sandbox need not be stopped or
/// started for this: membership changes are visible to its gateway (if it
/// has one) on the very next lookup, with nothing to restart.
pub(crate) async fn connect(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(asked): Json<Connect>,
) -> Response {
    if asked.alias.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "alias must not be empty");
    }
    match state
        .local_networks
        .connect(&name, &asked.sandbox_id, &asked.alias)
    {
        Ok(address) => Json(json!({
            "network": name,
            "sandboxID": asked.sandbox_id,
            "alias": asked.alias,
            "address": address.to_string(),
        }))
        .into_response(),
        Err(e) => api_error(status_for(e), e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disconnect {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
}

/// `POST /networks/{name}/disconnect`.
pub(crate) async fn disconnect(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(asked): Json<Disconnect>,
) -> Response {
    match state.local_networks.disconnect(&name, &asked.sandbox_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(status_for(e), e),
    }
}
