//! This node's shields: rules an operator lays over running sandboxes'
//! networks without restarting them, signed, approved, expiring and measured.
//! See `hv2-shield` for what a shield is, and docs/SHIELDS.md for the whole.
//!
//! A sandbox here is a microVM, so what a shield refuses here is network:
//! every sandbox's gateway asks [`GatewayShield`] on each DNS question and
//! connection. Its system calls are made to its own kernel, which a shield
//! does not reach; the process sandboxes elsewhere take syscall rules.
//!
//! # Who may do what
//!
//! Every route is behind the node's cluster token. Beyond that:
//!
//! - anyone holding it may *propose* a shield, which does nothing;
//! - a shield is *loaded*, in monitor mode, only signed by a key in the
//!   node's keyring (`<shield-dir>/keyring.json`, which only the operator
//!   writes);
//! - it is *enforced*, returned to monitor, or *removed* only by a signed
//!   directive, and with `--shield-two-keys` the key enforcing it must be
//!   another than the one that signed it;
//! - it leaves force on its own when it expires.
//!
//! Each change is an event in a hash-chained log whose register this node
//! signs, over a verifier's nonce, with its own key: an attestation of what
//! has been in force here.

use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hv2_net::gateway::{Attempt, AttemptKind, ConnectionShield};
use hv2_shield::registry::{Policy, Saved};
use hv2_shield::{
    from_hex, now_secs, Connection, Event, Keyring, Live, Protocol, Registry, SignedDirective,
    SignedShield, SigningKey,
};
use serde::Deserialize;
use serde_json::json;

use crate::{api_error, AppState};

const KEYRING: &str = "keyring.json";
const NODE_KEY: &str = "node.key";
const REGISTRY: &str = "registry.json";

/// The node's shields, and what keeps them.
pub(crate) struct NodeShields {
    pub(crate) live: Arc<Live>,
    /// Signs this node's attestations.
    key: SigningKey,
    /// Where the keyring, key and registry are kept; `None` keeps the
    /// registry in memory, with an empty keyring, so nothing can be loaded.
    dir: Option<PathBuf>,
    /// One save at a time, so an older registry never overwrites a newer.
    saving: parking_lot::Mutex<()>,
}

/// What `--shield-*` options set.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShieldOptions {
    pub(crate) dir: Option<PathBuf>,
    pub(crate) policy: Policy,
}

impl NodeShields {
    /// Open this node's shields: read its keyring and key, and restore its
    /// registry, checking the log and every signature again.
    ///
    /// # Errors
    ///
    /// If a file is unreadable or does not check out. A registry that does
    /// not is not discarded: the node refuses to start until an operator
    /// moves it away, since discarding it would discard what is in force.
    pub(crate) fn open(options: &ShieldOptions) -> Result<Self, String> {
        let Some(dir) = options.dir.clone() else {
            return Ok(Self {
                live: Arc::new(Live::new(Registry::new(options.policy), Keyring::default())),
                key: SigningKey::generate().map_err(|e| format!("shield key: {e}"))?,
                dir: None,
                saving: parking_lot::Mutex::new(()),
            });
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let keyring = read_keyring(&dir)?;
        let key = node_key(&dir)?;
        let now = now_secs();
        let (registry, events) = match std::fs::read_to_string(dir.join(REGISTRY)) {
            Ok(text) => {
                let saved: Saved = serde_json::from_str(&text)
                    .map_err(|e| format!("{}: {e}", dir.join(REGISTRY).display()))?;
                Registry::restore(saved, options.policy, &keyring, now).map_err(|e| {
                    format!(
                        "{}: {e}; move it away to start with no shields",
                        dir.join(REGISTRY).display()
                    )
                })?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (Registry::new(options.policy), Vec::new())
            }
            Err(e) => return Err(format!("{}: {e}", dir.join(REGISTRY).display())),
        };
        let shields = Self {
            live: Arc::new(Live::new(registry, keyring)),
            key,
            dir: Some(dir),
            saving: parking_lot::Mutex::new(()),
        };
        if !events.is_empty() {
            shields.save()?;
        }
        Ok(shields)
    }

    /// Write the registry, whole, in place of the last.
    fn save(&self) -> Result<(), String> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let _one = self.saving.lock();
        let saved = self.live.registry().save();
        let text = serde_json::to_string(&saved).map_err(|e| e.to_string())?;
        write_private(&dir.join(REGISTRY), text.as_bytes())
    }

