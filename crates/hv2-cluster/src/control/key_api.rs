//! Managed API keys: made, listed and revoked through the control plane
//! instead of by editing the policy file.
//!
//! * `POST /keys` makes one and shows its secret once.
//! * `GET /keys` lists keys, without secrets or digests.
//! * `DELETE /keys/{id}` revokes one; it stops working on the next request.
//! * `GET /teams/{team}/members` lists a team's principals, from the policy
//!   file and from managed keys.
//!
//! Managed keys live in the [`ClusterStore`], never in the policy file, which
//! stays the operator's and read-only to this API. A request whose key no
//! policy names is looked up in the store by its SHA-256, on that request and
//! never cached, so a revocation takes effect on every control plane at once.
//! A key found there acts under the same [`ApiKeyPolicy`] a file entry with the
//! same fields would.
//!
//! # The store is not trusted
//!
//! Nodes share the store and can write to it, so a record in it proves
//! nothing by being there. Each is sealed with an HMAC under a secret only
//! control planes hold (`--managed-key-secret-file`), covering every field
//! that decides what the key may do. A record whose seal does not verify --
//! forged, altered, or sealed under another secret -- never authenticates
//! (the request is refused with a 503, as for any record that cannot be
//! trusted), is never listed, never counted against a limit, and can be
//! revoked only by an administrator. With no secret configured, managed keys
//! are off: `/keys` answers 503 and no key is looked up in the store.
//!
//! # Who may do what
//!
//! An administrator manages any key and any team. A team-admin manages its own
//! team's keys and nothing that would raise anyone above it: it cannot choose
//! another team, cannot make a team-admin or hold `admin`, cannot grant a
//! scope it lacks, and cannot make a key outlive itself. Nobody else may use
//! these routes. A key a team-admin made keeps working if its creator's own
//! key is revoked or expires: it is the team's, not the creator's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::{api_error, ControlPlane};
use crate::keys::{ApiKeyPolicy, ApiRole, ApiScope, KeyRequest, ManagedKey, ManagedKeySecret};
use crate::model::ClusterEvent;
use crate::ownership::{OwnerId, TeamId};
use std::sync::Arc;

/// The shortest a managed key may live, in seconds.
pub const MIN_KEY_TTL_SECS: u64 = 60;
/// The longest a managed key may live unless an operator configures otherwise.
pub const DEFAULT_MAX_KEY_TTL_SECS: u64 = 90 * 24 * 60 * 60;
/// The most a configured maximum may be: ten years.
pub const MAX_CONFIGURABLE_KEY_TTL_SECS: u64 = 3650 * 24 * 60 * 60;
/// Unexpired managed keys one team (or, for administrators' keys that name
/// no team, all of them together) may hold at most, unless configured.
pub const DEFAULT_MANAGED_KEY_LIMIT: usize = 1000;
/// What a secret begins with, so a leaked one is recognisable.
pub const KEY_PREFIX: &str = "hm_";

/// How long one store call may take before the request fails closed.
const STORE_TIMEOUT: Duration = Duration::from_secs(5);

/// Who is calling, as authentication decided: set by the middleware, never
/// by the client.
#[derive(Clone)]
pub(super) struct KeyCaller {
    administrator: bool,
    team_admin: bool,
    team: Option<TeamId>,
    scopes: BTreeSet<ApiScope>,
    expires_at: Option<i64>,
    created_by: String,
}

