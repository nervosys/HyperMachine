//! The REST mirror of `hm jobs`, under `/api/v1/jobs`.
//!
//! - `POST /api/v1/jobs` with a [`JobSpec`] queues it: `201 {"id": ...}`
//! - `GET  /api/v1/jobs[?state=running]` lists states, oldest first
//! - `GET  /api/v1/jobs/{id}` is `{"state": ..., "spec": ...}`
//! - `GET  /api/v1/jobs/{id}/logs?stream=stdout|stderr[&follow=true]` is the
//!   log as text; with `follow`, streamed until the job ends
//! - `POST /api/v1/jobs/{id}/cancel` asks it to stop: `202` with its state
//!
//! Submitting a job runs a program on the workers' hosts, so this is a
//! remote-execution endpoint. With a token, every request must carry
//! `Authorization: Bearer <token>`; `hm jobs serve` refuses to listen
//! anywhere but loopback without one.

use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::{JobError, JobSpec, Phase, Store};

#[derive(Clone)]
struct Api {
    store: Store,
    token: Option<String>,
}

/// The `/api/v1/jobs` routes over `store`, requiring `token` when given.
pub fn router(store: Store, token: Option<String>) -> Router {
    let api = Api { store, token };
    Router::new()
        .route("/api/v1/jobs", post(submit).get(list))
        .route("/api/v1/jobs/{id}", get(detail))
        .route("/api/v1/jobs/{id}/logs", get(logs))
        .route("/api/v1/jobs/{id}/cancel", post(cancel))
        .route(
            "/api/v1/schedules/{id}",
            post(schedule_create).get(schedule_detail),
        )
        .route("/api/v1/schedules/{id}/publish", post(schedule_publish))
        .route("/api/v1/schedules/{id}/cancel", post(schedule_cancel))
        .route(
            "/api/v1/schedules/{id}/occurrences",
            get(schedule_occurrences),
        )
        .route_layer(middleware::from_fn_with_state(api.clone(), authorize))
        .with_state(api)
}

async fn authorize(State(api): State<Api>, request: Request, next: Next) -> Response {
    if let Some(token) = &api.token {
        let presented = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let ok: bool = presented.len() == token.len()
            && bool::from(presented.as_bytes().ct_eq(token.as_bytes()));
        if !ok {
            return error(StatusCode::UNAUTHORIZED, "a bearer token is required");
        }
    }
    next.run(request).await
}

fn error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": message.to_string() }))).into_response()
}

fn from_job_error(e: JobError) -> Response {
    let status = match &e {
        JobError::NotFound(_) => StatusCode::NOT_FOUND,
        JobError::InvalidSpec(_) => StatusCode::BAD_REQUEST,
        JobError::Conflict(_) => StatusCode::CONFLICT,
        JobError::Io(_) | JobError::Corrupt(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, e)
}

// Schedule publication performs sync filesystem operations. Keep them off the
// async request executor; authentication is shared with every existing route.
async fn schedule_operation(
    status: StatusCode,
    operation: impl FnOnce() -> crate::Result<serde_json::Value> + Send + 'static,
) -> Response {
    match tokio::task::spawn_blocking(operation).await {
        Ok(Ok(value)) => (status, Json(value)).into_response(),
        Ok(Err(error)) => from_job_error(error),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "schedule operation failed",
        ),
    }
}

async fn schedule_create(
    State(api): State<Api>,
    Path(id): Path<String>,
    Json(spec): Json<crate::schedule::IntervalSchedule>,
) -> Response {
    schedule_operation(StatusCode::CREATED, move || {
        api.store.create_interval_schedule(&id, &spec)?;
        Ok(json!({"id": id}))
    })
    .await
}

async fn schedule_detail(State(api): State<Api>, Path(id): Path<String>) -> Response {
    schedule_operation(StatusCode::OK, move || {
        Ok(json!({
            "id": id, "schedule": api.store.interval_schedule(&id)?,
            "cancelled": api.store.interval_schedule_cancelled(&id)?,
            "publication_through_ms": api.store.interval_progress(&id)?
        }))
    })
    .await
}

async fn schedule_cancel(State(api): State<Api>, Path(id): Path<String>) -> Response {
    schedule_operation(StatusCode::OK, move || {
        api.store.cancel_interval_schedule(&id)?;
        Ok(json!({"id": id, "cancelled": true}))
    })
    .await
}

