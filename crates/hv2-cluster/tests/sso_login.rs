//! Signing in through a real (fixture) OpenID Connect provider, end to end:
//! discovery, the browser redirect, PKCE, the code exchange, ID-token
//! verification, membership, the session cookie and bearer token, and the
//! API admitting and refusing on them.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Form, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hv2_cluster::control::{self, ControlConfig, ControlPlane};
use hv2_cluster::sso::{Members, SessionKey};
use hv2_cluster::sso_login::{SsoConfig, SsoLogin};
use hv2_cluster::store::{ClusterStore, MemoryStore};
use hv2_core::crypto::asymmetric::{EcCurve, EcPrivateKey};
use hv2_core::crypto::{FipsCrypto, FipsMode};
use parking_lot::Mutex;
use serde_json::{json, Value};

const CLIENT_ID: &str = "hypermachine";
const CLIENT_SECRET: &str = "fixture-client-secret";
const REDIRECT: &str = "https://cp.test/auth/callback";

/// What the fixture provider does on its next sign-in.
#[derive(Clone)]
struct Next {
    email: String,
    verified: bool,
    /// Replace the nonce the login sent.
    wrong_nonce: bool,
}

struct Pending {
    nonce: String,
    challenge: String,
}

struct Provider {
    issuer: String,
    key: EcPrivateKey,
    jwk: Value,
    next: Mutex<Next>,
    codes: Mutex<std::collections::HashMap<String, Pending>>,
}

fn crypto() -> FipsCrypto {
    FipsCrypto::new(FipsMode::Enabled).unwrap()
}

impl Provider {
    fn sign(&self, claims: &Value) -> String {
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","kid":"fixture"}"#),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = crypto().ecdsa_sign(&self.key, input.as_bytes()).unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.data))
    }
}

async fn discovery(State(p): State<Arc<Provider>>) -> Json<Value> {
    Json(json!({
        "issuer": p.issuer,
        "authorization_endpoint": format!("{}/authorize", p.issuer),
        "token_endpoint": format!("{}/token", p.issuer),
        "jwks_uri": format!("{}/jwks", p.issuer),
    }))
}

async fn jwks(State(p): State<Arc<Provider>>) -> Json<Value> {
    Json(json!({"keys": [p.jwk]}))
}

/// Approve at once, as a user who is already signed in would be.
async fn authorize(
    State(p): State<Arc<Provider>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["redirect_uri"], REDIRECT);
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["scope"].split(' ').any(|s| s == "openid"));
    let code = format!("code-{}", uuid::Uuid::new_v4());
    p.codes.lock().insert(
        code.clone(),
        Pending {
            nonce: q["nonce"].clone(),
            challenge: q["code_challenge"].clone(),
        },
    );
    let location = format!("{REDIRECT}?code={code}&state={}", q["state"]);
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

async fn token(
    State(p): State<Arc<Provider>>,
    headers: HeaderMap,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Response {
    let expected = format!(
        "Basic {}",
        STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
    );
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        != Some(&expected)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(pending) = p.codes.lock().remove(&form["code"]) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // PKCE: the verifier must hash to the challenge the login sent.
    use sha2::Digest;
    let digest = sha2::Sha256::digest(form["code_verifier"].as_bytes());
    if URL_SAFE_NO_PAD.encode(digest) != pending.challenge {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let next = p.next.lock().clone();
    let now = chrono::Utc::now().timestamp();
    let id_token = p.sign(&json!({
        "iss": p.issuer,
        "aud": CLIENT_ID,
        "sub": format!("sub-{}", next.email),
        "email": next.email,
        "email_verified": next.verified,
        "iat": now,
        "exp": now + 300,
        "nonce": if next.wrong_nonce { "not-the-nonce".to_string() } else { pending.nonce },
    }));
    Json(json!({"id_token": id_token, "token_type": "Bearer", "access_token": "x"})).into_response()
}

async fn spawn_provider() -> Arc<Provider> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let key = crypto().generate_ecdsa_keypair(EcCurve::P256).unwrap();
    let public = crypto().ecdsa_public_key(&key);
    let provider = Arc::new(Provider {
        issuer,
        jwk: json!({"kty":"EC","crv":"P-256","kid":"fixture","use":"sig",
            "x": URL_SAFE_NO_PAD.encode(&public.x), "y": URL_SAFE_NO_PAD.encode(&public.y)}),
        key,
        next: Mutex::new(Next {
            email: "alice@example.com".into(),
            verified: true,
            wrong_nonce: false,
        }),
        codes: Mutex::new(Default::default()),
    });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(Arc::clone(&provider));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    provider
}

const MEMBERS: &str = r#"[{"email":"alice@example.com","scopes":["sandboxes","inventory"],
    "principal_id":"alice","team_id":"red"}]"#;

