//! Signing in through an OpenID Connect provider: the browser flow, and the
//! session it ends in.
//!
//! [`crate::sso`] decides what a token proves; this is the part that talks to
//! the provider and the browser.
//!
//! - `GET /auth/login?returnTo=/path` sends the browser to the provider with
//!   a fresh `state`, `nonce` and PKCE verifier, which ride in a short-lived
//!   cookie this control plane signed -- nothing is kept server-side.
//! - `GET /auth/callback` checks `state` against that cookie, exchanges the
//!   code (with the verifier) at the provider's token endpoint, verifies the
//!   ID token and its nonce, and looks the verified email up in the members
//!   file. A member gets a session cookie; anyone else, nothing.
//! - `GET /auth/session` says who the session is; `POST /auth/logout` clears
//!   the cookie.
//! - `hm login` uses the same flow with `cliPort` and `cliChallenge`: the
//!   callback sends the browser on to the CLI's listener on `127.0.0.1` with
//!   a sealed one-time code instead of setting a cookie, and the CLI trades
//!   code and PKCE verifier for a session token at `POST /auth/cli-token`. A
//!   code seen on its way to the CLI is useless without the verifier, and
//!   expires in a minute.
//!
//! The session cookie is `__Host-hm_session`: `Secure`, `HttpOnly`, `Path=/`,
//! no `Domain`, `SameSite=Lax`. Host-only, so sandbox hosts under the same
//! domain never receive it. The API also accepts the same token as
//! `Authorization: Bearer`, for the CLI.
//!
//! A session is stateless: it lasts until it expires, or until its email
//! leaves the members file (checked on every request). Logging out clears the
//! cookie but cannot recall a copy of the token.
//!
//! Cookie-authenticated requests that change anything must carry an `Origin`
//! naming this control plane. Sandboxes serve guest-controlled pages on the
//! same site, and `SameSite=Lax` still sends the cookie with a same-site POST.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::control::ControlPlane;
use crate::sso::{IdTokenRules, Jwks, Member, Members, SessionKey, SsoError};

/// The session cookie's name. `__Host-` makes a browser refuse it unless it
/// is `Secure`, has `Path=/` and no `Domain`.
pub const SESSION_COOKIE: &str = "__Host-hm_session";
/// The cookie a login keeps its state in while the browser is away.
pub const LOGIN_COOKIE: &str = "__Host-hm_login";
/// How long a login may take at the provider.
const LOGIN_TTL_SECS: i64 = 600;
/// How long the CLI has to trade its one-time code for a session.
const CLI_CODE_TTL_SECS: i64 = 60;
/// The least time between two JWKS fetches for an unknown `kid`.
const JWKS_REFETCH: Duration = Duration::from_secs(60);