impl KeyCaller {
    /// `policy` is the key's (or the session's) policy, absent for the legacy
    /// admin key. `created_by` names the caller in records and events: its
    /// principal when it has one, otherwise its kind and key id, which are
    /// the labels the access log already carries.
    pub(super) fn new(
        kind: &str,
        key_id: &str,
        principal: Option<&OwnerId>,
        administrator: bool,
        team_admin: bool,
        policy: Option<&ApiKeyPolicy>,
    ) -> Self {
        let created_by = match (principal, kind) {
            (Some(principal), _) => principal.as_str().to_owned(),
            (None, "legacy_admin") => "legacy-admin".to_owned(),
            (None, _) => format!("{kind}:{key_id}"),
        };
        Self {
            administrator,
            team_admin,
            team: policy.and_then(|policy| policy.team_id().cloned()),
            scopes: policy.map(|p| p.scopes().clone()).unwrap_or_default(),
            expires_at: policy.map(ApiKeyPolicy::expires_at),
            created_by,
        }
    }

    // Handlers answer with a Response; see events.rs for the same allowance.
    #[allow(clippy::result_large_err)]
    fn authority(&self) -> Result<Authority, Response> {
        if self.administrator {
            return Ok(Authority::Administrator);
        }
        match (&self.team, self.team_admin) {
            (Some(team), true) => Ok(Authority::Team(team.clone())),
            _ => Err(api_error(
                StatusCode::FORBIDDEN,
                "an administrator or team-admin credential is required",
            )),
        }
    }
}

enum Authority {
    Administrator,
    Team(TeamId),
}

/// What the store said about a key no policy names.
pub(super) enum Managed {
    /// Not a managed key either: the caller decides, as it always did.
    Unknown,
    Valid(Box<ApiKeyPolicy>, String),
    Expired,
    /// The store failed or held a record that cannot be trusted. The request
    /// is refused, and is not mistaken for an unknown key.
    Unavailable,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Look the presented key up in the store, on this request, uncached.
pub(super) async fn authenticate(control: &ControlPlane, sent: &[u8], now: i64) -> Managed {
    // Without the secret no record can be told from a forgery: managed keys
    // are off, and the key is simply not one.
    let Some(secret) = control.managed_key_secret() else {
        return Managed::Unknown;
    };
    let digest: [u8; 32] = Sha256::digest(sent).into();
    let found = tokio::time::timeout(
        STORE_TIMEOUT,
        control.store.managed_key_by_sha256(&hex(&digest)),
    )
    .await;
    let record = match found {
        Ok(Ok(None)) => return Managed::Unknown,
        Ok(Ok(Some(record))) => record,
        Ok(Err(error)) => {
            tracing::warn!(%error, "managed key lookup failed; request refused");
            return Managed::Unavailable;
        }
        Err(_) => {
            tracing::warn!("managed key lookup timed out; request refused");
            return Managed::Unavailable;
        }
    };
    if !secret.verifies(&record) {
        tracing::warn!(key = %record.id.escape_debug(),
            "a managed key record is not sealed by this control plane; request refused");
        return Managed::Unavailable;
    }
    let policy = match ApiKeyPolicy::from_managed(&record) {
        Ok(policy) if policy.has_digest(&digest) => policy,
        _ => {
            tracing::warn!(key = %record.id, "a managed key record is not trustworthy; request refused");
            return Managed::Unavailable;
        }
    };
    if policy.matches(&digest, now) {
        Managed::Valid(Box::new(policy), record.id)
    } else {
        Managed::Expired
    }
}

impl ControlPlane {
    /// Turn managed API keys on, sealing and checking them with `secret`
    /// (at least 32 bytes, used as given). Every control plane that shares a
    /// store must be given the same one; nodes must not have it. Without it
    /// `/keys` answers 503 and no managed key authenticates.
    ///
    /// # Errors
    /// A secret shorter than 32 bytes.
    pub fn set_managed_key_secret(&self, secret: Vec<u8>) -> Result<(), String> {
        *self.managed_key_secret.write() = Some(Arc::new(ManagedKeySecret::new(secret)?));
        Ok(())
    }

    pub(super) fn managed_key_secret(&self) -> Option<Arc<ManagedKeySecret>> {
        self.managed_key_secret.read().clone()
    }