fn batch_limit() -> usize {
    100
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequest {
    now_ms: Option<u64>,
    #[serde(default = "batch_limit")]
    limit: usize,
}

async fn schedule_publish(
    State(api): State<Api>,
    Path(id): Path<String>,
    Json(request): Json<PublishRequest>,
) -> Response {
    schedule_operation(StatusCode::OK, move || {
        let records = api.store.materialize_interval(
            &id,
            request.now_ms.unwrap_or_else(crate::now_ms),
            request.limit,
        )?;
        Ok(json!(records))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OccurrenceQuery {
    after_ms: Option<u64>,
    #[serde(default = "batch_limit")]
    limit: usize,
}

async fn schedule_occurrences(
    State(api): State<Api>,
    Path(id): Path<String>,
    Query(query): Query<OccurrenceQuery>,
) -> Response {
    schedule_operation(StatusCode::OK, move || {
        Ok(json!(api.store.committed_interval_occurrences(
            &id,
            query.after_ms,
            query.limit
        )?))
    })
    .await
}

async fn submit(State(api): State<Api>, Json(spec): Json<JobSpec>) -> Response {
    match api.store.submit(&spec) {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Err(e) => from_job_error(e),
    }
}

#[derive(Deserialize)]
struct ListQuery {
    state: Option<String>,
}

async fn list(State(api): State<Api>, Query(q): Query<ListQuery>) -> Response {
    match api.store.list() {
        Ok(all) => Json(
            all.into_iter()
                .filter(|s| {
                    q.state
                        .as_deref()
                        .is_none_or(|want| s.state.as_str() == want)
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => from_job_error(e),
    }
}

async fn detail(State(api): State<Api>, Path(id): Path<String>) -> Response {
    match (api.store.state(&id), api.store.spec(&id)) {
        (Ok(state), Ok(spec)) => Json(json!({ "state": state, "spec": spec })).into_response(),
        (Err(e), _) | (_, Err(e)) => from_job_error(e),
    }
}

async fn cancel(State(api): State<Api>, Path(id): Path<String>) -> Response {
    match api.store.cancel(&id) {
        Ok(state) => (StatusCode::ACCEPTED, Json(state)).into_response(),
        Err(e) => from_job_error(e),
    }
}

#[derive(Deserialize)]
struct LogQuery {
    stream: Option<String>,
    #[serde(default)]
    follow: bool,
}

async fn logs(
    State(api): State<Api>,
    Path(id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Response {
    let stream = q.stream.unwrap_or_else(|| "stdout".into());
    if stream != "stdout" && stream != "stderr" {
        return error(StatusCode::BAD_REQUEST, "stream is stdout or stderr");
    }
    if let Err(e) = api.store.state(&id) {
        return from_job_error(e);
    }
    let path = api.store.log_path(&id, &stream);
    if !q.follow {
        return match tokio::fs::read(&path).await {
            Ok(bytes) => {
                ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], bytes).into_response()
            }
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
    }
    // Follow: send what is there, then what is appended, until the job ends
    // and the log has been read to its end once more after that.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(16);
    let store = api.store.clone();
    tokio::spawn(async move {
        let mut sent: u64 = 0;
        loop {
            let ended = store
                .state(&id)
                .map_or(true, |s| s.state.is_final() || s.state == Phase::Cancelled);
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let len = bytes.len() as u64;
                    if len > sent {
                        let chunk = bytes[usize::try_from(sent).unwrap_or(usize::MAX)..].to_vec();
                        sent = len;
                        if tx.send(Ok(chunk)).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            }
            if ended {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn app(dir: &std::path::Path, token: Option<&str>) -> (Store, Router) {
        let store = Store::open(dir).unwrap();
        let r = router(store.clone(), token.map(str::to_string));
        (store, r)
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
        token: Option<&str>,
    ) -> (StatusCode, String) {
        let mut b = HttpRequest::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        let req = match body {
            Some(json) => b
                .header("content-type", "application/json")
                .body(Body::from(json.to_string())),
            None => b.body(Body::empty()),
        }
        .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn submit_list_detail_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let (store, app) = app(dir.path(), None);
        let (s, body) = call(
            &app,
            "POST",
            "/api/v1/jobs",
            Some(r#"{"command":["x"],"labels":["gpu"],"not_before_ms":18446744073709551615}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "{body}");
        let id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let (s, body) = call(&app, "GET", "/api/v1/jobs?state=queued", None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(body.contains(&id));
        let (_, body) = call(&app, "GET", "/api/v1/jobs?state=running", None, None).await;
        assert_eq!(body, "[]");

        let (s, body) = call(&app, "GET", &format!("/api/v1/jobs/{id}"), None, None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(body.contains("\"gpu\""), "{body}");
        let detail: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(detail["spec"]["not_before_ms"].as_u64(), Some(u64::MAX));
        assert!(store.claim("worker", &["gpu".into()]).unwrap().is_none());

        let (s, body) = call(
            &app,
            "POST",
            &format!("/api/v1/jobs/{id}/cancel"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::ACCEPTED, "{body}");
        assert!(body.contains("cancelled"), "{body}");
        let (s, _) = call(
            &app,
            "POST",
            &format!("/api/v1/jobs/{id}/cancel"),
            None,
            None,
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT);

        let (s, body) = call(&app, "GET", &format!("/api/v1/jobs/{id}/logs"), None, None).await;
        assert_eq!((s, body.as_str()), (StatusCode::OK, ""));
    }

    #[tokio::test]
    async fn schedule_routes_authenticate_publish_and_recover_pages() {
        let dir = tempfile::tempdir().unwrap();
        let (store, app) = app(dir.path(), Some("schedule-token"));
        for (method, path) in [
            ("POST", "/api/v1/schedules/test"),
            ("GET", "/api/v1/schedules/test"),
            ("POST", "/api/v1/schedules/test/publish"),
            ("POST", "/api/v1/schedules/test/cancel"),
            ("GET", "/api/v1/schedules/test/occurrences"),
        ] {
            for token in [None, Some("wrong")] {
                assert_eq!(
                    call(&app, method, path, Some("{}"), token).await.0,
                    StatusCode::UNAUTHORIZED
                );
            }
        }
        let token = Some("schedule-token");
        let spec = r#"{"first_ms":100,"every_ms":10,"job":{"command":["must-not-execute"]}}"#;
        assert_eq!(
            call(&app, "POST", "/api/v1/schedules/test", Some(spec), token)
                .await
                .0,
            StatusCode::CREATED
        );
        assert_eq!(
            call(&app, "POST", "/api/v1/schedules/test", Some(spec), token)
                .await
                .0,
            StatusCode::CONFLICT
        );
        let (status, body) = call(
            &app,
            "POST",
            "/api/v1/schedules/test/publish",
            Some(r#"{"now_ms":135,"limit":2}"#),
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let records: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(records.as_array().unwrap().len(), 2);
        assert_eq!(records[1]["scheduled_ms"], 110);
        let (status, body) = call(&app, "GET", "/api/v1/schedules/test", None, token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["publication_through_ms"],
            110
        );
        let (status, body) = call(
            &app,
            "GET",
            "/api/v1/schedules/test/occurrences?after_ms=100&limit=1",
            None,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()[0]["scheduled_ms"],
            110
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/schedules/test/publish",
                Some(r#"{"limit":0}"#),
                token
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert!(call(
            &app,
            "POST",
            "/api/v1/schedules/test/publish",
            Some(r#"{"typo":1}"#),
            token
        )
        .await
        .0
        .is_client_error());
        assert_eq!(
            call(&app, "GET", "/api/v1/schedules/missing", None, token)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert!(store.list().unwrap().is_empty());
        assert!(store.claim("worker", &[]).unwrap().is_none());
        assert_eq!(
            call(&app, "POST", "/api/v1/schedules/test/cancel", None, token)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/schedules/test/publish",
                Some("{}"),
                token
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn bad_specs_and_unknown_jobs_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (_s, app) = app(dir.path(), None);
        let (s, _) = call(
            &app,
            "POST",
            "/api/v1/jobs",
            Some(r#"{"command":[]}"#),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(
            &app,
            "POST",
            "/api/v1/jobs",
            Some(r#"{"command":["x"],"nope":1}"#),
            None,
        )
        .await;
        assert!(s.is_client_error());
        let (s, _) = call(&app, "GET", "/api/v1/jobs/j000-nothere", None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        let (s, _) = call(&app, "GET", "/api/v1/jobs/..%2F..%2Fetc/logs", None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn with_a_token_every_route_requires_it() {
        let dir = tempfile::tempdir().unwrap();
        let (_s, app) = app(dir.path(), Some("s3cret"));
        for (m, uri, body) in [
            ("GET", "/api/v1/jobs", None),
            ("POST", "/api/v1/jobs", Some(r#"{"command":["x"]}"#)),
        ] {
            let (s, _) = call(&app, m, uri, body, None).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri}");
            let (s, _) = call(&app, m, uri, body, Some("wrong!")).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri}");
        }
        let (s, _) = call(&app, "GET", "/api/v1/jobs", None, Some("s3cret")).await;
        assert_eq!(s, StatusCode::OK);
    }
}