    /// Retire what has expired. Run every few seconds.
    pub(crate) fn tick(&self) {
        let events = self.live.update(|registry, _| registry.expire(now_secs()));
        if !events.is_empty() {
            if let Err(e) = self.save() {
                tracing::error!("shields: saving after expiry: {e}");
            }
        }
    }
}

fn read_keyring(dir: &FsPath) -> Result<Keyring, String> {
    let path = dir.join(KEYRING);
    match std::fs::read_to_string(&path) {
        Ok(text) => Keyring::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Keyring::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// This node's attestation key: read, or made once and kept.
fn node_key(dir: &FsPath) -> Result<SigningKey, String> {
    let path = dir.join(NODE_KEY);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            SigningKey::from_seed_hex(text.trim()).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = SigningKey::generate().map_err(|e| format!("shield key: {e}"))?;
            write_private(&path, key.seed_hex().as_bytes())?;
            Ok(key)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write a file only its owner can read, by renaming a whole one into place.
fn write_private(path: &FsPath, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("partial");
    let fail = |e: std::io::Error| format!("{}: {e}", partial.display());
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&partial).map_err(fail)?;
        file.write_all(bytes).map_err(fail)?;
        file.sync_all().map_err(fail)?;
    }
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a sandbox's gateway asks: the node's shields, for that sandbox.
pub(crate) struct GatewayShield {
    pub(crate) live: Arc<Live>,
    pub(crate) sandbox: String,
}

impl ConnectionShield for GatewayShield {
    fn refuses(&self, attempt: &Attempt<'_>) -> Option<String> {
        let protocol = match attempt.kind {
            AttemptKind::Tcp => Protocol::Tcp,
            AttemptKind::Udp => Protocol::Udp,
            // A rule for either refuses the question that would lead to it.
            AttemptKind::Dns => Protocol::Any,
        };
        let connection = Connection {
            address: attempt.address,
            port: attempt.port,
            name: attempt.name,
            protocol,
        };
        self.live.connection(&self.sandbox, &connection, now_secs())
    }

    fn wants_names(&self) -> bool {
        self.live.names_matter(&self.sandbox, now_secs())
    }
}

/// The shield routes, for the node's operator alone.
pub(crate) fn router() -> axum::Router<Arc<AppState>> {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/shields", get(list).post(load))
        .route("/shields/proposals", post(propose))
        .route("/shields/proposals/{digest}", delete(withdraw))
        .route("/shields/directives", post(direct))
        .route("/shields/keyring", post(reload_keyring))
        .route("/shields/hits", get(hits))
        .route("/shields/events", get(events))
        .route("/shields/attestation", post(attest))
        .route_layer(axum::middleware::from_fn(operator_only))
}

/// Shields cover every sandbox on the node, and what they record names other
/// tenants' sandboxes and destinations: a request made for a team, which the
/// control plane marks with the team header, does not reach them.
async fn operator_only(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if request
        .headers()
        .contains_key(hv2_cluster::ownership::TEAM_HEADER)
    {
        return api_error(
            StatusCode::FORBIDDEN,
            "shields are the node operator's: a team's request cannot read or change them",
        );
    }
    next.run(request).await
}

fn shields(state: &AppState) -> &NodeShields {
    &state.shields
}

fn refused(e: hv2_shield::ShieldError) -> Response {
    let status = match &e {
        hv2_shield::ShieldError::NotFound(_) => StatusCode::NOT_FOUND,
        hv2_shield::ShieldError::Signature(_) => StatusCode::FORBIDDEN,
        hv2_shield::ShieldError::State(_) => StatusCode::CONFLICT,
        hv2_shield::ShieldError::Invalid(_) | hv2_shield::ShieldError::Key(_) => {
            StatusCode::BAD_REQUEST
        }
    };
    api_error(status, e)
}

/// After a change: save, and answer with the event.
fn changed(node: &NodeShields, event: Option<Event>) -> Response {
    if let Err(e) = node.save() {
        // In force in memory, and not on disk: say so rather than pretend.
        tracing::error!("shields: saving: {e}");
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("applied, but not saved, so a restart would undo it: {e}"),
        );
    }
    Json(json!({ "event": event })).into_response()
}

/// `GET /shields`.
pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Response {
    let node = shields(&state);
    let registry = node.live.registry();
    let entries: Vec<_> = registry.entries().cloned().collect();
    let policy = registry.policy();
    let register = hv2_shield::to_hex(&registry.log().register());
    drop(registry);
    Json(json!({
        "nodeKey": node.key.public_key().id(),
        "nodePublicKey": node.key.public_key().to_hex(),
        "register": register,
        "persistent": node.dir.is_some(),
        "policy": {
            "minMonitorSecs": policy.min_monitor_secs,
            "distinctEnforcer": policy.distinct_enforcer,
        },
        "shields": entries,
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Proposal {
    /// The shield's JSON, as a string: its digest is of these bytes.
    spec: String,
    /// Who proposes it, as they describe themselves. Not checked.
    #[serde(default)]
    proposed_by: Option<String>,
}

/// `POST /shields/proposals` -- a shield for an operator to review. It does
/// nothing until one signs and loads it.
pub(crate) async fn propose(
    State(state): State<Arc<AppState>>,
    Json(proposal): Json<Proposal>,
) -> Response {
    let node = shields(&state);
    let by = proposal
        .proposed_by
        .unwrap_or_else(|| "unnamed".to_string());
    let by: String = by.chars().take(128).collect();
    let result = node
        .live
        .update(|registry, _| registry.propose(&proposal.spec, &by, now_secs()).cloned());
    match result {
        Ok(entry) => match node.save() {
            Ok(()) => (StatusCode::CREATED, Json(entry)).into_response(),
            Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(e) => refused(e),
    }
}

/// `DELETE /shields/proposals/{digest}` -- withdraw a proposal. A loaded
/// shield is removed only by a signed directive.
pub(crate) async fn withdraw(
    State(state): State<Arc<AppState>>,
    Path(digest): Path<String>,
) -> Response {
    let node = shields(&state);
    let result = node
        .live
        .update(|registry, _| match registry.get(&digest).map(|e| e.state) {
            Some(hv2_shield::State::Proposed) => registry.remove(&digest, "", now_secs()),
            Some(_) => Err(hv2_shield::ShieldError::State(
                "a loaded shield is removed by a signed directive".into(),
            )),
            None => Err(hv2_shield::ShieldError::NotFound(format!(
                "no shield {digest}"
            ))),
        });
    match result {
        Ok(_) => match node.save() {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        Err(e) => refused(e),
    }
}

/// `POST /shields` -- load a signed shield, in monitor mode.
pub(crate) async fn load(
    State(state): State<Arc<AppState>>,
    Json(signed): Json<SignedShield>,
) -> Response {
    let node = shields(&state);
    let result = node
        .live
        .update(|registry, keyring| registry.load(&signed, keyring, &signed.signer, now_secs()));
    match result {
        Ok(event) => changed(node, Some(event)),
        Err(e) => refused(e),
    }
}

/// `POST /shields/directives` -- enforce, monitor or remove a loaded shield.
pub(crate) async fn direct(
    State(state): State<Arc<AppState>>,
    Json(signed): Json<SignedDirective>,
) -> Response {
    let node = shields(&state);
    let result = node
        .live
        .update(|registry, keyring| registry.apply(&signed, keyring, now_secs()));
    match result {
        Ok(event) => changed(node, event),
        Err(e) => refused(e),
    }
}

/// `POST /shields/keyring` -- read the keyring file again, and retire
/// whatever it no longer vouches for.
pub(crate) async fn reload_keyring(State(state): State<Arc<AppState>>) -> Response {
    let node = shields(&state);
    let Some(dir) = &node.dir else {
        return api_error(
            StatusCode::CONFLICT,
            "this node keeps no shield directory (--shield-dir), so it has no keyring file",
        );
    };
    let keyring = match read_keyring(dir) {
        Ok(keyring) => keyring,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    let signers = keyring.signers().count();
    let events = node.live.set_keyring(keyring, now_secs());
    if let Err(e) = node.save() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "signers": signers, "retired": events })).into_response()
}

/// `GET /shields/hits` -- what shields matched: refused when enforced,
/// recorded only when monitored.
pub(crate) async fn hits(State(state): State<Arc<AppState>>) -> Response {
    Json(shields(&state).live.hits()).into_response()
}

/// `GET /shields/events` -- the measured log, to replay against an
/// attestation's register.
pub(crate) async fn events(State(state): State<Arc<AppState>>) -> Response {
    let registry = shields(&state).live.registry();
    Json(json!({
        "register": hv2_shield::to_hex(&registry.log().register()),
        "events": registry.log().events(),
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttestRequest {
    /// The verifier's nonce, in hex: 16 to 64 bytes.
    nonce: String,
}

/// `POST /shields/attestation` -- this node's signed register, over the
/// caller's nonce.
pub(crate) async fn attest(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AttestRequest>,
) -> Response {
    let Some(nonce) = from_hex(&request.nonce) else {
        return api_error(StatusCode::BAD_REQUEST, "the nonce is hex");
    };
    let node = shields(&state);
    let result = node
        .live
        .registry()
        .log()
        .attest(&node.key, &nonce, now_secs());
    match result {
        Ok(attestation) => Json(attestation).into_response(),
        Err(e) => refused(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(dir: &FsPath) -> ShieldOptions {
        ShieldOptions {
            dir: Some(dir.to_path_buf()),
            policy: Policy::default(),
        }
    }

    fn keyring_file(dir: &FsPath, key: &SigningKey) {
        let file = hv2_shield::KeyringFile {
            keys: vec![hv2_shield::sign::KeyringEntry {
                public_key: key.public_key().to_hex(),
                label: "operator".into(),
            }],
            ..Default::default()
        };
        std::fs::write(dir.join(KEYRING), serde_json::to_string(&file).unwrap()).unwrap();
    }

    const SPEC: &str =
        r#"{"name":"block-c2","network":[{"name":"*.c2.example"}],"expiresAt":4102444800}"#;

    #[test]
    fn shields_outlive_a_restart_and_an_edited_registry_stops_the_node() {
        let dir = tempfile::tempdir().unwrap();
        let operator = SigningKey::generate().unwrap();
        keyring_file(dir.path(), &operator);

        let node = NodeShields::open(&options(dir.path())).unwrap();
        let attesting = node.key.public_key().id();
        let signed = SignedShield::sign(SPEC, &operator).unwrap();
        node.live
            .update(|r, k| r.load(&signed, k, &signed.signer, now_secs()))
            .unwrap();
        node.save().unwrap();
        drop(node);

        let node = NodeShields::open(&options(dir.path())).unwrap();
        assert_eq!(node.key.public_key().id(), attesting, "the same key");
        assert!(!node.live.is_empty(), "still loaded");
        let shield = GatewayShield {
            live: node.live.clone(),
            sandbox: "sb".into(),
        };
        assert!(shield.wants_names());
        let attempt = Attempt {
            kind: AttemptKind::Dns,
            address: None,
            port: 0,
            name: Some("x.c2.example"),
        };
        assert_eq!(shield.refuses(&attempt), None, "monitored: recorded only");
        assert_eq!(node.live.hits().recorded, 1);
        drop(node);

        // A shield put in force by editing the file stops the node: the
        // log says it is monitored.
        let path = dir.path().join(REGISTRY);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"monitor\""));
        std::fs::write(&path, text.replace("\"monitor\"", "\"enforce\"")).unwrap();
        assert!(NodeShields::open(&options(dir.path())).is_err());

        // One whose log no longer replays stops the node.
        std::fs::write(&path, text.replace("\"seq\":1", "\"seq\":2")).unwrap();
        assert!(NodeShields::open(&options(dir.path())).is_err());
    }

    #[tokio::test]
    async fn a_team_s_request_does_not_reach_the_shields() {
        let app: axum::Router = axum::Router::new()
            .route("/shields", axum::routing::get(|| async { "in" }))
            .route_layer(axum::middleware::from_fn(operator_only));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/shields", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let operator = client.get(&url).send().await.unwrap();
        assert_eq!(operator.status(), StatusCode::OK);
        let team = client
            .get(&url)
            .header(hv2_cluster::ownership::TEAM_HEADER, "team-a")
            .send()
            .await
            .unwrap();
        assert_eq!(team.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn without_a_directory_nothing_can_be_loaded() {
        let node = NodeShields::open(&ShieldOptions::default()).unwrap();
        let operator = SigningKey::generate().unwrap();
        let signed = SignedShield::sign(SPEC, &operator).unwrap();
        assert!(node
            .live
            .update(|r, k| r.load(&signed, k, &signed.signer, now_secs()))
            .is_err());
    }
}
