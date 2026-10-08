//! Single sign-on for private guest URLs (`{port}-{sandbox}.{domain}`).
//!
//! The API's session cookie is host-only on the control plane, and must stay
//! that way: a guest URL is served by a program in the guest, and a cookie
//! its browser sent there would be that program's to keep. So a guest URL
//! gets a credential of its own, bound to its one host, which the proxy
//! removes before anything reaches the guest:
//!
//! 1. A browser at a guest URL with no credential is sent to the control
//!    plane's `/auth/guest?url=…`. There, a signed-in member (anyone else is
//!    sent through `/auth/login` first) who may view that sandbox gets a
//!    one-minute handoff token sealed to that host, and is sent back to
//!    `https://{host}/__hm/auth`.
//! 2. The proxy checks the handoff names this host and sets
//!    `__Host-hm_guest`: Secure, HttpOnly, host-only, sealed to the host.
//! 3. On every request the proxy checks that cookie, looks the member up
//!    again and checks they may still view the sandbox. Then it strips the
//!    cookie and sends the guest `X-HyperMachine-User: <verified email>`, as
//!    an exe.dev guest receives `X-ExeDev-Email`.
//!
//! Who may view a sandbox: an administrator; a member of its team (or, in a
//! deployment without teams, its creator); or an email its owner granted
//! through web sharing.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::control::ControlPlane;
use crate::sso::Member;
use crate::sso_login::SsoLogin;
use crate::store::ClusterStore;

/// The cookie a guest URL's host keeps.
pub const GUEST_COOKIE: &str = "__Host-hm_guest";
/// The path on a guest URL's host that turns a handoff into the cookie.
pub const HANDOFF_PATH: &str = "/__hm/auth";
/// How long a handoff may take between the control plane and the guest host.
const HANDOFF_TTL_SECS: i64 = 60;

/// A sign-in on its way from the control plane to one guest host.
#[derive(Serialize, Deserialize)]
struct Handoff {
    email: String,
    host: String,
    exp: i64,
}

/// What `__Host-hm_guest` holds.
#[derive(Serialize, Deserialize)]
struct Pass {
    email: String,
    host: String,
    exp: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Whether `member` may view `sandbox_id`'s guest URLs, as of now.
pub(crate) async fn may_view(store: &dyn ClusterStore, member: &Member, sandbox_id: &str) -> bool {
    let administrator = member.role == crate::keys::ApiRole::Operator
        && member.scopes.contains(&crate::keys::ApiScope::Admin);
    let lookup = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.web_sharing_snapshot(sandbox_id),
    )
    .await;
    let (record, sharing) = match lookup {
        Ok(Ok(Some((record, sharing)))) => (record, Some(sharing)),
        // No sharing state yet: the record alone decides.
        Ok(Ok(None)) => match store.sandbox(sandbox_id).await {
            Ok(Some(record)) => (record, None),
            _ => return false,
        },
        _ => return false,
    };
    if administrator {
        return true;
    }
    let by_membership = match &member.team_id {
        Some(team) => record.team_id.as_ref() == Some(team),
        None => record.owner_id.as_ref() == Some(&member.principal_id),
    };
    by_membership || sharing.is_some_and(|sharing| sharing.allows(&record, &member.email, now()))
}

/// A path to land on after the handoff: this host's own, never another's.
fn safe_path(path: Option<&str>) -> String {
    match path {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.starts_with("/\\")
                && !p.contains(['\r', '\n'])
                && p.len() <= 2048 =>
        {
            p.to_string()
        }
        _ => "/".to_string(),
    }
}

fn cookie_header(value: &str, max_age: i64) -> String {
    format!("{GUEST_COOKIE}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age}")
}