/// How a control plane signs people in.
pub struct SsoConfig {
    /// The provider's issuer, exactly as its discovery document gives it.
    pub issuer: String,
    pub client_id: String,
    /// Absent for a public client, which PKCE alone protects.
    pub client_secret: Option<zeroize::Zeroizing<String>>,
    /// Where the provider sends the browser back: this control plane's
    /// `/auth/callback`, as registered with the provider.
    pub redirect_url: String,
    pub session_ttl: Duration,
    /// Extra trust roots for the provider's TLS (PEM), beyond the Web PKI.
    pub provider_ca_pem: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

/// A provider this control plane signs people in through.
pub struct SsoLogin {
    config: SsoConfig,
    discovery: Discovery,
    jwks: parking_lot::RwLock<(Jwks, Instant)>,
    members: parking_lot::RwLock<Members>,
    key: SessionKey,
    /// This control plane's own origin, from the redirect URL: what an
    /// `Origin` header must say on a cookie-authenticated change.
    origin: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for SsoLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsoLogin")
            .field("issuer", &self.config.issuer)
            .finish_non_exhaustive()
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    hv2_core::crypto::FipsCrypto::new(hv2_core::crypto::FipsMode::Enabled)
        .and_then(|crypto| crypto.random_bytes(&mut buffer))
        .map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(data).into()
}

/// `scheme://host[:port]` of a URL.
fn origin_of(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{url} has no host"))?;
    // `port()` is only an explicit, non-default port, which is exactly what
    // a browser's `Origin` header includes.
    Ok(match parsed.port() {
        Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
        None => format!("{}://{host}", parsed.scheme()),
    })
}

impl SsoLogin {
    /// Fetch the provider's discovery document and keys, and get ready to
    /// sign people in.
    ///
    /// # Errors
    /// A provider that cannot be reached, whose discovery document names a
    /// different issuer, or whose keys are unusable; a redirect URL that is
    /// not HTTPS (session cookies are `Secure`).
    pub async fn discover(
        config: SsoConfig,
        members: Members,
        key: SessionKey,
    ) -> Result<Self, String> {
        let origin = origin_of(&config.redirect_url)?;
        if !config.redirect_url.starts_with("https://") {
            return Err("the SSO redirect URL must be HTTPS: session cookies are Secure".into());
        }
        // A provider's keys and tokens over plain HTTP are anyone's on the
        // path. Loopback is allowed, for a provider on the same host.
        let issuer = reqwest::Url::parse(&config.issuer).map_err(|e| format!("issuer: {e}"))?;
        let loopback = matches!(issuer.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if issuer.scheme() != "https" && !loopback {
            return Err("the SSO issuer must be HTTPS".into());
        }
        let http = hv2_tls::http_client(config.provider_ca_pem.as_deref())
            .map_err(|e| format!("provider CA: {e}"))?
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let url = format!(
            "{}/.well-known/openid-configuration",
            config.issuer.trim_end_matches('/')
        );
        let discovery: Discovery = http
            .get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| format!("fetching {url}: {e}"))?
            .json()
            .await
            .map_err(|e| format!("reading {url}: {e}"))?;
        // OpenID Connect Discovery 4.3: the document must name the issuer it
        // was fetched for, or tokens from it name someone else.
        if discovery.issuer != config.issuer {
            return Err(format!(
                "the provider's discovery document names issuer {:?}, not {:?}",
                discovery.issuer, config.issuer
            ));
        }
        let jwks = fetch_jwks(&http, &discovery.jwks_uri).await?;
        Ok(Self {
            config,
            discovery,
            jwks: parking_lot::RwLock::new((jwks, Instant::now())),
            members: parking_lot::RwLock::new(members),
            key,
            origin,
            http,
        })
    }

    /// Replace the members, after validating the whole document; a rejected
    /// one leaves the current members in place.
    ///
    /// # Errors
    /// What [`Members::from_json`] refuses.
    pub fn replace_members(&self, json: &str) -> Result<(), String> {
        *self.members.write() = Members::from_json(json)?;
        Ok(())
    }

    /// The member a session token names, if it is valid and they still are
    /// one.
    pub(crate) fn member_for(&self, token: &str) -> Option<(Member, i64)> {
        let session = self.key.verify(token, now()).ok()?;
        let member = self.members.read().get(&session.email).cloned()?;
        Some((member, session.exp))
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn key(&self) -> &SessionKey {
        &self.key
    }

    pub(crate) fn session_ttl(&self) -> Duration {
        self.config.session_ttl
    }

    /// The member with this verified email, as the members file says now.
    pub(crate) fn member(&self, email: &str) -> Option<Member> {
        self.members.read().get(email).cloned()
    }

    /// Keys to check `token` with: the cached set, refetched first when the
    /// token names a `kid` it lacks and the last fetch was a while ago -- a
    /// provider rotates keys in before it signs with them.
    async fn keys_for(&self, token: &str) -> Jwks {
        let (stale, keys) = {
            let cached = self.jwks.read();
            let unknown = crate::sso::token_kid(token).is_some_and(|kid| !cached.0.has_kid(&kid));
            (
                unknown && cached.1.elapsed() >= JWKS_REFETCH,
                cached.0.clone(),
            )
        };
        if !stale {
            return keys;
        }
        match fetch_jwks(&self.http, &self.discovery.jwks_uri).await {
            Ok(fresh) => {
                *self.jwks.write() = (fresh.clone(), Instant::now());
                fresh
            }
            Err(e) => {
                tracing::warn!("SSO: refetching the provider's keys: {e}");
                // Do not retry on every request while the provider is down.
                self.jwks.write().1 = Instant::now();
                keys
            }
        }
    }
}

async fn fetch_jwks(http: &reqwest::Client, url: &str) -> Result<Jwks, String> {
    let body = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("fetching {url}: {e}"))?
        .text()
        .await
        .map_err(|e| format!("reading {url}: {e}"))?;
    Jwks::from_json(&body).map_err(|e| e.to_string())
}

/// A `Set-Cookie` value for one of this module's cookies.
fn cookie(name: &str, value: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age}"
    ))
    .expect("cookie values are base64url and ASCII")
}

