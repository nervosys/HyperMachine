//! E2B's sandbox events and webhooks, over the cluster store's event
//! stream: `GET /events/sandboxes[/{id}]`, and `/events/webhooks` with its
//! deliveries and stats. Served by a control plane and by a node alike --
//! a node on its own keeps them in a [`crate::store::MemoryStore`].
//!
//! An event is delivered by the node that emitted it, so each is sent once
//! however many control planes there are. A delivery is signed as E2B signs
//! one -- `e2b-signature` is base64 (unpadded) of SHA-256 over the secret
//! followed by the body -- and retried with backoff. Webhook URLs are
//! user-supplied, so by default an address that is not global (loopback,
//! private, link-local, the cloud metadata service) is refused, and the
//! address checked is the one connected to.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{now_ms, rfc3339, ClusterEvent, Delivery, Webhook};
use crate::store::ClusterStore;

/// The team every event belongs to: this cluster has one.
pub const TEAM_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Events read back, at most, for a query.
const SCAN: usize = 10_000;
/// Attempts per delivery, and the wait before each retry.
const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(4)];
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// A response body kept in a delivery record, at most.
const RESPONSE_KEPT: usize = 2048;

/// E2B's name for what a cluster event records, if it is one E2B has.
#[must_use]
pub fn e2b_type(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "sandbox-created" | "sandbox-forked" => "sandbox.lifecycle.created",
        "sandbox-paused" => "sandbox.lifecycle.paused",
        "sandbox-resumed" => "sandbox.lifecycle.resumed",
        "sandbox-updated" => "sandbox.lifecycle.updated",
        "sandbox-deleted" | "sandbox-expired" | "sandbox-lost" => "sandbox.lifecycle.killed",
        _ => return None,
    })
}

/// E2B's short label for an event type: `kill` for `sandbox.lifecycle.killed`.
/// Only `kill` is in E2B's published example; the others follow it.
fn label(kind: &str) -> &'static str {
    match kind {
        "sandbox.lifecycle.created" => "create",
        "sandbox.lifecycle.paused" => "pause",
        "sandbox.lifecycle.resumed" => "resume",
        "sandbox.lifecycle.updated" => "update",
        "sandbox.lifecycle.killed" => "kill",
        _ => "",
    }
}

/// `event_data`: the sandbox's metadata and execution as the event recorded
/// them, and why a sandbox was killed -- `request` as E2B's example has it,
/// `timeout` for one that ran out its time, `lost` for one whose node died.
fn event_data(event: &ClusterEvent) -> Value {
    let mut data = event.data.clone().unwrap_or_else(|| json!({}));
    let reason = match event.kind.as_str() {
        "sandbox-deleted" => Some("request"),
        "sandbox-expired" => Some("timeout"),
        "sandbox-lost" => Some("lost"),
        _ => None,
    };
    if let Some(map) = data.as_object_mut() {
        if let Some(reason) = reason {
            map.insert("kill_reason".into(), reason.into());
        }
        // Not E2B's: which node ran it, and anything it noted. A receiver
        // written for E2B ignores keys it does not know.
        map.insert("node_id".into(), event.node_id.clone().into());
        if let Some(detail) = &event.detail {
            map.insert("detail".into(), detail.clone().into());
        }
    }
    data
}

/// A cluster event as E2B's `SandboxEvent`, which `GET /events/sandboxes`
/// answers with: camelCase, as E2B's API spec has it.
#[must_use]
pub fn to_e2b(event: &ClusterEvent) -> Option<Value> {
    let kind = e2b_type(&event.kind)?;
    let sandbox = event.sandbox_id.clone()?;
    let template = event.template_id.clone().unwrap_or_default();
    Some(json!({
        "id": event.id,
        "version": "v2",
        "type": kind,
        "eventCategory": "lifecycle",
        "eventLabel": label(kind),
        "eventData": event_data(event),
        "timestamp": rfc3339(event.at_ms),
        "sandboxId": sandbox,
        "sandboxExecutionId": sandbox,
        "sandboxTemplateId": template,
        "sandboxBuildId": template,
        "sandboxTeamId": TEAM_ID,
    }))
}

