//! Authenticated, bounded MCP JSON HTTP transport behind an operator TLS proxy.
use super::{mcp, Api};
use anyhow::{bail, Result};
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Mutex, Semaphore};

const VERSION: &str = "2025-11-25";
const LIMIT: usize = 1024 * 1024;
const SESSION_LIMIT: usize = 64;
const CONCURRENT_LIMIT: usize = 16;
const SESSION_TTL: Duration = Duration::from_secs(600);

struct Entry {
    protocol: Arc<Mutex<mcp::Session>>,
    cancellation: Mutex<Option<(Value, oneshot::Sender<()>)>>,
    created: Instant,
}
struct Server {
    api: Api,
    deadline: u64,
    token: String,
    origins: Vec<String>,
    sessions: Mutex<HashMap<String, Arc<Entry>>>,
    body_slots: Arc<Semaphore>,
    operation_slots: Arc<Semaphore>,
}

fn field<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, StatusCode> {
    let values: Vec<_> = headers.get_all(name).iter().collect();
    if values.len() > 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    values
        .first()
        .map(|value| value.to_str().map_err(|_| StatusCode::BAD_REQUEST))
        .transpose()
}

fn authorize(server: &Server, headers: &HeaderMap) -> Result<(), StatusCode> {
    if let Some(origin) = field(headers, "origin")? {
        if !server.origins.iter().any(|allowed| allowed == origin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let expected = format!("Bearer {}", server.token);
    let supplied = field(headers, "authorization")?.unwrap_or("");
    // Do not short-circuit based on the first differing credential byte.
    let mut difference = supplied.len() ^ expected.len();
    for (a, b) in supplied.bytes().zip(expected.bytes()) {
        difference |= usize::from(a ^ b);
    }
    if difference != 0 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(version) = field(headers, "mcp-protocol-version")? {
        if version != VERSION {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    Ok(())
}

async fn endpoint(State(server): State<Arc<Server>>, request: Request) -> Response {
    if let Err(status) = authorize(&server, request.headers()) {
        let mut response = status.into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert("www-authenticate", "Bearer".parse().unwrap());
        }
        return response;
    }
    let session_id = match field(request.headers(), "mcp-session-id") {
        Ok(value) => value.map(str::to_owned),
        Err(status) => return status.into_response(),
    };
    if matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::DELETE
    ) {
        let Some(key) = &session_id else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let mut sessions = server.sessions.lock().await;
        sessions.retain(|_, entry| entry.created.elapsed() < SESSION_TTL);
        if !sessions.contains_key(key) {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    if request.method() == axum::http::Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if request.method() == axum::http::Method::DELETE {
        let Some(id) = session_id else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let entry = server.sessions.lock().await.remove(&id);
        let Some(entry) = entry else {
            return StatusCode::NOT_FOUND.into_response();
        };
        // Termination also releases an active client wait; upstream work may continue.
        if let Some((_, signal)) = entry.cancellation.lock().await.take() {
            let _ = signal.send(());
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    let accept = match field(request.headers(), "accept") {
        Ok(Some(value)) => value,
        _ => return StatusCode::NOT_ACCEPTABLE.into_response(),
    };
    let media: Vec<_> = accept
        .split(',')
        .map(|part| part.trim().split(';').next().unwrap_or(""))
        .collect();
    if !media.contains(&"application/json") || !media.contains(&"text/event-stream") {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if !matches!(field(request.headers(), "content-type"), Ok(Some(value)) if value.split(';').next().is_some_and(|part| part.trim() == "application/json"))
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(body_permit) = server.body_slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let body = match tokio::time::timeout(
        Duration::from_secs(server.deadline),
        to_bytes(request.into_body(), LIMIT),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let message: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    if !message.is_object() || message["jsonrpc"] != "2.0" || !message["method"].is_string() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = message.get("id");
    if id.is_some_and(|id| !id.is_string() && id.as_i64().is_none() && id.as_u64().is_none()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let has_id = id.is_some();
    let initializing = message["method"] == "initialize" && has_id;
    let (key, entry, fresh) = {
        let mut sessions = server.sessions.lock().await;
        sessions.retain(|_, entry| entry.created.elapsed() < SESSION_TTL);
        if let Some(key) = session_id {
            let Some(entry) = sessions.get(&key).cloned() else {
                return StatusCode::NOT_FOUND.into_response();
            };
            (key, entry, false)
        } else {
            if !initializing {
                return StatusCode::BAD_REQUEST.into_response();
            }
            if sessions.len() >= SESSION_LIMIT {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            let key = uuid::Uuid::new_v4().to_string();
            let entry = Arc::new(Entry {
                protocol: Arc::new(Mutex::new(mcp::Session::default())),
                cancellation: Mutex::new(None),
                created: Instant::now(),
            });
            sessions.insert(key.clone(), entry.clone());
            (key, entry, true)
        }
    };
    if let Some(cancelled) = mcp::cancellation_id(&body) {
        let mut active = entry.cancellation.lock().await;
        if active.as_ref().is_some_and(|(id, _)| *id == cancelled) {
            if let Some((_, signal)) = active.take() {
                let _ = signal.send(());
            }
        }
        return StatusCode::ACCEPTED.into_response();
    }
    let Ok(operation_permit) = server.operation_slots.clone().try_acquire_owned() else {
        if fresh {
            server.sessions.lock().await.remove(&key);
        }
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // Refuse overlap rather than accumulating unbounded requests per session.
    let Ok(mut protocol) = entry.protocol.clone().try_lock_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let (cancel, cancelled) = oneshot::channel();
    if let Some(id) = id {
        *entry.cancellation.lock().await = Some((id.clone(), cancel));
    }
    // Detached execution means transport disconnect alone does not cancel an
    // accepted tool call. An explicit cancellation signal does release its wait.
    let worker_server = server.clone();
    let worker_entry = entry.clone();
    drop(body_permit);
    let worker = tokio::spawn(async move {
        let _operation_permit = operation_permit;
        let operation = protocol.message(&worker_server.api, worker_server.deadline, &body);
        let response = if has_id && !initializing {
            tokio::select! { biased; value = operation => value, _ = cancelled => None }
        } else {
            operation.await
        };
        worker_entry.cancellation.lock().await.take();
        response
    });
    let response = match worker.await {
        Ok(Some(value)) => {
            let valid_initialization = value.get("result").is_some();
            if fresh && !valid_initialization {
                server.sessions.lock().await.remove(&key);
            }
            let mut response = Json(value).into_response();
            if fresh && valid_initialization {
                response
                    .headers_mut()
                    .insert("mcp-session-id", key.parse().unwrap());
            }
            response
        }
        Ok(None) => {
            if has_id {
                StatusCode::REQUEST_TIMEOUT.into_response()
            } else {
                StatusCode::ACCEPTED.into_response()
            }
        }
        Err(_) => {
            if fresh {
                server.sessions.lock().await.remove(&key);
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    };
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "owned-test-token-00000000000000000000";
    struct Fixture {
        url: String,
        client: reqwest::Client,
        state: Arc<Server>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Fixture {
        async fn new() -> Self {
            Self::with_api("http://127.0.0.1:1").await
        }
        async fn with_api(api: &str) -> Self {
            let state = Arc::new(Server {
                api: Api::new(api, 2, None).unwrap(),
                deadline: 2,
                token: TOKEN.into(),
                origins: vec!["https://agent.example.test".into()],
                sessions: Mutex::new(HashMap::new()),
                body_slots: Arc::new(Semaphore::new(CONCURRENT_LIMIT)),
                operation_slots: Arc::new(Semaphore::new(CONCURRENT_LIMIT)),
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let router = Router::new()
                .route("/mcp", post(endpoint).get(endpoint).delete(endpoint))
                .with_state(state.clone());
            let task = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            Self {
                url,
                client: reqwest::Client::new(),
                state,
                task,
            }
        }
        fn post(&self, message: Value) -> reqwest::RequestBuilder {
            self.client
                .post(&self.url)
                .bearer_auth(TOKEN)
                .header("accept", "application/json, text/event-stream")
                .json(&message)
        }
        async fn initialize(&self) -> String {
            let response = self.post(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).send().await.unwrap();
            assert_eq!(response.status(), 200);
            response.headers()["mcp-session-id"]
                .to_str()
                .unwrap()
                .to_string()
        }
    }
    #[tokio::test]
    async fn authentication_origin_and_version_precede_sessions() {
        let f = Fixture::new().await;
        let ping = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
        assert_eq!(
            f.client
                .post(&f.url)
                .json(&ping)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            f.post(ping.clone())
                .header("origin", "https://untrusted.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            f.post(ping.clone())
                .header("mcp-protocol-version", "unsupported")
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(f.post(ping).send().await.unwrap().status(), 400);
        assert!(f.state.sessions.lock().await.is_empty());
    }
    #[tokio::test]
    async fn initialized_notification_discovery_and_termination() {
        let f = Fixture::new().await;
        let session = f.initialize().await;
        let early = f
            .post(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .header("mcp-session-id", &session)
            .send()
            .await
            .unwrap();
        assert_eq!(
            early.json::<Value>().await.unwrap()["error"]["code"],
            -32002
        );
        let notification = f
            .post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .header("mcp-session-id", &session)
            .send()
            .await
            .unwrap();
        assert_eq!(notification.status(), 202);
        assert!(notification.bytes().await.unwrap().is_empty());
        let discovery = f
            .post(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))
            .header("origin", "https://agent.example.test")
            .header("mcp-session-id", &session)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(discovery["result"]["tools"].as_array().unwrap().len(), 12);
        assert_eq!(
            f.client
                .get(&f.url)
                .bearer_auth(TOKEN)
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            405
        );
        assert_eq!(
            f.client
                .delete(&f.url)
                .bearer_auth(TOKEN)
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":4,"method":"ping"}))
                .header("mcp-session-id", session)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    #[tokio::test]
    async fn invalid_initialization_and_expired_sessions_are_removed() {
        let f = Fixture::new().await;
        let response = f
            .post(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .send()
            .await
            .unwrap();
        assert!(response.headers().get("mcp-session-id").is_none());
        assert!(f.state.sessions.lock().await.is_empty());
        let session = f.initialize().await;
        f.state.sessions.lock().await.insert(
            session.clone(),
            Arc::new(Entry {
                protocol: Arc::new(Mutex::new(mcp::Session::default())),
                cancellation: Mutex::new(None),
                created: Instant::now() - SESSION_TTL,
            }),
        );
        assert_eq!(
            f.client
                .get(&f.url)
                .bearer_auth(TOKEN)
                .header("mcp-session-id", session)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    #[tokio::test]
    async fn duplicate_headers_invalid_ids_and_oversized_bodies_are_refused() {
        let f = Fixture::new().await;
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":null,"method":"ping"}))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
                .header("authorization", format!("Bearer {TOKEN}"))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            f.client
                .post(&f.url)
                .bearer_auth(TOKEN)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .body(vec![b' '; LIMIT + 1])
                .send()
                .await
                .unwrap()
                .status(),
            413
        );
    }
    #[tokio::test]
    async fn explicit_cancellation_bypasses_busy_session_and_releases_wait() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let entered_handler = entered.clone();
        let release_handler = release.clone();
        let upstream = Router::new().route(
            "/sandboxes",
            axum::routing::get(move || {
                let entered = entered_handler.clone();
                let release = release_handler.clone();
                async move {
                    entered.notify_one();
                    release.notified().await;
                    Json(json!([]))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        struct Stop(tokio::task::JoinHandle<()>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _upstream = Stop(tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        }));
        let f = Fixture::with_api(&address).await;
        let session = f.initialize().await;
        f.post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .header("mcp-session-id", &session)
            .send()
            .await
            .unwrap();
        let call = f.post(json!({"jsonrpc":"2.0","id":"active","method":"tools/call","params":{"name":"sandbox_list","arguments":{}}})).header("mcp-session-id", &session);
        let active = tokio::spawn(async move { call.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":5,"method":"ping"}))
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            429
        );
        let cancelled = f.post(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"active"}})).header("mcp-session-id", &session).send().await.unwrap();
        assert_eq!(cancelled.status(), 202);
        assert!(cancelled.bytes().await.unwrap().is_empty());
        let completed = tokio::time::timeout(Duration::from_secs(1), active)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.status(), 408);
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":6,"method":"ping"}))
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        release.notify_one();
    }

    #[tokio::test]
    async fn saturated_body_admission_refuses_and_recovers_without_session_allocation() {
        let f = Fixture::new().await;
        let held = f
            .state
            .body_slots
            .clone()
            .acquire_many_owned(CONCURRENT_LIMIT as u32)
            .await
            .unwrap();
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
        assert_eq!(f.post(initialize).send().await.unwrap().status(), 503);
        assert!(f.state.sessions.lock().await.is_empty());
        drop(held);
        assert!(!f.initialize().await.is_empty());
    }

    #[tokio::test]
    async fn saturated_workers_preserve_cancellation_and_recover() {
        let f = Fixture::new().await;
        let session = f.initialize().await;
        let held = f
            .state
            .operation_slots
            .clone()
            .acquire_many_owned(CONCURRENT_LIMIT as u32)
            .await
            .unwrap();
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":2,"method":"ping"}))
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        let entry = f.state.sessions.lock().await.get(&session).unwrap().clone();
        let (sender, receiver) = oneshot::channel();
        *entry.cancellation.lock().await = Some((json!("active"), sender));
        assert_eq!(f.post(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"active"}})).header("mcp-session-id", &session).send().await.unwrap().status(), 202);
        tokio::time::timeout(Duration::from_secs(1), receiver)
            .await
            .unwrap()
            .unwrap();
        drop(held);
        assert_eq!(
            f.post(json!({"jsonrpc":"2.0","id":3,"method":"ping"}))
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }

    #[tokio::test]
    async fn invalid_origin_is_forbidden_without_credentials_and_auth_challenges_bearer() {
        let f = Fixture::new().await;
        assert_eq!(
            f.client
                .post(&f.url)
                .header("origin", "https://untrusted.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let denied = f.client.post(&f.url).send().await.unwrap();
        assert_eq!(denied.status(), 401);
        assert_eq!(denied.headers()["www-authenticate"], "Bearer");
    }

    #[tokio::test]
    async fn sixteen_live_calls_saturate_admission_and_can_be_cancelled() {
        let entered = Arc::new(Semaphore::new(0));
        let released = Arc::new(Semaphore::new(0));
        let entered_handler = entered.clone();
        let released_handler = released.clone();
        let upstream = Router::new().route(
            "/sandboxes",
            axum::routing::get(move || {
                let entered = entered_handler.clone();
                let released = released_handler.clone();
                async move {
                    entered.add_permits(1);
                    let permit = released.acquire().await.unwrap();
                    permit.forget();
                    Json(serde_json::json!([]))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        struct Stop(tokio::task::JoinHandle<()>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _upstream = Stop(tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        }));
        let f = Fixture::with_api(&address).await;
        let mut sessions = Vec::new();
        for _ in 0..=CONCURRENT_LIMIT {
            let session = f.initialize().await;
            f.post(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .header("mcp-session-id", &session)
                .send()
                .await
                .unwrap();
            sessions.push(session);
        }
        let mut calls = Vec::new();
        for session in sessions.iter().take(CONCURRENT_LIMIT) {
            let request = f.post(serde_json::json!({"jsonrpc":"2.0","id":"active","method":"tools/call","params":{"name":"sandbox_list","arguments":{}}})).header("mcp-session-id", session);
            calls.push(tokio::spawn(async move { request.send().await.unwrap() }));
        }
        let observed = tokio::time::timeout(
            Duration::from_secs(1),
            entered.acquire_many(CONCURRENT_LIMIT as u32),
        )
        .await
        .unwrap()
        .unwrap();
        observed.forget();
        assert_eq!(
            f.post(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"ping"}))
                .header("mcp-session-id", &sessions[CONCURRENT_LIMIT])
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        assert_eq!(f.post(serde_json::json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"active"}})).header("mcp-session-id", &sessions[0]).send().await.unwrap().status(), 202);
        let cancelled = tokio::time::timeout(Duration::from_secs(1), calls.remove(0))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.status(), 408);
        assert_eq!(
            f.post(serde_json::json!({"jsonrpc":"2.0","id":3,"method":"ping"}))
                .header("mcp-session-id", &sessions[CONCURRENT_LIMIT])
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        released.add_permits(CONCURRENT_LIMIT);
        for call in calls {
            assert_eq!(call.await.unwrap().status(), 200);
        }
    }
}

pub(super) async fn serve(
    api: Api,
    deadline: u64,
    listen: SocketAddr,
    origins: Vec<String>,
) -> Result<()> {
    if !listen.ip().is_loopback() {
        bail!("MCP HTTP must bind to loopback behind an operator TLS proxy");
    }
    let token = std::env::var("HM_MCP_TOKEN").unwrap_or_default();
    if token.len() < 32 || token.len() > 4096 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        bail!("HM_MCP_TOKEN must contain 32-4096 visible ASCII bytes");
    }
    for origin in &origins {
        let parsed = reqwest::Url::parse(origin)?;
        if parsed.scheme() != "https" || parsed.origin().ascii_serialization() != *origin {
            bail!("allowed origins must be exact HTTPS origins");
        }
    }
    let state = Arc::new(Server {
        api,
        deadline,
        token,
        origins,
        sessions: Mutex::new(HashMap::new()),
        body_slots: Arc::new(Semaphore::new(CONCURRENT_LIMIT)),
        operation_slots: Arc::new(Semaphore::new(CONCURRENT_LIMIT)),
    });
    let router = Router::new()
        .route("/mcp", post(endpoint).get(endpoint).delete(endpoint))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