/// The value of cookie `name` in a request.
pub(crate) fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// The session token a request carries: a bearer token, else the cookie.
/// The second element says whether it came from the cookie, which is the
/// one a browser attaches by itself.
pub(crate) fn session_token(headers: &HeaderMap) -> Option<(&str, bool)> {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|token| token.starts_with("hms1."))
    {
        return Some((bearer, false));
    }
    cookie_value(headers, SESSION_COOKIE).map(|token| (token, true))
}

/// Whether a request that may change something came from this control
/// plane's own pages. Safe methods always pass.
pub(crate) fn same_origin(method: &Method, headers: &HeaderMap, origin: &str) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        || headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|sent| sent == origin)
}

fn not_configured() -> Response {
    error(StatusCode::NOT_FOUND, "single sign-on is not configured")
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"code": status.as_u16(), "message": message})),
    )
        .into_response()
}

/// What a login keeps in its cookie while the browser is at the provider.
#[derive(Serialize, Deserialize)]
struct Pending {
    state: String,
    nonce: String,
    verifier: String,
    #[serde(rename = "returnTo")]
    return_to: String,
    exp: i64,
    /// For `hm login`: the port its loopback listener is on, and the PKCE
    /// challenge it will answer at `/auth/cli-token`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cli: Option<CliLogin>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CliLogin {
    port: u16,
    challenge: String,
}

/// The one-time code a CLI login ends in.
#[derive(Serialize, Deserialize)]
struct CliCode {
    email: String,
    challenge: String,
    exp: i64,
}

/// A path to return to after signing in: this control plane's own, never
/// another site's (`//evil.example` and `/\evil.example` are other sites to
/// a browser).
fn safe_return(path: Option<&str>) -> String {
    match path {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.starts_with("/\\")
                && !p.contains(['\r', '\n'])
                && p.len() <= 1024 =>
        {
            p.to_string()
        }
        _ => "/ui".to_string(),
    }
}

#[derive(Deserialize)]
pub(crate) struct LoginQuery {
    #[serde(rename = "returnTo")]
    return_to: Option<String>,
    #[serde(rename = "cliPort")]
    cli_port: Option<u16>,
    #[serde(rename = "cliChallenge")]
    cli_challenge: Option<String>,
}

/// A CLI login's parameters, if the request is one: an unprivileged port
/// and a challenge shaped like a base64url SHA-256.
fn cli_login(query: &LoginQuery) -> Result<Option<CliLogin>, &'static str> {
    match (query.cli_port, &query.cli_challenge) {
        (None, None) => Ok(None),
        (Some(port), Some(challenge))
            if port >= 1024
                && challenge.len() == 43
                && challenge
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
        {
            Ok(Some(CliLogin {
                port,
                challenge: challenge.clone(),
            }))
        }
        _ => Err(
            "cliPort and cliChallenge go together: a port from 1024, a 43-character S256 challenge",
        ),
    }
}