/// Days an event is kept, as a webhook payload reports it.
const EVENTS_TTL_DAYS: u32 = 30;

/// A cluster event as E2B delivers it to a webhook: the same event in
/// snake_case, as E2B's webhook documentation shows it -- not the API's
/// camelCase, which a receiver written against E2B would not read.
#[must_use]
pub fn to_webhook(event: &ClusterEvent) -> Option<Value> {
    let kind = e2b_type(&event.kind)?;
    let sandbox = event.sandbox_id.clone()?;
    let template = event.template_id.clone().unwrap_or_default();
    Some(json!({
        "id": event.id,
        "version": "v2",
        "type": kind,
        "timestamp": rfc3339(event.at_ms),
        "event_category": "lifecycle",
        "event_label": label(kind),
        "event_data": event_data(event),
        "sandbox_id": sandbox,
        "sandbox_execution_id": sandbox,
        "sandbox_template_id": template,
        "sandbox_build_id": template,
        "sandbox_team_id": TEAM_ID,
        "events_ttl_days": EVENTS_TTL_DAYS,
    }))
}

fn api_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "code": status.as_u16(), "message": message.to_string() })),
    )
        .into_response()
}

/// The routes, over `store`.
pub fn router<S: Clone + Send + Sync + 'static>(store: Arc<dyn ClusterStore>) -> Router<S> {
    Router::<Arc<dyn ClusterStore>>::new()
        .route("/events/sandboxes", get(all_events))
        .route("/events/sandboxes/{id}", get(sandbox_events))
        .route("/events/webhooks", get(list_webhooks).post(create_webhook))
        .route(
            "/events/webhooks/{id}",
            get(get_webhook)
                .patch(update_webhook)
                .delete(delete_webhook),
        )
        .route("/events/webhooks/{id}/deliveries", get(deliveries))
        .route("/events/webhooks/{id}/stats", get(stats))
        .with_state(store)
}

type Store = State<Arc<dyn ClusterStore>>;

#[derive(Debug, Default, Deserialize)]
struct EventQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    #[serde(rename = "orderAsc")]
    order_asc: Option<bool>,
    /// Comma-separated E2B types.
    types: Option<String>,
}