struct Cluster {
    base: String,
    sso: Arc<SsoLogin>,
    client: reqwest::Client,
}

async fn spawn_cluster(provider: &Provider) -> Cluster {
    let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::new());
    let control = ControlPlane::new(
        store,
        ControlConfig {
            api_key: None,
            api_keys: Vec::new(),
            access_audit: None,
            cluster_token: Some("cluster-token".into()),
            proxy_port: 3000,
            create_timeout: Duration::from_secs(5),
            identity_issuer: None,
        },
    );
    let sso = Arc::new(
        SsoLogin::discover(
            SsoConfig {
                issuer: provider.issuer.clone(),
                client_id: CLIENT_ID.into(),
                client_secret: Some(zeroize::Zeroizing::new(CLIENT_SECRET.into())),
                redirect_url: REDIRECT.into(),
                session_ttl: Duration::from_secs(3600),
                provider_ca_pem: None,
            },
            Members::from_json(MEMBERS).unwrap(),
            SessionKey::new(vec![9u8; 32]).unwrap(),
        )
        .await
        .unwrap(),
    );
    control.enable_sso(Arc::clone(&sso)).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = control::router(control);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Cluster {
        base,
        sso,
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
    }
}

/// The value of `name` among a response's `Set-Cookie`s.
fn set_cookie(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| {
            let (pair, attributes) = v.split_once(';').unwrap_or((v, ""));
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| {
                assert!(attributes.contains("Secure") && attributes.contains("HttpOnly"));
                value.to_string()
            })
        })
}

/// A browser's sign-in: to the control plane, to the provider, back.
/// Returns the callback's response.
async fn sign_in(
    cluster: &Cluster,
    tamper: impl Fn(&str, &str) -> (String, String),
) -> reqwest::Response {
    let login = cluster
        .client
        .get(format!("{}/auth/login?returnTo=/sandboxes", cluster.base))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 303);
    let pending = set_cookie(&login, "__Host-hm_login").expect("a login cookie");
    let to_provider = login.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    let back = cluster.client.get(&to_provider).send().await.unwrap();
    let callback = reqwest::Url::parse(back.headers()[header::LOCATION].to_str().unwrap()).unwrap();
    let query: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    let (state, cookie) = tamper(&query["state"], &pending);
    cluster
        .client
        .get(format!(
            "{}/auth/callback?code={}&state={state}",
            cluster.base, query["code"]
        ))
        .header(header::COOKIE, format!("__Host-hm_login={cookie}"))
        .send()
        .await
        .unwrap()
}

fn untouched(state: &str, cookie: &str) -> (String, String) {
    (state.to_string(), cookie.to_string())
}