/// `GET /auth/login`.
pub(crate) async fn login(
    State(control): State<Arc<ControlPlane>>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let Some(sso) = control.sso() else {
        return not_configured();
    };
    let cli = match cli_login(&query) {
        Ok(cli) => cli,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let tokens = (random_token(32), random_token(32), random_token(32));
    let (Ok(state), Ok(nonce), Ok(verifier)) = tokens else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "no randomness");
    };
    let challenge = URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()));
    let pending = Pending {
        state: state.clone(),
        nonce: nonce.clone(),
        verifier,
        return_to: safe_return(query.return_to.as_deref()),
        exp: now() + LOGIN_TTL_SECS,
        cli,
    };
    let Ok(sealed) = sso.key.seal("hml1", &pending) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "sealing the login");
    };
    let mut url = match reqwest::Url::parse(&sso.discovery.authorization_endpoint) {
        Ok(url) => url,
        Err(_) => return error(StatusCode::BAD_GATEWAY, "the provider's authorization URL"),
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &sso.config.client_id)
        .append_pair("redirect_uri", &sso.config.redirect_url)
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(url.as_str()).expect("a URL is a header value"),
    );
    headers.insert(
        header::SET_COOKIE,
        cookie(LOGIN_COOKIE, &sealed, LOGIN_TTL_SECS),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// `GET /auth/callback`.
pub(crate) async fn callback(
    State(control): State<Arc<ControlPlane>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(sso) = control.sso() else {
        return not_configured();
    };
    if query.error.is_some() {
        return error(StatusCode::UNAUTHORIZED, "the provider refused the sign-in");
    }
    let Some(pending) = cookie_value(&headers, LOGIN_COOKIE)
        .and_then(|sealed| sso.key.open::<Pending>("hml1", sealed).ok())
        .filter(|pending| pending.exp > now())
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "no sign-in in progress; start again",
        );
    };
    let (Some(code), Some(state)) = (query.code, query.state) else {
        return error(StatusCode::BAD_REQUEST, "the provider sent no code");
    };
    if !bool::from(subtle::ConstantTimeEq::ct_eq(
        state.as_bytes(),
        pending.state.as_bytes(),
    )) {
        return error(StatusCode::BAD_REQUEST, "state does not match this sign-in");
    }
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", sso.config.redirect_url.as_str()),
        ("code_verifier", pending.verifier.as_str()),
    ];
    let mut request = sso.http.post(&sso.discovery.token_endpoint);
    match &sso.config.client_secret {
        // client_secret_basic, the method a provider must support.
        Some(secret) => {
            request = request.basic_auth(&sso.config.client_id, Some(secret.as_str()));
        }
        None => form.push(("client_id", sso.config.client_id.as_str())),
    }
    let tokens: TokenResponse = match request.form(&form).send().await {
        Ok(response) if response.status().is_success() => match response.json().await {
            Ok(tokens) => tokens,
            Err(_) => return error(StatusCode::BAD_GATEWAY, "the provider's token answer"),
        },
        Ok(_) => return error(StatusCode::UNAUTHORIZED, "the provider refused the code"),
        Err(_) => return error(StatusCode::BAD_GATEWAY, "the provider's token endpoint"),
    };
    let keys = sso.keys_for(&tokens.id_token).await;
    let identity = match crate::sso::verify_id_token(
        &tokens.id_token,
        &keys,
        &IdTokenRules {
            issuer: &sso.config.issuer,
            client_id: &sso.config.client_id,
            nonce: Some(&pending.nonce),
            now: now(),
        },
    ) {
        Ok(identity) => identity,
        Err(e) => {
            tracing::warn!("SSO: an ID token was refused: {e}");
            return error(
                StatusCode::UNAUTHORIZED,
                "the provider's ID token was refused",
            );
        }
    };
    if sso.members.read().get(&identity.email).is_none() {
        tracing::info!("SSO: {} signed in but is not a member", identity.email);
        return error(
            StatusCode::FORBIDDEN,
            "signed in, but not a member of this control plane",
        );
    }
    if let Some(cli) = pending.cli {
        // On to the CLI on this machine, with a code only its verifier opens.
        let code = CliCode {
            email: identity.email.clone(),
            challenge: cli.challenge,
            exp: now() + CLI_CODE_TTL_SECS,
        };
        let Ok(sealed) = sso.key.seal("hmc1", &code) else {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "sealing the CLI code");
        };
        tracing::info!("SSO: {} signed in for the CLI", identity.email);
        let mut response = StatusCode::SEE_OTHER.into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::LOCATION,
            HeaderValue::from_str(&format!(
                "http://127.0.0.1:{}/callback?code={sealed}",
                cli.port
            ))
            .expect("a port and base64url"),
        );
        headers.insert(header::SET_COOKIE, cookie(LOGIN_COOKIE, "", 0));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    let ttl = i64::try_from(sso.config.session_ttl.as_secs()).unwrap_or(i64::MAX);
    let Ok(session) = sso.key.mint(&identity.email, now(), ttl) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "minting the session");
    };
    tracing::info!("SSO: {} signed in", identity.email);
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&pending.return_to).expect("checked by safe_return"),
    );
    headers.append(header::SET_COOKIE, cookie(SESSION_COOKIE, &session, ttl));
    headers.append(header::SET_COOKIE, cookie(LOGIN_COOKIE, "", 0));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Deserialize)]