/// The sandbox and host a guest URL names, if it names one this control
/// plane routes.
async fn route_of_url(control: &ControlPlane, url: &str) -> Option<(String, String)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() {
        return None;
    }
    let host = parsed.host_str()?;
    let authority = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    if let Some((_, sandbox)) = hv2_api::sandbox_proxy::route_of(&authority) {
        return Some((authority.clone(), sandbox.to_string()));
    }
    let name = crate::domains::DomainName::parse(host).ok()?;
    let binding = control.store().domain(&name).await.ok()??;
    Some((authority, binding.sandbox_id().to_string()))
}

#[derive(Deserialize)]
pub(crate) struct GuestQuery {
    url: String,
}

/// `GET /auth/guest?url=…`: send a signed-in member who may view the sandbox
/// back to its guest URL with a handoff for that host.
pub(crate) async fn guest(
    State(control): State<Arc<ControlPlane>>,
    headers: HeaderMap,
    Query(query): Query<GuestQuery>,
) -> Response {
    let Some(sso) = control.sso() else {
        return (StatusCode::NOT_FOUND, "single sign-on is not configured").into_response();
    };
    let member = crate::sso_login::session_token(&headers)
        .filter(|(_, from_cookie)| *from_cookie)
        .and_then(|(token, _)| sso.member_for(token));
    let Some((member, _)) = member else {
        // Sign in first, then come back here.
        let back = format!("/auth/guest?url={}", url_encode(&query.url));
        let to = format!("/auth/login?returnTo={}", url_encode(&back));
        return redirect(&to, None);
    };
    let Some((host, sandbox)) = route_of_url(&control, &query.url).await else {
        return (
            StatusCode::BAD_REQUEST,
            "not a guest URL of this control plane",
        )
            .into_response();
    };
    if !may_view(control.store().as_ref(), &member, &sandbox).await {
        return (StatusCode::FORBIDDEN, "you may not view this sandbox").into_response();
    }
    let handoff = Handoff {
        email: member.email.clone(),
        host: host.clone(),
        exp: now() + HANDOFF_TTL_SECS,
    };
    let Ok(sealed) = sso.key().seal("hmg1", &handoff) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let path = reqwest::Url::parse(&query.url)
        .map(|u| match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        })
        .unwrap_or_else(|_| "/".into());
    redirect(
        &format!(
            "https://{host}{HANDOFF_PATH}?token={sealed}&path={}",
            url_encode(&path)
        ),
        None,
    )
}

fn redirect(location: &str, cookie: Option<String>) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(location) {
        headers.insert(header::LOCATION, value);
    }
    if let Some(cookie) = cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        headers.insert(header::SET_COOKIE, cookie);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The host a request was addressed to.
fn request_host(headers: &HeaderMap, uri: &axum::http::Uri) -> Option<String> {
    uri.authority().map(|a| a.as_str().to_string()).or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
    })
}

/// The proxy's answer to a guest-URL request it should not forward: the
/// handoff turned into a cookie, or a browser with no credential sent to
/// sign in. `None` forwards to admission.
pub(crate) fn intercept(
    sso: &SsoLogin,
    uri: &axum::http::Uri,
    headers: &HeaderMap,
    basic_auth_allowed: bool,
) -> Option<hv2_api::sandbox_proxy::ProxyAnswer> {
    let host = request_host(headers, uri)?;
    if uri.path() == HANDOFF_PATH {
        let query: std::collections::HashMap<String, String> =
            uri.query().map(url_decode_pairs).unwrap_or_default();
        let handoff = query
            .get("token")
            .and_then(|token| sso.key().open::<Handoff>("hmg1", token).ok())
            .filter(|h| h.host == host && h.exp > now());
        let Some(handoff) = handoff else {
            return Some(hv2_api::sandbox_proxy::ProxyAnswer {
                status: 403,
                location: None,
                set_cookie: None,
            });
        };
        let ttl = i64::try_from(sso.session_ttl().as_secs()).unwrap_or(i64::MAX);
        let pass = Pass {
            email: handoff.email,
            host,
            exp: now() + ttl,
        };
        let sealed = sso.key().seal("hmh1", &pass).ok()?;
        return Some(hv2_api::sandbox_proxy::ProxyAnswer {
            status: 303,
            location: Some(safe_path(query.get("path").map(String::as_str))),
            set_cookie: Some(cookie_header(&sealed, ttl)),
        });
    }
    let has_pass = crate::sso_login::cookie_value(headers, GUEST_COOKIE).is_some();
    let has_basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Basic "));
    if has_pass || (has_basic && basic_auth_allowed) {
        return None;
    }
    let path = uri
        .path_and_query()
        .map_or("/", axum::http::uri::PathAndQuery::as_str);
    Some(hv2_api::sandbox_proxy::ProxyAnswer {
        status: 303,
        location: Some(format!(
            "{}/auth/guest?url={}",
            sso.origin(),
            url_encode(&format!("https://{host}{path}"))
        )),
        set_cookie: None,
    })
}