    /// Set how long a managed key may live at most; the default is 90 days.
    ///
    /// # Errors
    /// Reject less than a minute (the shortest a key may live) or more than ten years.
    pub fn set_managed_key_max_ttl(&self, ttl: Duration) -> Result<(), String> {
        let secs = ttl.as_secs();
        if !(MIN_KEY_TTL_SECS..=MAX_CONFIGURABLE_KEY_TTL_SECS).contains(&secs) {
            return Err(format!(
                "the managed key lifetime limit must be {MIN_KEY_TTL_SECS} to {MAX_CONFIGURABLE_KEY_TTL_SECS} seconds"
            ));
        }
        self.managed_key_max_ttl_secs.store(secs, Ordering::Relaxed);
        Ok(())
    }

    /// Set how many unexpired managed keys one team may hold (administrators'
    /// keys that name no team count together as one); the default is 1000.
    pub fn set_managed_key_limit(&self, limit: usize) {
        self.managed_key_limit.store(limit, Ordering::Relaxed);
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn role_name(role: ApiRole) -> &'static str {
    match role {
        ApiRole::Operator => "operator",
        ApiRole::Observer => "observer",
        ApiRole::TeamAdmin => "team-admin",
    }
}

/// Random bytes from the OS CSPRNG through the FIPS-capable module the rest
/// of the control plane uses, URL-safe base64 without padding.
fn random_url_safe(bytes: usize) -> Result<String, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mut buffer = zeroize::Zeroizing::new(vec![0u8; bytes]);
    hv2_core::crypto::FipsCrypto::new(hv2_core::crypto::FipsMode::Enabled)
        .and_then(|crypto| crypto.random_bytes(&mut buffer))
        .map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(&*buffer))
}

/// What a listing says about a key: everything but the secret and its digest.
fn metadata(key: &ManagedKey, now: i64) -> Value {
    json!({
        "id": key.id,
        "label": key.label,
        "scopes": key.scopes,
        "role": key.role,
        "teamId": key.team_id,
        "principalId": key.principal,
        "createdBy": key.created_by,
        "createdAt": key.created_at,
        "expiresAt": key.expires_at,
        "expired": key.expires_at <= now,
    })
}

/// A key id is `mk_` and 16 URL-safe characters; anything else cannot name one.
fn plausible_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn unavailable(what: &str, error: &dyn std::fmt::Display) -> Response {
    tracing::warn!(%error, "{what}");
    api_error(StatusCode::SERVICE_UNAVAILABLE, "key store unavailable")
}

/// The answer when no secret is configured: nothing here can be trusted, so
/// nothing here is offered.
fn disabled() -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "managed API keys are disabled: this control plane has no managed key secret \
         (--managed-key-secret-file)",
    )
}

/// What the store holds, split by whether this control plane sealed it.
struct Held {
    /// Sealed by this control plane's secret.
    trusted: Vec<ManagedKey>,
    /// Anything else. Never acted on; an administrator may revoke it.
    untrusted: Vec<ManagedKey>,
}

/// Every managed key, with the ones this control plane did not seal set aside.
// Handlers answer with a Response; see events.rs for the same allowance.
#[allow(clippy::result_large_err)]
async fn read_managed(control: &ControlPlane, secret: &ManagedKeySecret) -> Result<Held, Response> {
    let keys = match tokio::time::timeout(STORE_TIMEOUT, control.store.managed_keys()).await {
        Ok(Ok(keys)) => keys,
        Ok(Err(error)) => return Err(unavailable("could not read managed keys", &error)),
        Err(_) => return Err(unavailable("reading managed keys timed out", &"timeout")),
    };
    let (trusted, untrusted): (Vec<_>, Vec<_>) =
        keys.into_iter().partition(|key| secret.verifies(key));
    if !untrusted.is_empty() {
        tracing::warn!(
            count = untrusted.len(),
            "managed key records not sealed by this control plane were ignored"
        );
    }
    Ok(Held { trusted, untrusted })
}