pub(crate) struct CliTokenRequest {
    code: String,
    verifier: String,
}

/// `POST /auth/cli-token`: a CLI login's code and verifier, for a session
/// token. The member is looked up again: one removed since signing in gets
/// nothing.
pub(crate) async fn cli_token(
    State(control): State<Arc<ControlPlane>>,
    Json(request): Json<CliTokenRequest>,
) -> Response {
    let Some(sso) = control.sso() else {
        return not_configured();
    };
    let Some(code) = sso
        .key
        .open::<CliCode>("hmc1", &request.code)
        .ok()
        .filter(|code| code.exp > now())
    else {
        return error(
            StatusCode::UNAUTHORIZED,
            "the sign-in code is not valid or has expired",
        );
    };
    let answered = URL_SAFE_NO_PAD.encode(sha256(request.verifier.as_bytes()));
    if !bool::from(subtle::ConstantTimeEq::ct_eq(
        answered.as_bytes(),
        code.challenge.as_bytes(),
    )) {
        return error(
            StatusCode::UNAUTHORIZED,
            "the verifier does not answer this sign-in",
        );
    }
    if sso.members.read().get(&code.email).is_none() {
        return error(
            StatusCode::FORBIDDEN,
            "no longer a member of this control plane",
        );
    }
    let ttl = i64::try_from(sso.config.session_ttl.as_secs()).unwrap_or(i64::MAX);
    let issued = now();
    match sso.key.mint(&code.email, issued, ttl) {
        Ok(token) => Json(json!({
            "token": token,
            "email": code.email,
            "expiresAt": issued + ttl,
        }))
        .into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "minting the session"),
    }
}

/// `GET /auth/session`: who the session is.
pub(crate) async fn session(
    State(control): State<Arc<ControlPlane>>,
    headers: HeaderMap,
) -> Response {
    let Some(sso) = control.sso() else {
        return not_configured();
    };
    let Some((member, exp)) = session_token(&headers).and_then(|(t, _)| sso.member_for(t)) else {
        return error(StatusCode::UNAUTHORIZED, "not signed in");
    };
    Json(json!({
        "email": member.email,
        "principalID": member.principal_id.as_str(),
        "teamID": member.team_id.as_ref().map(crate::ownership::TeamId::as_str),
        "expiresAt": exp,
    }))
    .into_response()
}

/// `POST /auth/logout`: clear the session cookie.
pub(crate) async fn logout(
    State(control): State<Arc<ControlPlane>>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some(sso) = control.sso() else {
        return not_configured();
    };
    if !same_origin(&method, &headers, sso.origin()) {
        return error(
            StatusCode::FORBIDDEN,
            "sign out from this control plane's own pages",
        );
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie(SESSION_COOKIE, "", 0));
    response
}

impl SessionKey {
    /// MAC-protect `value` under `prefix`, for a cookie that must come back
    /// unchanged.
    pub(crate) fn seal<T: Serialize>(&self, prefix: &str, value: &T) -> Result<String, SsoError> {
        let body = format!(
            "{prefix}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("serializable"))
        );
        let mac = self.mac_of(&body)?;
        Ok(format!("{body}.{}", URL_SAFE_NO_PAD.encode(mac)))
    }