#[tokio::test]
async fn a_member_signs_in_and_the_api_admits_the_session() {
    let provider = spawn_provider().await;
    let cluster = spawn_cluster(&provider).await;
    let api = |path: &str| cluster.client.get(format!("{}{path}", cluster.base));

    // With sign-on on, nobody is anonymous -- even with no API keys at all.
    assert_eq!(api("/sandboxes").send().await.unwrap().status(), 401);

    let callback = sign_in(&cluster, untouched).await;
    assert_eq!(callback.status(), 303);
    assert_eq!(callback.headers()[header::LOCATION], "/sandboxes");
    let session = set_cookie(&callback, "__Host-hm_session").expect("a session cookie");
    let cookie = format!("__Host-hm_session={session}");

    let who: Value = api("/auth/session")
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(who["email"], "alice@example.com");
    assert_eq!(who["teamID"], "red");
    assert_eq!(
        api("/sandboxes")
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // A change with the cookie needs this control plane's Origin; a bearer
    // token, which no browser attaches by itself, does not.
    let create = |auth: (&str, String)| {
        cluster
            .client
            .post(format!("{}/sandboxes", cluster.base))
            .header(auth.0, auth.1)
            .json(&json!({"templateID": "base"}))
    };
    let response = create(("cookie", cookie.clone())).send().await.unwrap();
    assert_eq!(response.status(), 403);
    let response = create(("cookie", cookie.clone()))
        .header(header::ORIGIN, "https://8080-sbx.cp.test")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        403,
        "a sandbox page's origin is not this one"
    );
    for response in [
        create(("cookie", cookie.clone()))
            .header(header::ORIGIN, "https://cp.test")
            .send()
            .await
            .unwrap(),
        create(("authorization", format!("Bearer {session}")))
            .send()
            .await
            .unwrap(),
    ] {
        // Past authentication: no node to create on, but not refused.
        assert!(
            ![401, 403].contains(&response.status().as_u16()),
            "{}",
            response.status()
        );
    }

    // Scopes still apply: this member has no volumes scope.
    assert_eq!(
        api("/volumes")
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );

    // Removed from the members, the session stops working at once.
    cluster.sso.replace_members("[{\"email\":\"someone@example.com\",\"scopes\":[\"sandboxes\"],\"principal_id\":\"x\"}]").unwrap();
    assert_eq!(
        api("/sandboxes")
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

#[tokio::test]
async fn a_sign_in_is_refused_for_anything_short_of_a_verified_member() {
    let provider = spawn_provider().await;
    let cluster = spawn_cluster(&provider).await;

    // Forged state, or a login cookie this control plane did not seal.
    let response = sign_in(&cluster, |_, cookie| ("forged".into(), cookie.into())).await;
    assert_eq!(response.status(), 400);
    let response = sign_in(&cluster, |state, cookie| {
        let mut bytes = cookie.as_bytes().to_vec();
        let last = bytes.len() - 2;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        (state.into(), String::from_utf8(bytes).unwrap())
    })
    .await;
    assert_eq!(response.status(), 400);

    // A provider answering with the wrong nonce, an unverified email, or
    // someone who is not a member.
    for (next, status) in [
        (
            Next {
                email: "alice@example.com".into(),
                verified: true,
                wrong_nonce: true,
            },
            401,
        ),
        (
            Next {
                email: "alice@example.com".into(),
                verified: false,
                wrong_nonce: false,
            },
            401,
        ),
        (
            Next {
                email: "carol@example.com".into(),
                verified: true,
                wrong_nonce: false,
            },
            403,
        ),
    ] {
        *provider.next.lock() = next;
        let response = sign_in(&cluster, untouched).await;
        assert_eq!(response.status(), status);
        assert!(set_cookie(&response, "__Host-hm_session").is_none());
    }

    // A session token made with another key is no session.
    let forged = SessionKey::new(vec![1u8; 32])
        .unwrap()
        .mint("alice@example.com", chrono::Utc::now().timestamp(), 600)
        .unwrap();
    let response = cluster
        .client
        .get(format!("{}/sandboxes", cluster.base))
        .header(header::AUTHORIZATION, format!("Bearer {forged}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn a_provider_naming_another_issuer_is_refused_at_startup() {
    let provider = spawn_provider().await;
    let result = SsoLogin::discover(
        SsoConfig {
            issuer: format!("{}/", provider.issuer),
            client_id: CLIENT_ID.into(),
            client_secret: None,
            redirect_url: REDIRECT.into(),
            session_ttl: Duration::from_secs(60),
            provider_ca_pem: None,
        },
        Members::default(),
        SessionKey::new(vec![9u8; 32]).unwrap(),
    )
    .await;
    assert!(result.unwrap_err().contains("names issuer"));
    let result = SsoLogin::discover(
        SsoConfig {
            issuer: "http://idp.example".into(),
            client_id: CLIENT_ID.into(),
            client_secret: None,
            redirect_url: REDIRECT.into(),
            session_ttl: Duration::from_secs(60),
            provider_ca_pem: None,
        },
        Members::default(),
        SessionKey::new(vec![9u8; 32]).unwrap(),
    )
    .await;
    assert!(result.unwrap_err().contains("must be HTTPS"));
}