async fn announce(control: &ControlPlane, kind: &str, key: &ManagedKey, by: &str) {
    // The id and the creator, never the secret, the digest or the label.
    let mut event = ClusterEvent::new(kind, "control-plane", None)
        .with_detail(format!("id={} by={by}", key.id));
    event.team_id = key.team_id.clone();
    if let Err(error) = control.store.publish(&event).await {
        tracing::warn!(%error, key = %key.id, "could not publish a key event");
    }
}

/// Whether a generated secret is one the operator already configured: a
/// policy-file digest or the legacy admin key. Not reachable with 256 random
/// bits; checked because the cost of being wrong is a key that authenticates
/// as someone else.
fn collides_with_configuration(control: &ControlPlane, secret: &str, digest: &[u8; 32]) -> bool {
    let in_file = control
        .api_keys
        .read()
        .iter()
        .any(|policy| policy.has_digest(digest));
    in_file
        || control
            .config
            .api_key
            .as_deref()
            .is_some_and(|admin| bool::from(admin.as_bytes().ct_eq(secret.as_bytes())))
}

/// Whether `principal` is carried by a policy-file or managed key that is
/// outside `team` or is an administrator's, expired keys included (an expired
/// key's principal may still own sandboxes).
fn principal_is_foreign(
    control: &ControlPlane,
    held: &[ManagedKey],
    principal: &OwnerId,
    team: &TeamId,
) -> bool {
    let in_file = control.api_keys.read().iter().any(|policy| {
        policy.principal_id() == Some(principal)
            && (policy.team_id() != Some(team) || policy.is_administrator())
    });
    // Signed-in members carry principals too; a team-admin may not mint a
    // key as one outside its team, or as an administrator.
    let in_members = control
        .sso()
        .is_some_and(|sso| sso.principal_is_foreign(principal, team));
    in_file
        || in_members
        || held.iter().any(|key| {
            key.principal.as_ref() == Some(principal)
                && (key.team_id.as_ref() != Some(team) || key.is_administrator())
        })
}

fn valid_label(label: &str) -> bool {
    let chars = label.chars().count();
    (1..=128).contains(&chars) && !label.chars().any(char::is_control)
}