    /// What [`Self::seal`] protected, if this key sealed it under `prefix`.
    pub(crate) fn open<T: for<'de> Deserialize<'de>>(
        &self,
        prefix: &str,
        sealed: &str,
    ) -> Result<T, SsoError> {
        let (body, mac) = sealed
            .rsplit_once('.')
            .ok_or(SsoError::Malformed("not sealed"))?;
        let payload = body
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('.'))
            .ok_or(SsoError::Malformed("not sealed"))?;
        let expected = self.mac_of(body)?;
        let given = URL_SAFE_NO_PAD
            .decode(mac)
            .map_err(|_| SsoError::Malformed("not sealed"))?;
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            given.as_slice(),
            &expected[..],
        )) {
            return Err(SsoError::Signature);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| SsoError::Malformed("not sealed"))?;
        serde_json::from_slice(&bytes).map_err(|_| SsoError::Malformed("not sealed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cli_login_needs_both_parameters_well_formed() {
        let query = |port: Option<u16>, challenge: Option<&str>| LoginQuery {
            return_to: None,
            cli_port: port,
            cli_challenge: challenge.map(str::to_string),
        };
        let good = URL_SAFE_NO_PAD.encode([1u8; 32]);
        assert!(cli_login(&query(None, None)).unwrap().is_none());
        assert_eq!(
            cli_login(&query(Some(49152), Some(&good)))
                .unwrap()
                .unwrap()
                .port,
            49152
        );
        for bad in [
            query(Some(49152), None),
            query(None, Some(&good)),
            query(Some(80), Some(&good)),
            query(Some(49152), Some("short")),
            query(Some(49152), Some(&format!("{}!", &good[..42]))),
        ] {
            assert!(cli_login(&bad).is_err());
        }
    }

    #[test]
    fn only_this_control_planes_own_paths_are_returned_to() {
        assert_eq!(safe_return(Some("/sandboxes")), "/sandboxes");
        for hostile in [
            "//evil.example/",
            "/\\evil.example",
            "https://evil.example/",
            "evil",
            "/x\r\nSet-Cookie: a=b",
        ] {
            assert_eq!(safe_return(Some(hostile)), "/ui", "{hostile:?}");
        }
        assert_eq!(safe_return(None), "/ui");
    }

    #[test]
    fn a_sealed_login_opens_only_under_its_key_and_prefix() {
        let key = SessionKey::new(vec![3u8; 32]).unwrap();
        let pending = Pending {
            state: "s".into(),
            nonce: "n".into(),
            verifier: "v".into(),
            return_to: "/ui".into(),
            exp: 10,
            cli: None,
        };
        let sealed = key.seal("hml1", &pending).unwrap();
        let opened: Pending = key.open("hml1", &sealed).unwrap();
        assert_eq!(opened.verifier, "v");
        // Not a session, and not under another key.
        assert!(key.open::<Pending>("hms1", &sealed).is_err());
        let other = SessionKey::new(vec![4u8; 32]).unwrap();
        assert!(other.open::<Pending>("hml1", &sealed).is_err());
        // A session token is not a login either.
        let session = key.mint("a@x", 0, 10).unwrap();
        assert!(key.open::<Pending>("hml1", &session).is_err());
    }

    #[test]
    fn a_cookie_is_found_among_others_and_a_bearer_wins() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; __Host-hm_session=hms1.x.y; b=2"),
        );
        assert_eq!(session_token(&headers), Some(("hms1.x.y", true)));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer hms1.p.q"),
        );
        assert_eq!(session_token(&headers), Some(("hms1.p.q", false)));
        // Basic is web access, not a session.
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic eDp5"),
        );
        assert_eq!(session_token(&headers), Some(("hms1.x.y", true)));
    }

    #[test]
    fn a_change_needs_this_origin_and_a_read_does_not() {
        let mut headers = HeaderMap::new();
        let origin = "https://cp.example";
        assert!(same_origin(&Method::GET, &headers, origin));
        assert!(!same_origin(&Method::POST, &headers, origin));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://8080-sbx.cp.example"),
        );
        assert!(!same_origin(&Method::DELETE, &headers, origin));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://cp.example"),
        );
        assert!(same_origin(&Method::POST, &headers, origin));
    }
}