async fn query_events(store: &dyn ClusterStore, sandbox: Option<&str>, q: &EventQuery) -> Response {
    let events = match store.events(SCAN).await {
        Ok(events) => events,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let types: Option<Vec<&str>> = q.types.as_deref().map(|t| t.split(',').collect());
    let mut out: Vec<Value> = events
        .iter()
        .filter(|e| sandbox.is_none_or(|s| e.sandbox_id.as_deref() == Some(s)))
        .filter_map(to_e2b)
        .filter(|v| {
            types
                .as_ref()
                .is_none_or(|t| t.contains(&v["type"].as_str().unwrap_or_default()))
        })
        .collect();
    // The store answers newest first.
    if q.order_asc == Some(true) {
        out.reverse();
    }
    let out: Vec<Value> = out
        .into_iter()
        .skip(q.offset.unwrap_or(0))
        .take(q.limit.unwrap_or(100).min(1000))
        .collect();
    Json(out).into_response()
}

async fn all_events(State(store): Store, Query(q): Query<EventQuery>) -> Response {
    query_events(store.as_ref(), None, &q).await
}

async fn sandbox_events(
    State(store): Store,
    Path(id): Path<String>,
    Query(q): Query<EventQuery>,
) -> Response {
    query_events(store.as_ref(), Some(&id), &q).await
}

fn detail(hook: &Webhook) -> Value {
    json!({
        "id": hook.id,
        "teamId": TEAM_ID,
        "name": hook.name,
        "createdAt": rfc3339(hook.created_ms),
        "url": hook.url,
        "enabled": hook.enabled,
        "events": hook.events,
    })
}

/// A URL a webhook may have: HTTP or HTTPS, with a host.
fn valid_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

fn valid_events(events: &[String]) -> Result<(), String> {
    for event in events {
        if !event.starts_with("sandbox.lifecycle.") {
            return Err(format!("event type {event:?}: sandbox.lifecycle.* only"));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WebhookCreate {
    name: String,
    url: String,
    #[serde(default)]
    events: Vec<String>,
    enabled: Option<bool>,
    #[serde(rename = "signatureSecret")]
    signature_secret: String,
}

async fn create_webhook(State(store): Store, Json(req): Json<WebhookCreate>) -> Response {
    if !valid_url(&req.url) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("url {:?}: http or https", req.url),
        );
    }
    if req.signature_secret.len() < 16 {
        return api_error(
            StatusCode::BAD_REQUEST,
            "signatureSecret: 16 characters at least",
        );
    }
    if let Err(e) = valid_events(&req.events) {
        return api_error(StatusCode::BAD_REQUEST, e);
    }
    let hook = Webhook {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        url: req.url,
        events: req.events,
        enabled: req.enabled.unwrap_or(true),
        secret: req.signature_secret,
        created_ms: now_ms(),
    };
    match store.put_webhook(&hook).await {
        Ok(()) => (StatusCode::CREATED, Json(detail(&hook))).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn list_webhooks(State(store): Store) -> Response {
    match store.webhooks().await {
        Ok(hooks) => Json(hooks.iter().map(detail).collect::<Vec<_>>()).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
async fn find(store: &dyn ClusterStore, id: &str) -> Result<Webhook, Response> {
    match store.webhooks().await {
        Ok(hooks) => hooks
            .into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("no webhook {id}"))),
        Err(e) => Err(api_error(StatusCode::SERVICE_UNAVAILABLE, e)),
    }
}

async fn get_webhook(State(store): Store, Path(id): Path<String>) -> Response {
    match find(store.as_ref(), &id).await {
        Ok(hook) => Json(detail(&hook)).into_response(),
        Err(r) => r,
    }
}

#[derive(Debug, Default, Deserialize)]
struct WebhookConfiguration {
    enabled: Option<bool>,
    name: Option<String>,
    url: Option<String>,
    events: Option<Vec<String>>,
    #[serde(rename = "signatureSecret")]
    signature_secret: Option<String>,
}

async fn update_webhook(
    State(store): Store,
    Path(id): Path<String>,
    Json(req): Json<WebhookConfiguration>,
) -> Response {
    let mut hook = match find(store.as_ref(), &id).await {
        Ok(hook) => hook,
        Err(r) => return r,
    };
    if let Some(url) = req.url {
        if !valid_url(&url) {
            return api_error(
                StatusCode::BAD_REQUEST,
                format!("url {url:?}: http or https"),
            );
        }
        hook.url = url;
    }
    if let Some(events) = req.events {
        if let Err(e) = valid_events(&events) {
            return api_error(StatusCode::BAD_REQUEST, e);
        }
        hook.events = events;
    }
    if let Some(secret) = req.signature_secret {
        if secret.len() < 16 {
            return api_error(
                StatusCode::BAD_REQUEST,
                "signatureSecret: 16 characters at least",
            );
        }
        hook.secret = secret;
    }
    if let Some(name) = req.name {
        hook.name = name;
    }
    if let Some(enabled) = req.enabled {
        hook.enabled = enabled;
    }
    match store.put_webhook(&hook).await {
        Ok(()) => Json(detail(&hook)).into_response(),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn delete_webhook(State(store): Store, Path(id): Path<String>) -> Response {
    match store.delete_webhook(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => api_error(StatusCode::NOT_FOUND, format!("no webhook {id}")),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

fn delivery_json(d: &Delivery) -> Value {
    json!({
        "id": d.id,
        "teamId": TEAM_ID,
        "webhookId": d.webhook_id,
        "eventId": d.event_id,
        "sandboxId": d.sandbox_id,
        "eventType": d.event_type,
        "status": d.status,
        "durationMs": d.duration_ms,
        "requestBody": d.request_body,
        "requestHeaders": r#"{"content-type":"application/json","e2b-signature":"[redacted]"}"#,
        "requestUrl": d.request_url,
        "responseBody": d.response_body,
        "responseHeaders": Value::Null,
        "responseHttpStatusCode": d.response_status,
        "errorClass": d.error_class,
        "errorMessage": d.error_message,
        "timestamp": rfc3339(d.at_ms),
    })
}

#[derive(Debug, Default, Deserialize)]
struct Limit {
    limit: Option<usize>,
}

/// `GET /events/webhooks/{id}/deliveries`: attempts, grouped by event.
async fn deliveries(
    State(store): Store,
    Path(id): Path<String>,
    Query(q): Query<Limit>,
) -> Response {
    if let Err(r) = find(store.as_ref(), &id).await {
        return r;
    }
    let all = match store
        .deliveries(&id, q.limit.unwrap_or(100).min(1000))
        .await
    {
        Ok(all) => all,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let mut groups: Vec<(String, Value)> = Vec::new();
    for d in &all {
        match groups.iter_mut().find(|(event, _)| *event == d.event_id) {
            Some((_, group)) => {
                if let Some(attempts) = group["attempts"].as_array_mut() {
                    attempts.push(delivery_json(d));
                }
            }
            None => groups.push((
                d.event_id.clone(),
                json!({
                    "eventId": d.event_id,
                    "eventType": d.event_type,
                    "sandboxId": d.sandbox_id,
                    "attempts": [delivery_json(d)],
                }),
            )),
        }
    }
    Json(groups.into_iter().map(|(_, g)| g).collect::<Vec<_>>()).into_response()
}

/// `GET /events/webhooks/{id}/stats`: totals, and per hour.
async fn stats(State(store): Store, Path(id): Path<String>) -> Response {
    if let Err(r) = find(store.as_ref(), &id).await {
        return r;
    }
    let all = match store.deliveries(&id, crate::store::DELIVERY_TAIL).await {
        Ok(all) => all,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let durations = |ds: &[&Delivery]| {
        let values: Vec<f64> = ds.iter().map(|d| d.duration_ms as f64).collect();
        let n = values.len().max(1) as f64;
        json!({
            "minimum": if values.is_empty() { 0.0 } else { values.iter().copied().fold(f64::INFINITY, f64::min) },
            "average": values.iter().sum::<f64>() / n,
            "maximum": values.iter().copied().fold(0.0, f64::max),
        })
    };
    let mut hours: std::collections::BTreeMap<u64, Vec<&Delivery>> =
        std::collections::BTreeMap::new();
    for d in &all {
        hours.entry(d.at_ms / 3_600_000).or_default().push(d);
    }
    let buckets: Vec<Value> = hours
        .iter()
        .map(|(hour, ds)| {
            json!({
                "timestamp": rfc3339(hour * 3_600_000),
                "total": ds.len(),
                "failed": ds.iter().filter(|d| d.status != "success").count(),
                "durationMs": durations(ds),
            })
        })
        .collect();
    let every: Vec<&Delivery> = all.iter().collect();
    Json(json!({
        "buckets": buckets,
        "total": all.len(),
        "failed": all.iter().filter(|d| d.status != "success").count(),
        "durationMs": durations(&every),
    }))
    .into_response()
}

// ── Delivery ────────────────────────────────────────────────────────────────

/// Sends the events a node emits to the webhooks that want them.
#[derive(Clone)]
pub struct Dispatcher {
    store: Arc<dyn ClusterStore>,
    http: reqwest::Client,
    /// Deliver to loopback, private and link-local addresses too: for a
    /// cluster whose receivers are inside it, which an operator decides.
    allow_private: bool,
}

impl Dispatcher {
    #[must_use]
    pub fn new(store: Arc<dyn ClusterStore>, allow_private: bool) -> Self {
        Self {
            store,
            http: reqwest::Client::new(),
            allow_private,
        }
    }

    #[must_use]
    pub fn store(&self) -> &Arc<dyn ClusterStore> {
        &self.store
    }

    /// Send `event` to every enabled webhook that subscribes to it, each on
    /// a task of its own: a slow receiver delays nothing here.
    pub fn deliver(&self, event: &ClusterEvent) {
        let Some(payload) = to_webhook(event) else {
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            let Ok(hooks) = this.store.webhooks().await else {
                return;
            };
            let kind = payload["type"].as_str().unwrap_or_default().to_string();
            for hook in hooks {
                if !hook.enabled || !(hook.events.is_empty() || hook.events.contains(&kind)) {
                    continue;
                }
                let this = this.clone();
                let payload = payload.clone();
                tokio::spawn(async move { this.deliver_to(&hook, &payload).await });
            }
        });
    }

    async fn deliver_to(&self, hook: &Webhook, payload: &Value) {
        let body = payload.to_string();
        for attempt in 0..=BACKOFF.len() {
            let delivery = self.attempt(hook, payload, &body).await;
            let done = delivery.status == "success"
                || matches!(delivery.error_class.as_deref(), Some("request_error"));
            if let Err(e) = self.store.record_delivery(&delivery).await {
                tracing::warn!("recording a webhook delivery: {e}");
            }
            if done {
                return;
            }
            if let Some(wait) = BACKOFF.get(attempt) {
                tokio::time::sleep(*wait).await;
            }
        }
    }

    async fn attempt(&self, hook: &Webhook, payload: &Value, body: &str) -> Delivery {
        let started = Instant::now();
        let mut delivery = Delivery {
            id: uuid::Uuid::new_v4().to_string(),
            webhook_id: hook.id.clone(),
            event_id: payload["id"].as_str().unwrap_or_default().to_string(),
            sandbox_id: payload["sandbox_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            event_type: payload["type"].as_str().unwrap_or_default().to_string(),
            status: "failed".into(),
            duration_ms: 0,
            request_body: body.to_string(),
            request_url: hook.url.clone(),
            response_status: None,
            response_body: None,
            error_class: None,
            error_message: None,
            at_ms: now_ms(),
        };
        let result = self.send(hook, &delivery.id, body).await;
        delivery.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok((status, text)) => {
                delivery.response_status = Some(status);
                delivery.response_body = Some(text.chars().take(RESPONSE_KEPT).collect());
                if (200..300).contains(&status) {
                    delivery.status = "success".into();
                } else {
                    delivery.error_class = Some("http_error".into());
                }
            }
            Err((class, message)) => {
                delivery.error_class = Some(class.into());
                delivery.error_message = Some(message);
            }
        }
        delivery
    }

    async fn send(
        &self,
        hook: &Webhook,
        delivery_id: &str,
        body: &str,
    ) -> Result<(u16, String), (&'static str, String)> {
        let url = reqwest::Url::parse(&hook.url).map_err(|e| ("request_error", e.to_string()))?;
        let host = url
            .host_str()
            .ok_or(("request_error", "no host".to_string()))?
            .to_string();
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| ("dns_error", e.to_string()))?
            .collect();
        let addr = addrs
            .iter()
            .copied()
            .find(|a| self.allow_private || is_global(a.ip()))
            .ok_or_else(|| {
                (
                    "request_error",
                    format!("{host} resolves to no address a webhook may be sent to"),
                )
            })?;
        // The address checked is the address used: a name that resolves
        // somewhere else by the time the request connects is not asked again.
        let client = reqwest::Client::builder()
            .resolve(&host, addr)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(DELIVERY_TIMEOUT)
            .build()
            .unwrap_or_else(|_| self.http.clone());
        let response = client
            .post(url)
            .header("content-type", "application/json")
            .header("e2b-signature-version", "v1")
            .header("e2b-signature", sign(&hook.secret, body))
            .header("e2b-webhook-id", &hook.id)
            .header("e2b-delivery-id", delivery_id)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ("timeout", e.to_string())
                } else {
                    ("transport_error", e.to_string())
                }
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        Ok((status, text))
    }
}

/// E2B's webhook signature: base64 of SHA-256 over the secret followed by
/// the body, without padding.
#[must_use]
pub fn sign(secret: &str, body: &str) -> String {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    hash.update(secret.as_bytes());
    hash.update(body.as_bytes());
    base64_unpadded(&hash.finalize())
}

fn base64_unpadded(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Whether `ip` is an address on the public internet -- not loopback,
/// private, link-local (the cloud metadata service), shared, multicast,
/// documentation, or unspecified.
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
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_global(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80 // link-local
                || first == 0x2001 && v6.segments()[1] == 0x0db8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_e2bs() {
        // sha256("secret" + "{}") = 0bbd...; base64 without padding.
        let sig = sign("secret", "{}");
        assert!(!sig.ends_with('='));
        assert_eq!(sig.len(), 43);
        assert_eq!(base64_unpadded(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_unpadded(b"fo"), "Zm8");
    }

    #[test]
    fn only_public_addresses_are_global() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_global(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700::1111"] {
            assert!(is_global(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn events_map_to_e2b_types() {
        let e = ClusterEvent::new("sandbox-expired", "n", Some("sb")).with_template("base");
        let v = to_e2b(&e).unwrap();
        assert_eq!(v["type"], "sandbox.lifecycle.killed");
        assert_eq!(v["sandboxTemplateId"], "base");
        assert!(to_e2b(&ClusterEvent::new("node-joined", "n", None)).is_none());
    }

    /// The payload of E2B's webhook documentation, key for key.
    #[test]
    fn a_webhook_payload_has_e2bs_documented_shape() {
        let record = crate::model::SandboxRecord {
            sandbox_id: "sb".into(),
            node_id: "n".into(),
            template_id: "base".into(),
            started_at_ms: 1_000,
            end_at_ms: 0,
            cpu_count: 2,
            memory_mb: 512,
            metadata: [("k".to_string(), "v".to_string())].into(),
            envd_version: String::new(),
            descriptor: Value::Null,
            paused: false,
            portable: false,
            volume_mounts: Vec::new(),
        };
        let mut e = ClusterEvent::new("sandbox-deleted", "n", Some("sb"));
        e.at_ms = 2_000;
        let v = to_webhook(&e.with_record(&record)).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "event_category",
                "event_data",
                "event_label",
                "events_ttl_days",
                "id",
                "sandbox_build_id",
                "sandbox_execution_id",
                "sandbox_id",
                "sandbox_team_id",
                "sandbox_template_id",
                "timestamp",
                "type",
                "version",
            ]
        );
        assert_eq!(v["event_label"], "kill");
        assert_eq!(v["event_data"]["kill_reason"], "request");
        assert_eq!(v["event_data"]["sandbox_metadata"]["k"], "v");
        let x = &v["event_data"]["execution"];
        assert_eq!(x["vcpu_count"], 2);
        assert_eq!(x["memory_mb"], 512);
        assert_eq!(x["execution_time"], 1_000);
        assert!(x["started_at"]
            .as_str()
            .unwrap()
            .starts_with("1970-01-01T00:00:01"));
        // A created event has metadata and no execution yet.
        let c = ClusterEvent::new("sandbox-created", "n", Some("sb")).with_record(&record);
        let c = to_webhook(&c).unwrap();
        assert!(c["event_data"].get("execution").is_none());
        assert!(c["event_data"].get("kill_reason").is_none());
    }

    /// E2B's documented Python verifier, transcribed: the signature it
    /// computes for a body is the one sent.
    #[test]
    fn the_signature_is_what_e2bs_documented_verifier_expects() {
        // base64(sha256("secret" + "{}")) with its padding stripped, computed
        // independently with Python's hashlib.
        assert_eq!(
            sign("secret", "{}"),
            "e9B8ejqc2/sG6v6Lh2cAHsXgRKkAkGentpGLCOuoSYs"
        );
    }
}