pub(super) async fn create_key(
    State(control): State<Arc<ControlPlane>>,
    Extension(caller): Extension<KeyCaller>,
    body: Bytes,
) -> Response {
    let authority = match caller.authority() {
        Ok(authority) => authority,
        Err(response) => return response,
    };
    let Some(seal) = control.managed_key_secret() else {
        return disabled();
    };
    let request: KeyRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "label and expiresInSeconds are required; unknown fields and invalid values are refused",
            )
        }
    };
    if !valid_label(&request.label) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "label must be 1 to 128 characters without control characters",
        );
    }
    let max_ttl = control.managed_key_max_ttl_secs.load(Ordering::Relaxed);
    if !(MIN_KEY_TTL_SECS..=max_ttl).contains(&request.expires_in_seconds) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("expiresInSeconds must be {MIN_KEY_TTL_SECS} to {max_ttl}"),
        );
    }
    // A team-admin's team is its own: omitted is forced, another is refused.
    let team = match (&authority, &request.team_id) {
        (Authority::Administrator, team) => team.clone(),
        (Authority::Team(own), None) => Some(own.clone()),
        (Authority::Team(own), Some(team)) if team == own => Some(team.clone()),
        (Authority::Team(_), Some(_)) => {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator can only make keys for its own team",
            )
        }
    };
    let (scopes, role) = match request.grant(team.as_ref()) {
        Ok(grant) => grant,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message),
    };
    let created_at = now();
    let expires_at =
        created_at.saturating_add(i64::try_from(request.expires_in_seconds).unwrap_or(i64::MAX));
    if matches!(authority, Authority::Team(_)) {
        if role == ApiRole::TeamAdmin {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator cannot make a team administrator",
            );
        }
        if scopes.contains(&ApiScope::Admin) {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator cannot grant the admin scope",
            );
        }
        if !scopes.is_subset(&caller.scopes) {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator cannot grant a scope it does not hold",
            );
        }
        if !caller.expires_at.is_some_and(|own| expires_at <= own) {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator cannot make a key that outlives its own",
            );
        }
    }
    let administrator = role == ApiRole::Operator && scopes.contains(&ApiScope::Admin);
    // Without a team a key would be global, and the file's own rule is that
    // only an administrator may be.
    if team.is_none() && !administrator {
        return api_error(
            StatusCode::BAD_REQUEST,
            "teamId is required unless the key is an administrator's",
        );
    }

    let (id, secret) = match (random_url_safe(12), random_url_safe(32)) {
        (Ok(id), Ok(secret)) => (
            format!("mk_{id}"),
            zeroize::Zeroizing::new(format!("{KEY_PREFIX}{secret}")),
        ),
        _ => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "no randomness to make a key from",
            )
        }
    };
    let digest: [u8; 32] = Sha256::digest(secret.as_bytes()).into();
    let collides = collides_with_configuration(&control, &secret, &digest);
    if collides {
        // Not reachable with 256 random bits; checked because the cost of
        // being wrong is a key that authenticates as someone else.
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the generated key collides with a configured key; try again",
        );
    }
    // Principal: given, else a label that names the key; a team's keys always
    // have one, as the policy file requires of them.
    let principal = match request.principal_id {
        Some(principal) => Some(principal),
        None if team.is_some() => OwnerId::parse(&id).ok(),
        None => None,
    };
    let record = ManagedKey {
        id,
        sha256: hex(&digest),
        scopes,
        role,
        team_id: team,
        principal,
        label: request.label,
        created_by: caller.created_by.clone(),
        created_at,
        expires_at,
        mac: String::new(),
    };
    let record = match seal.seal(record) {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(%error, "could not seal a managed key");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "could not seal the key");
        }
    };

    let limit = control.managed_key_limit.load(Ordering::Relaxed);
    match read_managed(&control, &seal).await {
        Ok(Held { trusted: held, .. }) => {
            // Principals are global. A team-admin may name one its own team
            // already uses (a new key for a member), but never one a key
            // outside its team or an administrator's key carries: that would
            // be minting a key as someone it does not administer.
            if let (Authority::Team(own), Some(principal)) = (&authority, &record.principal) {
                if principal_is_foreign(&control, &held, principal, own) {
                    return api_error(
                        StatusCode::FORBIDDEN,
                        "that principal belongs to a key outside this team, or to an administrator",
                    );
                }
            }
            // Per team, so one team cannot use up the room of the others;
            // administrators' keys that name no team are one bucket of their
            // own. Records this control plane did not seal are not counted:
            // anyone who can write to the store could otherwise fill a team's
            // room with them.
            if held
                .iter()
                .filter(|key| key.expires_at > created_at && key.team_id == record.team_id)
                .count()
                >= limit
            {
                return api_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "too many managed keys for this team; delete some first",
                );
            }
        }
        Err(response) => return response,
    }
    match tokio::time::timeout(STORE_TIMEOUT, control.store.put_managed_key(&record)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return unavailable("could not store a managed key", &error),
        Err(_) => return unavailable("storing a managed key timed out", &"timeout"),
    }
    tracing::info!(target: "hv2_cluster::access", key = %record.id, by = %caller.created_by,
        team = record.team_id.as_ref().map(TeamId::as_str), "managed key created");
    announce(&control, "key-created", &record, &caller.created_by).await;

    let mut response = (
        StatusCode::CREATED,
        Json(json!({
            "id": record.id,
            "key": secret.as_str(),
            "label": record.label,
            "scopes": record.scopes,
            "role": record.role,
            "teamId": record.team_id,
            "principalId": record.principal,
            "expiresAt": record.expires_at,
            "createdAt": record.created_at,
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Whether a team administrator of `team` may see and revoke `key`: a key of
/// its team that is not an administrator's.
fn within(team: &TeamId, key: &ManagedKey) -> bool {
    key.team_id.as_ref() == Some(team) && !key.is_administrator()
}

pub(super) async fn list_keys(
    State(control): State<Arc<ControlPlane>>,
    Extension(caller): Extension<KeyCaller>,
) -> Response {
    let authority = match caller.authority() {
        Ok(authority) => authority,
        Err(response) => return response,
    };
    let Some(secret) = control.managed_key_secret() else {
        return disabled();
    };
    let keys = match read_managed(&control, &secret).await {
        Ok(held) => held.trusted,
        Err(response) => return response,
    };
    let at = now();
    let listed: Vec<Value> = keys
        .iter()
        .filter(|key| match &authority {
            Authority::Administrator => true,
            Authority::Team(team) => within(team, key),
        })
        .map(|key| metadata(key, at))
        .collect();
    Json(json!({ "keys": listed })).into_response()
}

pub(super) async fn delete_key(
    State(control): State<Arc<ControlPlane>>,
    Extension(caller): Extension<KeyCaller>,
    Path(id): Path<String>,
) -> Response {
    let authority = match caller.authority() {
        Ok(authority) => authority,
        Err(response) => return response,
    };
    // Policy-file keys have no id here and are never reachable: 404, as for
    // any id the store does not hold.
    let not_found = || api_error(StatusCode::NOT_FOUND, "no such managed key");
    if !plausible_id(&id) {
        return not_found();
    }
    let Some(secret) = control.managed_key_secret() else {
        return disabled();
    };
    let held = match read_managed(&control, &secret).await {
        Ok(held) => held,
        Err(response) => return response,
    };
    let key = held.trusted.into_iter().find(|key| key.id == id);
    match (&authority, &key) {
        // Another team's key, or an administrator's, does not exist to a
        // team-admin; nor does a record this control plane did not seal.
        (Authority::Team(team), Some(key)) if within(team, key) => {}
        (Authority::Team(_), _) => return not_found(),
        // An administrator may remove anything the store holds under this id,
        // a record that cannot be trusted or even read included: revoking is
        // always possible.
        (Authority::Administrator, _) => {}
    }
    let untrusted = held.untrusted.iter().find(|key| key.id == id);
    match tokio::time::timeout(STORE_TIMEOUT, control.store.remove_managed_key(&id)).await {
        Ok(Ok(true)) => {}
        // Someone else revoked it between the read and the removal.
        Ok(Ok(false)) => return not_found(),
        Ok(Err(error)) => return unavailable("could not remove a managed key", &error),
        Err(_) => return unavailable("removing a managed key timed out", &"timeout"),
    }
    tracing::info!(target: "hv2_cluster::access", key = %id, by = %caller.created_by,
        "managed key revoked");
    if let Some(key) = key.as_ref().or(untrusted) {
        announce(&control, "key-deleted", key, &caller.created_by).await;
    }
    Json(json!({ "deleted": id, "kind": "key" })).into_response()
}

pub(super) async fn team_members(
    State(control): State<Arc<ControlPlane>>,
    Extension(caller): Extension<KeyCaller>,
    Path(team): Path<String>,
) -> Response {
    let authority = match caller.authority() {
        Ok(authority) => authority,
        Err(response) => return response,
    };
    let Ok(team) = TeamId::parse(&team) else {
        return api_error(StatusCode::BAD_REQUEST, "invalid team ID");
    };
    if let Authority::Team(own) = &authority {
        if *own != team {
            return api_error(
                StatusCode::FORBIDDEN,
                "a team administrator can only list its own team's members",
            );
        }
    }
    let at = now();
    // principal -> (roles, active keys). Administrators are global, not members.
    let mut members: BTreeMap<String, (BTreeSet<&'static str>, usize)> = BTreeMap::new();
    {
        let policies = control.api_keys.read();
        for policy in policies.iter() {
            if policy.team_id() != Some(&team) || policy.expires_at() <= at {
                continue;
            }
            if let Some(principal) = policy.principal_id() {
                let entry = members.entry(principal.as_str().to_owned()).or_default();
                entry.0.insert(role_name(policy.role()));
                entry.1 += 1;
            }
        }
    }
    // With managed keys off there are none to list: the policy file's members
    // are all there are.
    let managed = match control.managed_key_secret() {
        Some(secret) => match read_managed(&control, &secret).await {
            Ok(held) => held.trusted,
            Err(response) => return response,
        },
        None => Vec::new(),
    };
    for key in managed
        .iter()
        .filter(|key| within(&team, key) && key.expires_at > at)
    {
        if let Some(principal) = &key.principal {
            let entry = members.entry(principal.as_str().to_owned()).or_default();
            entry.0.insert(role_name(key.role));
            entry.1 += 1;
        }
    }
    let members: Vec<Value> = members
        .into_iter()
        .map(|(principal, (roles, keys))| {
            json!({"principalId": principal, "roles": roles, "keyCount": keys})
        })
        .collect();
    Json(json!({ "teamId": team, "members": members })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlConfig;
    use crate::store::MemoryStore;

    fn plane(admin: Option<&str>, file_secret: &str) -> Arc<ControlPlane> {
        let policies = json!([{"sha256": hex(&Sha256::digest(file_secret.as_bytes())),
            "expires_at": 4_000_000_000_i64, "scopes": ["inventory"]}]);
        ControlPlane::new(
            Arc::new(MemoryStore::new()),
            ControlConfig {
                api_key: admin.map(str::to_owned),
                api_keys: ApiKeyPolicy::from_json(&policies.to_string()).unwrap(),
                access_audit: None,
                cluster_token: None,
                proxy_port: 0,
                create_timeout: Duration::from_secs(1),
                identity_issuer: None,
            },
        )
    }

    fn digest(secret: &str) -> [u8; 32] {
        Sha256::digest(secret.as_bytes()).into()
    }

    /// A secret that is a configured credential is refused: the policy
    /// file's (by digest) and the legacy admin's (by value).
    #[test]
    fn a_secret_that_matches_a_configured_key_collides() {
        let control = plane(Some("hm_the-admin"), "hm_the-file-key");
        for secret in ["hm_the-admin", "hm_the-file-key"] {
            assert!(
                collides_with_configuration(&control, secret, &digest(secret)),
                "{secret}"
            );
        }
        for secret in ["hm_other", "hm_the-admin ", "hm_the-file-key2"] {
            assert!(
                !collides_with_configuration(&control, secret, &digest(secret)),
                "{secret}"
            );
        }
        let no_admin = plane(None, "hm_the-file-key");
        assert!(!collides_with_configuration(
            &no_admin,
            "hm_the-admin",
            &digest("hm_the-admin")
        ));
    }

    #[test]
    fn secrets_are_long_random_and_url_safe() {
        let a = random_url_safe(32).unwrap();
        let b = random_url_safe(32).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43, "32 bytes, unpadded");
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert!(plausible_id(&format!(
            "mk_{}",
            random_url_safe(12).unwrap()
        )));
        for bad in ["", "a/b", "a b", "..", &"a".repeat(65)] {
            assert!(!plausible_id(bad), "{bad}");
        }
    }

    #[test]
    fn labels_are_bounded_and_printable() {
        assert!(valid_label("ci runner 4"));
        assert!(valid_label("\u{e9}t\u{e9}"));
        assert!(valid_label(&"x".repeat(128)));
        for bad in ["", "a\nb", "a\u{0}b", &"x".repeat(129)] {
            assert!(!valid_label(bad), "{bad:?}");
        }
    }
}