fn url_decode_pairs(query: &str) -> std::collections::HashMap<String, String> {
    reqwest::Url::parse(&format!("http://x/?{query}"))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// What a guest pass says about a request.
pub(crate) enum Admission {
    /// A member who may view the sandbox: their verified email.
    Member(String),
    /// No pass at all.
    None,
    /// A pass that is invalid, for another host, expired, or whose member
    /// may no longer view the sandbox.
    Refused,
}

/// Check a request's guest pass, and remove it from what the guest will
/// receive either way.
pub(crate) async fn admit(
    sso: &SsoLogin,
    store: &dyn ClusterStore,
    sandbox: &str,
    headers: &mut HeaderMap,
) -> Admission {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let pass = crate::sso_login::cookie_value(headers, GUEST_COOKIE).map(str::to_string);
    strip_cookie(headers, GUEST_COOKIE);
    let Some(pass) = pass else {
        return Admission::None;
    };
    let Some(pass) = sso
        .key()
        .open::<Pass>("hmh1", &pass)
        .ok()
        .filter(|p| Some(&p.host) == host.as_ref() && p.exp > now())
    else {
        return Admission::Refused;
    };
    let Some(member) = sso.member(&pass.email) else {
        return Admission::Refused;
    };
    if may_view(store, &member, sandbox).await {
        Admission::Member(member.email)
    } else {
        Admission::Refused
    }
}

/// Remove cookie `name` from a request's `Cookie` headers, keeping the rest:
/// the guest's own cookies are its business.
fn strip_cookie(headers: &mut HeaderMap, name: &str) {
    let kept: Vec<String> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .filter(|pair| !pair.is_empty() && pair.split_once('=').is_none_or(|(k, _)| k != name))
        .map(str::to_string)
        .collect();
    headers.remove(header::COOKIE);
    if !kept.is_empty() {
        if let Ok(value) = HeaderValue::from_str(&kept.join("; ")) {
            headers.insert(header::COOKIE, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pass_is_removed_and_the_guests_own_cookies_kept() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("app=1; __Host-hm_guest=hmh1.x.y; theme=dark"),
        );
        strip_cookie(&mut headers, GUEST_COOKIE);
        assert_eq!(headers[header::COOKIE], "app=1; theme=dark");
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-hm_guest=a"),
        );
        strip_cookie(&mut headers, GUEST_COOKIE);
        assert!(headers.get(header::COOKIE).is_none());
    }

    #[test]
    fn a_handoff_lands_only_on_this_hosts_own_paths() {
        assert_eq!(safe_path(Some("/app?x=1")), "/app?x=1");
        for hostile in ["//evil.example", "/\\evil", "https://evil", "x", "/a\r\nb"] {
            assert_eq!(safe_path(Some(hostile)), "/", "{hostile:?}");
        }
    }

    #[test]
    fn urls_are_encoded_for_a_query() {
        assert_eq!(
            url_encode("https://8080-sbx.example/a?b=c&d"),
            "https%3A%2F%2F8080-sbx.example%2Fa%3Fb%3Dc%26d"
        );
    }
}
