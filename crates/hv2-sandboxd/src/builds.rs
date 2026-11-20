//! E2B's template build API -- what `Template.build()` in E2B's SDKs calls,
//! and so what `Template().from_dockerfile(...)` builds with, the SDK
//! having turned the Dockerfile into steps:
//!
//! - `POST /v3/templates` names a build;
//! - `GET /templates/{id}/files/{hash}` says whether a `COPY`'s files are
//!   here, and where to `PUT` them if not;
//! - `POST /v2/templates/{id}/builds/{buildID}` starts it, from an image or
//!   a template, with its steps (`RUN`, `COPY`, `ENV`, `WORKDIR`, `USER`)
//!   and an optional start command and readiness check;
//! - `GET /templates/{id}/builds/{buildID}/status` reports it, logs and all.
//!
//! A build runs in a sandbox: the base restored, each step run in it, and
//! then -- with the start command's process running and its readiness check
//! passed -- the sandbox is snapshotted into the template. Every sandbox
//! created from the template is restored from that snapshot, the started
//! process already up. The build's commands run in a microVM of their own,
//! never on the node, and no container runtime is involved.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::HeaderMap;
use serde_json::{json, Value};

use hv2_agent::AgentVM;
use hv2_guest_agent::{TemplateDefaults, TEMPLATE_DEFAULTS_PATH};

use super::{
    api_error, bring_up, build_from_image, caller_team, create_park, new_access_token, policy_from,
    reserve, sizes_of, snapshots, token_digest, valid_template_name, AppState, Arc, Deserialize,
    IntoResponse, Json, NetworkSpec, Path, Query, Response, Running, Sizes, State, StatusCode,
};
use crate::env_vars::{join_guest, Member, Unpack};
use crate::oci::Credentials;

/// The longest one `RUN` -- or a readiness check, all attempts -- may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const READY_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How often a running step's output is collected into the build's log.
const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// What one status answer carries, as E2B's does.
const LOGS_PER_ANSWER: usize = 100;
/// The most one `COPY`'s upload may hold.
const MAX_UPLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// The most uploads one node keeps, and the most bytes they hold together.
/// The namespace is node-wide (not per team -- a design decision, noted in the
/// audit's O8), so these bound what any caller can leave on the disk.
const MAX_STORED_UPLOADS: usize = 256;
const MAX_STORED_UPLOAD_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// How long an offered upload's token is good for: the SDK sends its `PUT`
/// straight after the link it was given.
const UPLOAD_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);
/// The most offered upload tokens a node keeps at once; the oldest goes first.
const MAX_UPLOAD_GRANTS: usize = 1024;

/// What an upload may be and what the node will hold: the per-upload cap, the
/// stored count and bytes, and whether the name must be the content's SHA-256.
#[derive(Debug, Clone, Copy)]
struct UploadLimits {
    one: u64,
    count: usize,
    bytes: u64,
    verify_hash: bool,
}

/// A refused upload and the status it is refused with.
#[derive(Debug)]
struct UploadError(StatusCode, String);

impl UploadError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
}

/// How many uploads `dir` holds, and how many bytes it holds in all (scratch
/// files of uploads in flight included).
fn stored_uploads(dir: &std::path::Path) -> (usize, u64) {
    let mut count = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // Not `entry.metadata()`: on Windows that is the directory
            // listing's cached size, which a file published by hard link can
            // show as 0, and the byte quota would then never bind.
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            bytes += meta.len();
            if !entry.file_name().to_string_lossy().starts_with('.') {
                count += 1;
            }
        }
    }
    (count, bytes)
}

/// 507 if the node holds as many uploads, or as many bytes, as it may.
fn check_quota(dir: &std::path::Path, limits: &UploadLimits) -> Result<(), UploadError> {
    let (count, bytes) = stored_uploads(dir);
    if count >= limits.count {
        return Err(UploadError(
            StatusCode::INSUFFICIENT_STORAGE,
            format!("this node already holds {count} uploads, the most it keeps"),
        ));
    }
    if bytes >= limits.bytes {
        return Err(UploadError(
            StatusCode::INSUFFICIENT_STORAGE,
            format!("this node's uploads already hold {bytes} bytes, the most it keeps"),
        ));
    }
    Ok(())
}

fn upload_limits(state: &AppState) -> UploadLimits {
    UploadLimits {
        one: MAX_UPLOAD_BYTES,
        count: MAX_STORED_UPLOADS,
        bytes: MAX_STORED_UPLOAD_BYTES,
        verify_hash: state.opts.verify_upload_hash,
    }
}
/// Per agent call, not per step.
const AGENT_TIMEOUT: Duration = Duration::from_secs(60);

/// One template build: waiting for its steps, building, then done.
pub(crate) struct Build {
    template: String,
    build_id: String,
    /// The scope that made it ([`scope_of`]): its `COPY`s read the uploads of
    /// that scope, and no other.
    scope: String,
    /// The size asked for, if any: E2B's `cpuCount` and `memoryMB`.
    sizes: Option<Sizes>,
    state: parking_lot::Mutex<Progress>,
}

#[derive(Default)]
struct Progress {
    status: &'static str,
    logs: Vec<Value>,
    reason: Option<Value>,
}

impl Build {
    fn log(&self, level: &str, step: Option<&str>, message: impl Into<String>) {
        let mut entry = json!({
            "timestamp": hv2_cluster::model::rfc3339(hv2_cluster::model::now_ms()),
            "level": level,
            "message": message.into(),
        });
        if let Some(step) = step {
            entry["step"] = json!(step);
        }
        self.state.lock().logs.push(entry);
    }

    fn set_status(&self, status: &'static str) {
        self.state.lock().status = status;
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RequestBuild {
    name: Option<String>,
    alias: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "cpuCount")]
    cpu_count: Option<u32>,
    #[serde(rename = "memoryMB")]
    memory_mb: Option<u64>,
}

/// `POST /v3/templates`: name a build, to be started once its files are
/// uploaded. A template of that name is rebuilt in place: sandboxes
/// created from it afterwards get the new build.
pub(crate) async fn request(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RequestBuild>,
) -> Response {
    let team = match caller_team(&state, &headers) {
        Ok(team) => team,
        Err(refused) => return refused,
    };
    let Some(requested) = req.name.or(req.alias) else {
        return api_error(StatusCode::BAD_REQUEST, "a build needs a name");
    };
    let name = snapshots::untagged(&requested).to_string();
    if !valid_template_name(&name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("template name {name:?}: letters, digits, - _ . only"),
        );
    }
    if state.initrds.read().contains_key(&name) {
        return api_error(
            StatusCode::CONFLICT,
            format!("{name} is a template built from an image; build under another name"),
        );
    }
    if state.opts.guest_kit.is_none() {
        return api_error(
            StatusCode::NOT_IMPLEMENTED,
            "this node builds no templates: start it with --guest-kit DIR",
        );
    }
    let sizes = if req.cpu_count.is_some() || req.memory_mb.is_some() {
        match Sizes::requested(req.cpu_count, req.memory_mb, Sizes::of(&state.opts)) {
            Ok(sizes) => Some(sizes),
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        }
    } else {
        None
    };
    let mut tags: Vec<String> = req.tags;
    if let Some((_, tag)) = requested.rsplit_once(':') {
        tags.insert(0, tag.to_string());
    }
    if tags.is_empty() {
        tags.push("default".into());
    }
    let build = Arc::new(Build {
        template: name.clone(),
        build_id: uuid::Uuid::new_v4().to_string(),
        scope: scope_of(team.as_ref()),
        sizes,
        state: parking_lot::Mutex::new(Progress {
            status: "waiting",
            ..Progress::default()
        }),
    });
    state
        .step_builds
        .lock()
        .insert(build.build_id.clone(), Arc::clone(&build));
    let names: Vec<String> = tags.iter().map(|t| format!("{name}:{t}")).collect();
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "templateID": name,
            "buildID": build.build_id,
            "public": false,
            "aliases": [name],
            "names": names,
            "tags": tags,
        })),
    )
        .into_response()
}

/// The scope a caller's uploads and builds are kept under: its team's, from
/// the control plane's [`TEAM_HEADER`](hv2_cluster::ownership::TEAM_HEADER),
/// or none for an administrator and for a node without teams. A `COPY` reads
/// only the uploads of its build's scope, so one scope cannot supply another's.
fn scope_of(team: Option<&hv2_cluster::ownership::TeamId>) -> String {
    team.map_or_else(String::new, |team| format!("team:{}", team.as_str()))
}

/// The file name an upload of `hash` under `scope` is stored as, beside every
/// other's: the first 16 hex digits of the scope's digest, then the hash. The
/// scope part is fixed-width, so two scopes' names cannot meet.
fn stored_name(scope: &str, hash: &str) -> String {
    let digest = token_digest(&format!("upload-scope\0{scope}"));
    format!("{}-{hash}.tar", &digest[..16])
}

/// Where a `COPY`'s upload for `hash` under `scope` is kept: in the store when
/// there is one, so a retried build on another node finds it.
fn upload_path(state: &AppState, scope: &str, hash: &str) -> Option<std::path::PathBuf> {
    let valid = !hash.is_empty()
        && hash.len() <= 128
        && hash
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let dir = match &state.store {
        Some(store) => store.dir.join("uploads"),
        None => state.suspend_dir.join("uploads"),
    };
    valid.then(|| dir.join(stored_name(scope, hash)))
}

/// What an offered upload may write: one scope's file of one hash, for one
/// template. Its scope comes from the offer, never from the request.
#[derive(Clone)]
pub(crate) struct UploadGrant {
    digest: String,
    scope: String,
    template: String,
    hash: String,
    expires: Instant,
}

/// The upload tokens offered by `GET .../files/{hash}` and not yet used. Only
/// their digests are kept. A token is compared with every live grant in
/// constant time, so the time taken does not say which one matched.
#[derive(Default)]
pub(crate) struct UploadGrants(parking_lot::Mutex<Vec<UploadGrant>>);

impl UploadGrants {
    /// A new token for `hash` of `template` under `scope`. An earlier offer
    /// for the same three is voided; offers for other scopes, templates or
    /// hashes are not.
    fn offer(&self, scope: &str, template: &str, hash: &str, now: Instant) -> String {
        let token = new_access_token();
        let mut grants = self.0.lock();
        grants.retain(|g| {
            g.expires > now && !(g.scope == scope && g.template == template && g.hash == hash)
        });
        if grants.len() >= MAX_UPLOAD_GRANTS {
            grants.remove(0);
        }
        grants.push(UploadGrant {
            digest: token_digest(&token),
            scope: scope.to_string(),
            template: template.to_string(),
            hash: hash.to_string(),
            expires: now + UPLOAD_TOKEN_TTL,
        });
        token
    }

    /// The grant `token` was offered as, if it is live and was offered for
    /// this template and hash. Any other answer is the same refusal.
    fn redeem(&self, token: &str, template: &str, hash: &str, now: Instant) -> Option<UploadGrant> {
        use subtle::ConstantTimeEq;
        let digest = token_digest(token);
        let grants = self.0.lock();
        let mut matched = None;
        for grant in grants.iter() {
            let same = bool::from(grant.digest.as_bytes().ct_eq(digest.as_bytes()));
            if same && grant.expires > now {
                matched = Some(grant);
            }
        }
        matched
            .filter(|g| g.template == template && g.hash == hash)
            .cloned()
    }

    /// Uses up a grant once its upload is stored.
    fn spend(&self, grant: &UploadGrant) {
        self.0.lock().retain(|g| g.digest != grant.digest);
    }
}

/// `GET /templates/{id}/files/{hash}`: whether a `COPY`'s files are here,
/// under the caller's scope; if not, a URL to `PUT` them to, good for that
/// one upload to that template and hash. The URL is this node's, as the
/// caller reached it -- through a control plane, the control plane's, which
/// passes the upload on.
pub(crate) async fn file_link(
    State(state): State<Arc<AppState>>,
    Path((template, hash)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let team = match caller_team(&state, &headers) {
        Ok(team) => team,
        Err(refused) => return refused,
    };
    let scope = scope_of(team.as_ref());
    let Some(path) = upload_path(&state, &scope, &hash) else {
        return api_error(StatusCode::BAD_REQUEST, format!("a files hash {hash:?}"));
    };
    if path.exists() {
        return (StatusCode::CREATED, Json(json!({ "present": true }))).into_response();
    }
    if let Some(dir) = path.parent() {
        if let Err(UploadError(status, message)) = check_quota(dir, &upload_limits(&state)) {
            return api_error(status, message);
        }
    }
    let template = snapshots::untagged(&template);
    let token = state.uploads.offer(&scope, template, &hash, Instant::now());
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let host = header("x-forwarded-host")
        .or_else(|| header("host"))
        .unwrap_or_else(|| format!("127.0.0.1:{}", state.opts.port));
    let scheme = header("x-forwarded-proto").unwrap_or_else(|| "http".into());
    (
        StatusCode::CREATED,
        Json(json!({
            "present": false,
            "url": format!("{scheme}://{host}/templates/{template}/files/{hash}?token={token}"),
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct UploadQuery {
    token: String,
}

/// `PUT /templates/{id}/files/{hash}?token=...`: a `COPY`'s files, as the
/// SDK packs them -- a tar, gzipped or not. Not behind the API key: the SDK
/// sends none with an upload, so the token from `file_link` stands in. The
/// token says where the bytes go (its scope, from the offer), and it is good
/// for its template and hash only.
pub(crate) async fn upload(
    State(state): State<Arc<AppState>>,
    Path((template, hash)): Path<(String, String)>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> Response {
    let template = snapshots::untagged(&template);
    let Some(grant) = state
        .uploads
        .redeem(&query.token, template, &hash, Instant::now())
    else {
        return api_error(StatusCode::FORBIDDEN, "not an upload this node offered");
    };
    let Some(path) = upload_path(&state, &grant.scope, &hash) else {
        return api_error(StatusCode::BAD_REQUEST, format!("a files hash {hash:?}"));
    };
    match receive(body, &path, &hash, &upload_limits(&state)).await {
        Ok(()) => {
            state.uploads.spend(&grant);
            StatusCode::OK.into_response()
        }
        Err(UploadError(status, message)) => api_error(status, message),
    }
}

/// Streamed to a scratch file beside `path`, then renamed onto it: a build
/// never reads half an upload. The bytes must hash (SHA-256, hex) to `hash`,
/// the name they are stored under, unless the limits say otherwise: another
/// caller's `COPY` of that name would otherwise run whatever was put there.
async fn receive(
    body: Body,
    path: &std::path::Path,
    hash: &str,
    limits: &UploadLimits,
) -> Result<(), UploadError> {
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;
    let dir = path
        .parent()
        .ok_or_else(|| UploadError::bad("an upload path with no directory"))?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| UploadError::bad(format!("{}: {e}", dir.display())))?;
    check_quota(dir, limits)?;
    let scratch = dir.join(format!(".{}", uuid::Uuid::new_v4().simple()));
    let mut file = tokio::fs::File::create(&scratch)
        .await
        .map_err(|e| UploadError::bad(format!("{}: {e}", scratch.display())))?;
    let mut stream = body.into_data_stream();
    let mut total = 0u64;
    let mut digest = sha2::Sha256::new();
    let written: Result<(), UploadError> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| UploadError::bad(e.to_string()))?;
            total += chunk.len() as u64;
            if total > limits.one {
                return Err(UploadError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("larger than {} bytes", limits.one),
                ));
            }
            digest.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| UploadError::bad(e.to_string()))?;
        }
        file.flush()
            .await
            .map_err(|e| UploadError::bad(e.to_string()))?;
        if limits.verify_hash {
            let actual: String = digest
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(hash) {
                return Err(UploadError(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("the upload's SHA-256 is {actual}, not the {hash:?} it was sent as"),
                ));
            }
        }
        Ok(())
    }
    .await;
    // Not `written.and(rename(..).await)`: that runs the rename whether or not
    // the upload was accepted, and published a refused upload under its name.
    let published = match written {
        // First writer wins: a hard link fails if `path` exists, atomically,
        // where a rename would replace it -- so nobody can swap in other bytes
        // under a name another caller already uploaded and is building from.
        // A loser is told 200, as `file_link` would have said "present".
        Ok(()) => match tokio::fs::hard_link(&scratch, path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(UploadError::bad(e.to_string())),
        },
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_file(&scratch).await;
    published
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Step {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(rename = "filesHash")]
    files_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryLogin {
    #[serde(rename = "type")]
    kind: String,
    username: Option<String>,
    password: Option<String>,
    #[serde(rename = "awsAccessKeyId")]
    aws_access_key_id: Option<String>,
    #[serde(rename = "awsSecretAccessKey")]
    aws_secret_access_key: Option<String>,
    #[serde(rename = "awsRegion")]
    aws_region: Option<String>,
    #[serde(rename = "serviceAccountJson")]
    service_account_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StartBuild {
    #[serde(rename = "fromImage")]
    from_image: Option<String>,
    #[serde(rename = "fromTemplate")]
    from_template: Option<String>,
    #[serde(rename = "fromImageRegistry")]
    registry: Option<RegistryLogin>,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(rename = "startCmd")]
    start_cmd: Option<String>,
    #[serde(rename = "readyCmd")]
    ready_cmd: Option<String>,
}

/// `POST /v2/templates/{id}/builds/{buildID}`: start the build.
pub(crate) async fn start(
    State(state): State<Arc<AppState>>,
    Path((_template, build_id)): Path<(String, String)>,
    Json(spec): Json<StartBuild>,
) -> Response {
    let Some(build) = state.step_builds.lock().get(&build_id).cloned() else {
        return api_error(StatusCode::NOT_FOUND, format!("no build {build_id}"));
    };
    {
        let mut progress = build.state.lock();
        if progress.status != "waiting" {
            return api_error(
                StatusCode::CONFLICT,
                format!("build {build_id} is {}", progress.status),
            );
        }
        progress.status = "building";
    }
    if spec.from_image.is_some() == spec.from_template.is_some() {
        build.set_status("waiting");
        return api_error(
            StatusCode::BAD_REQUEST,
            "exactly one of fromImage and fromTemplate",
        );
    }
    tokio::spawn(async move {
        let started = Instant::now();
        let outcome = run(&state, &build, spec).await;
        let mut progress = build.state.lock();
        match outcome {
            Ok(()) => {
                progress.status = "ready";
                tracing::info!(
                    "template {} built by steps in {:?}",
                    build.template,
                    started.elapsed()
                );
            }
            Err((step, message)) => {
                tracing::warn!("building template {}: {message}", build.template);
                let recent: Vec<Value> =
                    progress.logs.iter().rev().take(20).rev().cloned().collect();
                progress.reason = Some(json!({
                    "message": message,
                    "step": step,
                    "logEntries": recent,
                }));
                progress.status = "error";
            }
        }
    });
    StatusCode::ACCEPTED.into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatusQuery {
    #[serde(rename = "logsOffset")]
    logs_offset: Option<usize>,
}

/// `GET /templates/{id}/builds/{buildID}/status`.
pub(crate) async fn status(
    State(state): State<Arc<AppState>>,
    Path((_template, build_id)): Path<(String, String)>,
    Query(query): Query<StatusQuery>,
) -> Response {
    let Some(build) = state.step_builds.lock().get(&build_id).cloned() else {
        return api_error(StatusCode::NOT_FOUND, format!("no build {build_id}"));
    };
    let progress = build.state.lock();
    let from = query.logs_offset.unwrap_or(0).min(progress.logs.len());
    let to = (from + LOGS_PER_ANSWER).min(progress.logs.len());
    let mut answer = json!({
        "templateID": build.template,
        "buildID": build.build_id,
        "status": progress.status,
        "logs": [],
        "logEntries": &progress.logs[from..to],
    });
    if let Some(reason) = &progress.reason {
        answer["reason"] = reason.clone();
    }
    Json(answer).into_response()
}

/// `GET /templates/aliases/{alias}`: whether a template has this name.
pub(crate) async fn alias(
    State(state): State<Arc<AppState>>,
    Path(alias): Path<String>,
) -> Response {
    let name = snapshots::untagged(&alias);
    if state.initrds.read().contains_key(name) || snapshots::lookup(&state, name).is_some() {
        Json(json!({ "templateID": name, "public": false })).into_response()
    } else {
        api_error(StatusCode::NOT_FOUND, format!("no template {name}"))
    }
}

/// A build's failure: the step it failed in, as E2B names steps (`base`, a
/// step's number, `finalize`), and why.
type Failure = (String, String);

fn at(step: &str) -> impl Fn(String) -> Failure + '_ {
    move |message| (step.to_string(), message)
}

/// The template an image is pulled into, named for the image and the size of
/// its guests: a second build from it at that size starts at once.
fn image_template(image: &str, sizes: Sizes, default: Sizes) -> String {
    use sha2::Digest;
    let mut named = image.to_string();
    if sizes != default {
        named += &format!("\0{}\0{}", sizes.cpus, sizes.memory_mb);
    }
    let digest = sha2::Sha256::digest(named.as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("image-{hex}")
}

/// The name the snapshot of a build's steps is kept under: a function of
/// its base, its size, and every step, files included by their hash. A
/// build whose steps are the same starts from it, at the start command --
/// what E2B's layer cache does for the whole run of them.
fn step_cache_name(spec: &StartBuild, sizes: Option<Sizes>, scope: &str) -> String {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    let mut key = json!({
        "image": spec.from_image,
        "template": spec.from_template,
        "sizes": sizes.map(|s| (s.cpus, s.memory_mb)),
        "steps": spec.steps.iter().map(|s| json!([s.kind, s.args, s.files_hash])).collect::<Vec<_>>(),
    });
    // A scope's steps are its own: another's build with the same steps, and
    // so the same files hashes, must not start from what this one kept.
    // Unscoped (an administrator's, or a node's without teams), the name is
    // what it was.
    if !scope.is_empty() {
        key["scope"] = json!(scope);
    }
    hash.update(key.to_string().as_bytes());
    let hex: String = hash
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("build-cache-{hex}")
}

async fn run(state: &Arc<AppState>, build: &Arc<Build>, spec: StartBuild) -> Result<(), Failure> {
    let base_step = at("base");
    let cache_name = step_cache_name(&spec, build.sizes, &build.scope);
    let cached = if spec.force || spec.steps.is_empty() {
        None
    } else {
        snapshots::lookup(state, &cache_name)
            .filter(|c| state.templates.read().contains_key(&c.base))
    };
    // The base: the snapshot of these steps, run before; an image, pulled
    // into a template of its own; or a template -- one built from an image,
    // or a snapshot, restored as it was left.
    let (base, from_snapshot) = if let Some(cache) = &cached {
        build.log(
            "info",
            Some("base"),
            format!("Steps unchanged: starting from {cache_name}"),
        );
        (cache.base.clone(), Some(Arc::clone(cache)))
    } else if let Some(image) = &spec.from_image {
        let default = Sizes::of(&state.opts);
        let sizes = build.sizes.unwrap_or(default);
        let name = image_template(image, sizes, default);
        if spec.force || !state.initrds.read().contains_key(&name) {
            build.log("info", Some("base"), format!("Pulling {image}"));
            let credentials = match &spec.registry {
                None => None,
                Some(login) if login.kind == "registry" => Some(Credentials {
                    username: login.username.clone().unwrap_or_default(),
                    password: login.password.clone().unwrap_or_default(),
                }),
                Some(login) if login.kind == "aws" => Some(
                    crate::cloud_login::ecr(
                        &state.http,
                        login.aws_access_key_id.as_deref().unwrap_or_default(),
                        login.aws_secret_access_key.as_deref().unwrap_or_default(),
                        login.aws_region.as_deref().unwrap_or_default(),
                    )
                    .await
                    .map_err(&base_step)?,
                ),
                Some(login) if login.kind == "gcp" => Some(
                    crate::cloud_login::gcp(
                        &state.http,
                        login.service_account_json.as_deref().unwrap_or_default(),
                    )
                    .await
                    .map_err(&base_step)?,
                ),
                Some(login) => {
                    return Err(base_step(format!(
                        "registry logins of type {:?}: registry, aws and gcp are supported",
                        login.kind
                    )));
                }
            };
            build_from_image(state, &name, image, credentials, sizes)
                .await
                .map_err(&base_step)?;
        } else {
            build.log("info", Some("base"), format!("{image}: pulled already"));
        }
        (name, None)
    } else {
        let requested = spec.from_template.as_deref().unwrap_or_default();
        let name = snapshots::untagged(requested);
        match snapshots::lookup(state, name) {
            Some(snapshot) if snapshot.sizes.is_some() => {
                return Err(base_step(format!(
                    "{requested:?} is a size of its own, and a template built on it would be too; \
                     to size one, build it from an image"
                )));
            }
            Some(snapshot) => (snapshot.base.clone(), Some(snapshot)),
            None if state.initrds.read().contains_key(name) => (name.to_string(), None),
            None => return Err(base_step(format!("no template {requested:?}"))),
        }
    };
    // A template's size is its snapshot's: one restored from it is that size.
    if let Some(asked) = build.sizes {
        let has = sizes_of(state, &base);
        if asked != has {
            return Err(base_step(format!(
                "the base is {} vCPU and {} MiB, and a template built on it is too; \
                 to size one, build it from an image",
                has.cpus, has.memory_mb
            )));
        }
    }
    if !state.templates.read().contains_key(&base) {
        return Err(base_step(
            "template builds need templates restored from snapshots, and this node boots them"
                .into(),
        ));
    }

    let slot = reserve(state, create_park(state))
        .await
        .map_err(&base_step)?;
    let network = if state.opts.network {
        let policy = policy_from(
            Some(true),
            None,
            state.opts.egress_default,
            &state.opts.tenant_reserved,
        )
        .map_err(&base_step)?;
        Some(NetworkSpec {
            policy,
            proxy: None,
            tokens: BTreeMap::new(),
        })
    } else {
        build.log(
            "warn",
            Some("base"),
            "this node gives sandboxes no network: steps run offline",
        );
        None
    };
    let sandbox_id = format!("build-{}", &build.build_id[..8]);
    let running = bring_up(
        state,
        &sandbox_id,
        &base,
        from_snapshot.as_ref().map(|s| s.file.as_path()),
        network,
        &[],
        None,
        None,
        &BTreeMap::new(),
        &[],
        &new_access_token(),
        None,
    )
    .await
    .map_err(|(_, e)| base_step(e))?;
    drop(from_snapshot);
    build.log("info", Some("base"), format!("Building in {sandbox_id}"));

    let cache = (cached.is_none() && !spec.steps.is_empty()).then_some(cache_name.as_str());
    let outcome = steps_then_snapshot(
        state,
        build,
        &running,
        &spec,
        &base,
        &sandbox_id,
        cached.is_some(),
        cache,
    )
    .await;
    tear_down(state, running, &sandbox_id).await;
    drop(slot);
    outcome
}

async fn tear_down(state: &AppState, running: Running, sandbox_id: &str) {
    state.routes.remove_sandbox(sandbox_id);
    let _ = running.process_shutdown.send(());
    if let Some(network) = running.network {
        network.bridge.abort();
    }
    if let Err(e) = running.vm.stop().await {
        tracing::warn!("stopping build sandbox {sandbox_id}: {e}");
    }
}

/// Where a step runs: the directory, user and environment the steps before
/// it left. Kept in the guest with a build's step cache, for the build that
/// starts from it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Context {
    env: BTreeMap<String, String>,
    cwd: String,
    user: String,
    /// The guest's own environment when the build began -- the image's
    /// `ENV` among it -- which an `ENV` value's `$PATH` refers to.
    base_env: BTreeMap<String, String>,
    /// A Dockerfile's `ENTRYPOINT` and `CMD`, as argument vectors, kept for
    /// `hm run` in the guest's `/etc/hv2/image.json`.
    #[serde(default)]
    entrypoint: Option<Vec<String>>,
    #[serde(default)]
    cmd: Option<Vec<String>>,
}

/// Where a build's context is kept in its guest, for a step cache.
const BUILD_CONTEXT_PATH: &str = "/etc/hv2/build-context.json";
/// How long one attempt of a readiness check may take.
const READY_ATTEMPT: Duration = Duration::from_secs(30);

#[allow(clippy::too_many_arguments)]
async fn steps_then_snapshot(
    state: &AppState,
    build: &Arc<Build>,
    running: &Running,
    spec: &StartBuild,
    base: &str,
    sandbox_id: &str,
    cached: bool,
    cache: Option<&str>,
) -> Result<(), Failure> {
    let vm = &running.vm;
    if cached {
        let context: Context = vm
            .read_file_in_guest(BUILD_CONTEXT_PATH, 1 << 20, AGENT_TIMEOUT)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| at("base")("the step cache has no build context".into()))?;
        return finalize(state, build, running, spec, base, sandbox_id, context).await;
    }
    // A template built from one built by steps starts where that one ended.
    let inherited: TemplateDefaults = vm
        .read_file_in_guest(TEMPLATE_DEFAULTS_PATH, 1 << 20, AGENT_TIMEOUT)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let base_env = base_environment(vm, &inherited.env).await;
    let mut context = Context {
        env: inherited.env,
        cwd: inherited.cwd.unwrap_or_else(|| "/".into()),
        user: "root".into(),
        base_env,
        entrypoint: None,
        cmd: None,
    };

    for (index, step) in spec.steps.iter().enumerate() {
        let name = (index + 1).to_string();
        let fail = at(&name);
        build.log(
            "info",
            Some(name.as_str()),
            format!(
                "[{name}/{}] {} {}",
                spec.steps.len(),
                step.kind,
                shown(step)
            ),
        );
        match step.kind.as_str() {
            "RUN" => {
                let command = step
                    .args
                    .first()
                    .ok_or_else(|| fail("RUN without a command".into()))?;
                let user = step
                    .args
                    .get(1)
                    .filter(|u| !u.is_empty())
                    .unwrap_or(&context.user);
                run_command(vm, build, &name, command, user, &context)
                    .await
                    .map_err(|e| {
                        if crate::guest_setup::guest_has_no_shell(&e) {
                            format!("a RUN step runs through /bin/sh: {e}")
                        } else {
                            e
                        }
                    })
                    .map_err(&fail)?;
            }
            "ENV" => {
                for pair in step.args.chunks(2) {
                    if let [key, value] = pair {
                        let mut known = context.base_env.clone();
                        known.extend(context.env.clone());
                        let value = expand(value, &known);
                        context.env.insert(key.clone(), value);
                    }
                }
            }
            "WORKDIR" => {
                let dir = step
                    .args
                    .first()
                    .ok_or_else(|| fail("WORKDIR without a path".into()))?;
                let dir = resolve(&context.cwd, dir);
                // Through the agent's file operations: no shell, no `mkdir`.
                // Only an agent that predates them runs the script.
                match workdir_by_agent(vm, &dir, &context.user).await {
                    Ok(()) => tracing::debug!("WORKDIR {dir}: using agent op"),
                    Err(Unpack::Failed(e)) => return Err(fail(e)),
                    Err(Unpack::Shell(why)) => {
                        tracing::debug!("WORKDIR {dir}: falling back to shell ({why})");
                        let owner = (context.user != "root")
                            .then(|| format!(" && chown {} {}", quote(&context.user), quote(&dir)));
                        let script =
                            format!("mkdir -p {}{}", quote(&dir), owner.unwrap_or_default());
                        run_command(vm, build, &name, &script, "root", &context)
                            .await
                            .map_err(&fail)?;
                    }
                }
                context.cwd = dir;
            }
            "USER" => {
                let user = step
                    .args
                    .first()
                    .ok_or_else(|| fail("USER without a user".into()))?;
                ensure_user(vm, build, &name, user, &context)
                    .await
                    .map_err(&fail)?;
                context.user = user.clone();
            }
            "COPY" => {
                copy(state, vm, build, &name, step, &context)
                    .await
                    .map_err(&fail)?;
            }
            // The program an image runs by default, argv in `args`.
            "ENTRYPOINT" => context.entrypoint = Some(step.args.clone()),
            "CMD" => context.cmd = Some(step.args.clone()),
            other => return Err(fail(format!("a step of type {other:?} is not supported"))),
        }
    }

    // The steps' result, kept: a build with the same steps starts here, and
    // a start command that fails costs one more try at it, not the steps.
    if let Some(cache) = cache {
        let kept = async {
            let bytes = serde_json::to_vec(&context).map_err(|e| e.to_string())?;
            vm.write_file_in_guest(BUILD_CONTEXT_PATH, bytes, AGENT_TIMEOUT)
                .await
                .map_err(|e| e.to_string())?;
            let file = snapshots::new_file(state, cache)?;
            let started = Instant::now();
            vm.checkpoint_to(&file).await.map_err(|e| e.to_string())?;
            snapshots::keep(
                state,
                cache,
                base.to_string(),
                file,
                sandbox_id.to_string(),
                None,
                None,
            )
            .await?;
            Ok::<_, String>(started.elapsed())
        }
        .await;
        match kept {
            Ok(took) => build.log(
                "info",
                Some("finalize"),
                format!("Steps kept as {cache} in {took:?}"),
            ),
            Err(e) => build.log(
                "warn",
                Some("finalize"),
                format!("Steps not kept as {cache}: {e}"),
            ),
        }
    }
    finalize(state, build, running, spec, base, sandbox_id, context).await
}

/// After the steps: the template's defaults, its start command up and its
/// readiness check passed, and the snapshot taken.
async fn finalize(
    state: &AppState,
    build: &Arc<Build>,
    running: &Running,
    spec: &StartBuild,
    base: &str,
    sandbox_id: &str,
    context: Context,
) -> Result<(), Failure> {
    let vm = &running.vm;
    let finalize = at("finalize");
    let defaults = TemplateDefaults {
        env: context.env.clone(),
        cwd: Some(context.cwd.clone()),
        user: (context.user != "root").then(|| context.user.clone()),
    };
    let bytes = serde_json::to_vec(&defaults).map_err(|e| finalize(e.to_string()))?;
    vm.write_file_in_guest(TEMPLATE_DEFAULTS_PATH, bytes, AGENT_TIMEOUT)
        .await
        .map_err(|e| finalize(e.to_string()))?;
    // What `hm run` starts: the base image's launch settings (when it was
    // pulled) with this Dockerfile's `ENTRYPOINT`, `CMD` and `USER` over them.
    let launch_path = "/etc/hv2/image.json";
    let mut launch: serde_json::Value = vm
        .read_file_in_guest(launch_path, 1 << 20, AGENT_TIMEOUT)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if context.entrypoint.is_some() || context.cmd.is_some() || context.user != "root" {
        if let Some(entrypoint) = &context.entrypoint {
            launch["entrypoint"] = serde_json::json!(entrypoint);
            // As in Docker, a new ENTRYPOINT drops the base image's CMD.
            launch["cmd"] = serde_json::json!(context.cmd.clone().unwrap_or_default());
        } else if let Some(cmd) = &context.cmd {
            launch["cmd"] = serde_json::json!(cmd);
        }
        if context.user != "root" {
            launch["user"] = serde_json::json!(context.user);
        }
        let bytes = serde_json::to_vec(&launch).map_err(|e| finalize(e.to_string()))?;
        vm.write_file_in_guest(launch_path, bytes, AGENT_TIMEOUT)
            .await
            .map_err(|e| finalize(e.to_string()))?;
    }

    // The start command's output goes to the build's log as it comes, and
    // its end, if it ends, is noticed: a server that exits is a readiness
    // check that will never pass, and why is in what it printed.
    let exited: Arc<parking_lot::Mutex<Option<Option<i32>>>> =
        Arc::new(parking_lot::Mutex::new(None));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(command) = &spec.start_cmd {
        build.log("info", Some("finalize"), format!("Starting: {command}"));
        let (program, args) = shell(command);
        let pid = vm
            .start_in_guest_as(
                &program,
                &args,
                Some(&context.cwd),
                &context.env,
                None,
                Some(&context.user),
                AGENT_TIMEOUT,
            )
            .await
            .map_err(|e| finalize(format!("starting {command:?}: {e}")))?;
        let (vm, build, exited, stop) = (
            Arc::clone(vm),
            Arc::clone(build),
            Arc::clone(&exited),
            Arc::clone(&stop),
        );
        tokio::spawn(async move {
            let mut partial = String::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok(output) = vm.poll_in_guest(pid, AGENT_TIMEOUT).await else {
                    break;
                };
                partial.push_str(&output.stdout);
                partial.push_str(&output.stderr);
                while let Some(end) = partial.find('\n') {
                    let line: String = partial.drain(..=end).collect();
                    build.log(
                        "info",
                        Some("finalize"),
                        format!("start: {}", line.trim_end()),
                    );
                }
                if !output.running {
                    if !partial.is_empty() {
                        build.log("info", Some("finalize"), format!("start: {partial}"));
                    }
                    *exited.lock() = Some(output.exit_code);
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        });
    }
    let ready = async {
        let Some(check) = &spec.ready_cmd else {
            return Ok(());
        };
        build.log("info", Some("finalize"), format!("Waiting for: {check}"));
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(code) = *exited.lock() {
                return Err(finalize(format!(
                    "the start command exited ({}) before {check:?} passed; its output is above",
                    code.map_or_else(|| "killed".to_string(), |c| format!("status {c}"))
                )));
            }
            let (program, args) = shell(check);
            let passed = match vm
                .start_in_guest_as(
                    &program,
                    &args,
                    Some(&context.cwd),
                    &context.env,
                    None,
                    Some(&context.user),
                    AGENT_TIMEOUT,
                )
                .await
            {
                // Each attempt bounded: a check that hangs is a failed one.
                Ok(pid) => match tokio::time::timeout(READY_ATTEMPT, finished(vm, pid, None)).await
                {
                    Ok(code) => code.is_ok_and(|code| code == Some(0)),
                    Err(_) => {
                        let _ = vm.signal_in_guest(pid, 9, AGENT_TIMEOUT).await;
                        false
                    }
                },
                Err(_) => false,
            };
            if passed {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(finalize(format!(
                    "the ready check {check:?} did not pass in {READY_TIMEOUT:?}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    .await;
    // Past the check, the output is the template's own again: a snapshot
    // with this poller mid-request would be of an agent answering no one.
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    ready?;

    let file = snapshots::new_file(state, &build.template).map_err(&finalize)?;
    let started = Instant::now();
    vm.checkpoint_to(&file)
        .await
        .map_err(|e| finalize(format!("snapshotting: {e}")))?;
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    snapshots::keep(
        state,
        &build.template,
        base.to_string(),
        file,
        sandbox_id.to_string(),
        None,
        None,
    )
    .await
    .map_err(&finalize)?;
    build.log(
        "info",
        Some("finalize"),
        format!(
            "Template {} ready: snapshot of {} MiB in {:?}",
            build.template,
            size >> 20,
            started.elapsed()
        ),
    );
    Ok(())
}

fn shown(step: &Step) -> String {
    match step.kind.as_str() {
        "COPY" => format!(
            "{} {}",
            step.args.first().map_or("", String::as_str),
            step.args.get(1).map_or("", String::as_str)
        ),
        _ => step.args.join(" "),
    }
}

/// `$NAME` and `${NAME}` in an `ENV` value, from the variables set before
/// it, as Docker expands them; one unset expands to nothing, and `\$` is a
/// literal `$`.
fn expand(value: &str, env: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'$') => out.push(chars.next().unwrap_or('$')),
            '$' => {
                let braced = chars.peek() == Some(&'{');
                if braced {
                    chars.next();
                }
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '_' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if braced {
                    if chars.peek() == Some(&'}') {
                        chars.next();
                    } else {
                        // Not a reference after all: kept as written.
                        out.push_str("${");
                        out.push_str(&name);
                        continue;
                    }
                }
                if name.is_empty() {
                    out.push('$');
                } else if let Some(v) = env.get(&name) {
                    out.push_str(v);
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// `path` from `cwd`, as `WORKDIR` and `COPY` resolve a relative path.
fn resolve(cwd: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    }
}

/// A shell word holding exactly `s`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `command` in a shell. Who runs it is the agent's to arrange: it drops to
/// the user from root itself, with that user's groups, `HOME` and `USER`.
fn shell(command: &str) -> (String, Vec<String>) {
    ("/bin/sh".into(), vec!["-c".into(), command.into()])
}

/// Run `command` in the build's sandbox as `user`, its output into the
/// build's log as it comes; an error unless it exits 0.
async fn run_command(
    vm: &AgentVM,
    build: &Build,
    step: &str,
    command: &str,
    user: &str,
    context: &Context,
) -> Result<(), String> {
    let (program, args) = shell(command);
    let pid = vm
        .start_in_guest_as(
            &program,
            &args,
            Some(&context.cwd),
            &context.env,
            None,
            Some(user),
            AGENT_TIMEOUT,
        )
        .await
        .map_err(|e| start_failure(&e.to_string()))?;
    match finished(vm, pid, Some((build, step))).await? {
        Some(0) => Ok(()),
        Some(code) => Err(format!("{command:?} exited {code}")),
        None => Err(format!("{command:?} was killed by a signal")),
    }
}

/// Wait for `pid` to end, its output into `log` if given: its exit code,
/// `None` if a signal ended it.
async fn finished(
    vm: &AgentVM,
    pid: u32,
    log: Option<(&Build, &str)>,
) -> Result<Option<i32>, String> {
    let deadline = Instant::now() + STEP_TIMEOUT;
    let mut partial = String::new();
    loop {
        let output = vm
            .poll_in_guest(pid, AGENT_TIMEOUT)
            .await
            .map_err(|e| format!("polling: {e}"))?;
        if let Some((build, step)) = log {
            partial.push_str(&output.stdout);
            partial.push_str(&output.stderr);
            while let Some(end) = partial.find('\n') {
                let line: String = partial.drain(..=end).collect();
                build.log("info", Some(step), line.trim_end());
            }
            if !output.running && !partial.is_empty() {
                build.log("info", Some(step), std::mem::take(&mut partial));
            }
        }
        if !output.running {
            return Ok(output.exit_code);
        }
        if Instant::now() > deadline {
            let _ = vm.signal_in_guest(pid, 9, AGENT_TIMEOUT).await;
            return Err(format!("still running after {STEP_TIMEOUT:?}"));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// `user`, made if the image has none by that name -- E2B's templates run
/// as `user`, which an image from a registry seldom has.
async fn ensure_user(
    vm: &AgentVM,
    build: &Build,
    step: &str,
    user: &str,
    context: &Context,
) -> Result<(), String> {
    if user == "root" || user == "0" {
        return Ok(());
    }
    // An image that already has the user needs nothing run, so a build on an
    // image with no shell can say `USER app` when it has one.
    match vm
        .read_file_in_guest("/etc/passwd", 1 << 20, AGENT_TIMEOUT)
        .await
    {
        Ok(bytes) if passwd_has_user(&String::from_utf8_lossy(&bytes), user) => {
            tracing::debug!("USER {user}: using agent op (found in /etc/passwd, nothing to run)");
            return Ok(());
        }
        _ => {}
    }
    tracing::debug!("USER {user}: not in /etc/passwd, falling back to shell");
    let u = quote(user);
    let script = format!(
        "id -u {u} >/dev/null 2>&1 || {{ \
           if command -v useradd >/dev/null 2>&1; then useradd -m -s /bin/sh {u}; \
           else /bin/busybox adduser -D -h /home/{user} -s /bin/sh {u}; fi; }}"
    );
    run_command(vm, build, step, &script, "root", context)
        .await
        .map_err(|e| {
            if crate::guest_setup::guest_has_no_shell(&e) {
                format!(
                    "user {user} is not in the image's /etc/passwd and the image has no \
                     shell to create it: {e}"
                )
            } else {
                e
            }
        })
}

/// The agent's own environment, which every program it starts inherits (the
/// template's defaults and the request's variables go over it): the base a
/// step's `ENV` expands against. `busybox env` reports exactly that, so it
/// is asked first; an image with no busybox is read from the agent instead.
async fn base_environment(
    vm: &AgentVM,
    defaults: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    if let Ok(out) = vm
        .exec_in_guest("/bin/busybox", &["env".to_string()], AGENT_TIMEOUT)
        .await
    {
        tracing::debug!("reading the base environment: using busybox env");
        return out
            .stdout
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    }
    tracing::debug!("reading the base environment: falling back to /proc/self/environ");
    let environ = vm
        .read_file_in_guest("/proc/self/environ", 1 << 20, AGENT_TIMEOUT)
        .await
        .unwrap_or_default();
    let stat = vm
        .read_file_in_guest("/proc/self/stat", 4096, AGENT_TIMEOUT)
        .await
        .unwrap_or_default();
    let mut env = agent_environment(&environ, &stat);
    // `busybox env` shows the template's defaults over the agent's variables.
    env.extend(defaults.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
}

/// What the agent's `/proc/self/environ` and `/proc/self/stat` say its
/// children inherit. `environ` is the environment the agent was *started*
/// with, which the kernel keeps unchanged: the agent as PID 1 sets `PATH`
/// and `HOME` itself afterwards (setenv), which `environ` does not show, so
/// they are added when `stat`'s first field says the agent is PID 1.
fn agent_environment(environ: &[u8], stat: &[u8]) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = environ
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (key, value) = entry.split_once('=')?;
            (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect();
    let pid = String::from_utf8_lossy(stat)
        .split_whitespace()
        .next()
        .and_then(|p| p.parse::<u32>().ok());
    if pid == Some(1) {
        env.insert("PATH".into(), "/bin".into());
        env.insert("HOME".into(), "/root".into());
    }
    env
}

/// Whether `/etc/passwd` has `user`, by name or by uid.
fn passwd_has_user(passwd: &str, user: &str) -> bool {
    !user.is_empty()
        && passwd.lines().any(|line| {
            let mut fields = line.split(':');
            let (name, _, uid) = (fields.next(), fields.next(), fields.next());
            line.split(':').count() >= 6 && (name == Some(user) || uid == Some(user))
        })
}

/// A `--chmod` that is an octal number of at most four digits, as `chmod`
/// reads one; anything else (`u+x`) is the shell's to interpret.
fn parse_octal_mode(mode: &str) -> Option<u32> {
    let valid =
        !mode.is_empty() && mode.len() <= 4 && mode.bytes().all(|b| (b'0'..=b'7').contains(&b));
    valid.then(|| u32::from_str_radix(mode, 8).ok()).flatten()
}

/// What a failure to start a shell-form command says: a guest with no
/// `/bin/sh` is a supported kind of image, and gets an explanation.
fn start_failure(error: &str) -> String {
    if crate::guest_setup::guest_has_no_shell(error) {
        crate::guest_setup::no_shell_message(false, error)
    } else {
        format!("starting: {error}")
    }
}

/// An agent op's failure, as the unpack code reports it: an agent that
/// predates the operation is the shell's to cover for.
fn by_agent(what: &str, e: hv2_agent::AgentError) -> Unpack {
    if hv2_agent::is_unsupported_op(&e) {
        Unpack::Shell(format!("the guest's agent predates {what}"))
    } else {
        Unpack::Failed(format!("{what}: {e}"))
    }
}

/// Refuse, as the shell's to do, an `owner` the agent cannot resolve: it
/// looks owners up in `/etc/passwd` only, where `chown` also takes a bare
/// uid with no entry.
async fn owner_for_agent(vm: &AgentVM, owner: &str) -> Result<(), Unpack> {
    if owner == "root" || owner == "0" {
        return Ok(());
    }
    let known = vm
        .read_file_in_guest("/etc/passwd", 4 << 20, AGENT_TIMEOUT)
        .await
        .map(|bytes| passwd_has_user(&String::from_utf8_lossy(&bytes), owner))
        .unwrap_or(false);
    if known {
        Ok(())
    } else {
        Err(Unpack::Shell(format!(
            "owner {owner} is not in /etc/passwd"
        )))
    }
}

/// `dir` made with its parents, and given to `user` unless that is root, as
/// `mkdir -p` and `chown` would, through the agent.
async fn workdir_by_agent(vm: &AgentVM, dir: &str, user: &str) -> Result<(), Unpack> {
    let wait = AGENT_TIMEOUT;
    owner_for_agent(vm, user).await?;
    // The two operations in one round trip, where the agent batches; an
    // agent that does not ran neither, and they go one at a time below.
    {
        use hv2_guest_agent::Operation;
        let mut ops = vec![Operation::MakeDir {
            path: dir.to_string(),
            parents: true,
            owner: None,
            mode: None,
        }];
        if user != "root" {
            ops.push(Operation::SetAttr {
                path: dir.to_string(),
                owner: Some(user.to_string()),
                mode: None,
                recursive: false,
            });
        }
        let whats = [format!("making {dir}"), format!("giving {dir} to {user}")];
        match vm.batch_in_guest(ops, wait).await {
            Ok(results) => {
                return match crate::env_vars::first_failure(&whats[..results.len()], &results) {
                    None => Ok(()),
                    Some((what, message)) => {
                        Err(by_agent(&what, hv2_agent::AgentError::Script(message)))
                    }
                };
            }
            Err(e) if hv2_agent::is_unsupported_op(&e) => {
                tracing::debug!(
                    "making {dir}: the guest's agent has no batch operation, using one op per connection"
                );
            }
            Err(e) => return Err(by_agent(&format!("making {dir}"), e)),
        }
    }
    vm.make_dir_in_guest(dir, true, None, None, wait)
        .await
        .map_err(|e| by_agent(&format!("making {dir}"), e))?;
    if user != "root" {
        vm.set_attr_in_guest(dir, Some(user), None, false, wait)
            .await
            .map_err(|e| by_agent(&format!("giving {dir} to {user}"), e))?;
    }
    Ok(())
}

/// Remove what is at `path` if it is not a directory, so that writing a
/// member replaces it (as `tar` does) rather than writing through a link.
async fn clear_for_member(vm: &AgentVM, path: &str) -> Result<(), Unpack> {
    let wait = AGENT_TIMEOUT;
    match vm.stat_in_guest(path, false, wait).await {
        Ok(entry) if entry.kind != hv2_guest_agent::DirEntryKind::Dir => vm
            .remove_in_guest(path, false, true, wait)
            .await
            .map_err(|e| by_agent(&format!("replacing {path}"), e)),
        _ => Ok(()),
    }
}

/// The steps of a COPY's `members` laid at `/` as [`copy_by_agent`] does, in
/// the order it does them. `replace` holds the paths that had something that
/// is not a directory on them (see [`clear_for_member`]), which is removed
/// first so that the write replaces it rather than going through a link.
fn copy_steps<'a>(
    members: &'a [Member],
    replace: &std::collections::HashSet<String>,
    paths: &[String],
    owner: Option<&str>,
    mode: Option<u32>,
) -> Vec<crate::env_vars::Step<'a>> {
    use crate::env_vars::{set_mode, Step};
    use hv2_guest_agent::Operation;
    let op = |what: String, op: Operation| Step::Op { what, op };
    let clear = |path: &str| {
        op(
            format!("replacing {path}"),
            Operation::Remove {
                path: path.to_string(),
                recursive: false,
                missing_ok: true,
            },
        )
    };
    let mut steps = Vec::new();
    for member in members {
        match member {
            Member::Dir { rel, mode } => {
                let path = join_guest("/", rel);
                steps.push(op(
                    format!("making {path}"),
                    Operation::MakeDir {
                        path: path.clone(),
                        parents: true,
                        owner: None,
                        mode: Some(*mode),
                    },
                ));
                steps.push(op(
                    format!("setting the mode of {path}"),
                    set_mode(&path, *mode),
                ));
            }
            Member::File { rel, mode, data } => {
                let path = join_guest("/", rel);
                if replace.contains(&path) {
                    steps.push(clear(&path));
                }
                steps.push(Step::Write {
                    what: format!("writing {path}"),
                    path: path.clone(),
                    data,
                    owner: None,
                });
                steps.push(op(
                    format!("setting the mode of {path}"),
                    set_mode(&path, *mode),
                ));
            }
            Member::Symlink { rel, target } => {
                let path = join_guest("/", rel);
                if let Some((parent, _)) = path.rsplit_once('/') {
                    if !parent.is_empty() {
                        steps.push(op(
                            format!("making {parent}"),
                            Operation::MakeDir {
                                path: parent.to_string(),
                                parents: true,
                                owner: None,
                                mode: None,
                            },
                        ));
                    }
                }
                if replace.contains(&path) {
                    steps.push(clear(&path));
                }
                steps.push(op(
                    format!("linking {path}"),
                    Operation::MakeSymlink {
                        target: target.clone(),
                        path,
                        owner: None,
                    },
                ));
            }
        }
    }
    if owner.is_some() || mode.is_some() {
        for path in paths {
            let what = match owner {
                Some(o) => format!("giving {path} to {o}"),
                None => format!("setting the mode of {path}"),
            };
            steps.push(op(
                what,
                Operation::SetAttr {
                    path: path.clone(),
                    owner: owner.map(str::to_string),
                    mode,
                    recursive: false,
                },
            ));
        }
    }
    steps
}

/// Whether two members name one path. Batching looks at what is on each path
/// before anything is written, which an earlier member of the same archive
/// would make stale; such an archive (a COPY's never is) takes the slow way.
fn has_repeated_paths(members: &[Member]) -> bool {
    let mut seen = std::collections::HashSet::new();
    members.iter().any(|m| {
        let rel = match m {
            Member::Dir { rel, .. } | Member::File { rel, .. } | Member::Symlink { rel, .. } => rel,
        };
        !seen.insert(rel.as_str())
    })
}

/// [`copy_by_agent`] through the agent's batch operation: one batch to see
/// what is on each file and link path, then the steps in a few more.
/// `Ok(false)`: not done and nothing written -- the agent has no batch
/// operation, or the archive repeats a path -- so the caller goes one
/// operation at a time.
async fn copy_batched(
    vm: &AgentVM,
    members: &[Member],
    paths: &[String],
    owner: Option<&str>,
    mode: Option<u32>,
    wait: Duration,
) -> Result<bool, Unpack> {
    use hv2_guest_agent::{DirEntryKind, OpResult, Operation};
    if has_repeated_paths(members) {
        tracing::debug!("COPY: the archive repeats a path, using one op per connection");
        return Ok(false);
    }
    let mut checked = Vec::new();
    for member in members {
        if let Member::File { rel, .. } | Member::Symlink { rel, .. } = member {
            checked.push(join_guest("/", rel));
        }
    }
    let stats = checked
        .iter()
        .map(|path| Operation::Stat {
            path: path.clone(),
            follow: false,
        })
        .collect();
    let results = match vm.batch_in_guest(stats, AGENT_TIMEOUT * 6).await {
        Ok(results) => results,
        Err(e) if hv2_agent::is_unsupported_op(&e) => {
            tracing::debug!(
                "COPY: the guest's agent has no batch operation, using one op per connection"
            );
            return Ok(false);
        }
        Err(e) => return Err(by_agent("looking at what the COPY replaces", e)),
    };
    // A path with nothing on it, or one that cannot be looked at, is not one
    // to clear, as `clear_for_member` has it.
    let replace: std::collections::HashSet<String> = checked
        .into_iter()
        .zip(results)
        .filter(|(_, result)| {
            matches!(result, OpResult::FileStat(entry) if entry.kind != DirEntryKind::Dir)
        })
        .map(|(path, _)| path)
        .collect();
    let steps = copy_steps(members, &replace, paths, owner, mode);
    match crate::env_vars::run_steps(vm, steps, wait).await {
        Ok(()) => Ok(true),
        // An agent that took the first batch takes the rest; by_agent maps
        // a surprise to the shell anyway.
        Err((what, e)) => Err(by_agent(&what, e)),
    }
}

/// A COPY's repacked `archive` (paths relative to `/`) laid into the guest
/// through the agent: directories made, files written, links made, then
/// `owner` and `mode` given to each of `paths` as `chown -h` and `chmod`
/// over the list would. Not recursive, as they were not.
///
/// Where this is not the same as the shell script, and why it is accepted:
/// the owner's primary group is set too (`chown user` keeps the group); a
/// symlink's mode is left alone (`chmod` would change its target's);
/// setuid/setgid bits of a file are dropped (a `chown` would clear them
/// anyway, and root-owned files are rare in a build context); modification
/// times are not kept.
async fn copy_by_agent(
    vm: &AgentVM,
    archive: &[u8],
    paths: &[String],
    owner: &str,
    mode: Option<&str>,
) -> Result<(), Unpack> {
    let wait = AGENT_TIMEOUT * 6;
    let mode = match mode {
        None => None,
        Some(m) => Some(
            parse_octal_mode(m)
                .ok_or_else(|| Unpack::Shell(format!("--chmod {m:?} is not an octal number")))?,
        ),
    };
    if owner.contains(':') {
        return Err(Unpack::Shell(format!("--chown {owner:?} names a group")));
    }
    owner_for_agent(vm, owner).await?;
    // Read it all first: an archive the agent cannot reproduce is found
    // before anything is written.
    let members = crate::env_vars::members_of(archive, "the COPY's files")?;
    let fail = |path: &str, e: hv2_agent::AgentError| by_agent(path, e);
    // Through the agent's batch operation where it has one: the same steps in
    // a few round trips instead of several per file.
    let kept_owner = (owner != "root" && owner != "0").then_some(owner);
    if copy_batched(vm, &members, paths, kept_owner, mode, wait).await? {
        return Ok(());
    }
    // One cheap new-protocol operation first, so an agent that predates them
    // is found before anything is written.
    if let Err(e) = vm.stat_in_guest("/", false, wait).await {
        if hv2_agent::is_unsupported_op(&e) {
            return Err(by_agent("file operations", e));
        }
    }
    for member in &members {
        match member {
            Member::Dir { rel, mode } => {
                let path = join_guest("/", rel);
                vm.make_dir_in_guest(&path, true, None, Some(*mode), wait)
                    .await
                    .map_err(|e| fail(&format!("making {path}"), e))?;
                vm.set_attr_in_guest(&path, None, Some(*mode), false, wait)
                    .await
                    .map_err(|e| fail(&format!("setting the mode of {path}"), e))?;
            }
            Member::File { rel, mode, data } => {
                let path = join_guest("/", rel);
                clear_for_member(vm, &path).await?;
                vm.write_file_in_guest_as(&path, data.clone(), None, wait)
                    .await
                    .map_err(|e| fail(&format!("writing {path}"), e))?;
                vm.set_attr_in_guest(&path, None, Some(*mode), false, wait)
                    .await
                    .map_err(|e| fail(&format!("setting the mode of {path}"), e))?;
            }
            Member::Symlink { rel, target } => {
                let path = join_guest("/", rel);
                if let Some((parent, _)) = path.rsplit_once('/') {
                    if !parent.is_empty() {
                        vm.make_dir_in_guest(parent, true, None, None, wait)
                            .await
                            .map_err(|e| fail(&format!("making {parent}"), e))?;
                    }
                }
                clear_for_member(vm, &path).await?;
                vm.make_symlink_in_guest(target, &path, None, wait)
                    .await
                    .map_err(|e| fail(&format!("linking {path}"), e))?;
            }
        }
    }
    let owner = (owner != "root" && owner != "0").then_some(owner);
    if owner.is_some() || mode.is_some() {
        for path in paths {
            vm.set_attr_in_guest(path, owner, mode, false, wait)
                .await
                .map_err(|e| match owner {
                    Some(o) => fail(&format!("giving {path} to {o}"), e),
                    None => fail(&format!("setting the mode of {path}"), e),
                })?;
        }
    }
    Ok(())
}

/// `COPY src dest [user] [mode]`: the upload's entries under `src`, laid
/// out at `dest` as Docker does -- a file to `dest`, or into it if it ends
/// in `/`; a directory's contents into `dest`; each glob match into `dest`.
/// Repacked on the host with their final paths, written into the guest, and
/// unpacked there by its own tar.
async fn copy(
    state: &AppState,
    vm: &AgentVM,
    build: &Build,
    step_name: &str,
    step: &Step,
    context: &Context,
) -> Result<(), String> {
    let (Some(src), Some(dest)) = (step.args.first(), step.args.get(1)) else {
        return Err("COPY needs a source and a destination".into());
    };
    let owner = step
        .args
        .get(2)
        .filter(|u| !u.is_empty())
        .unwrap_or(&context.user)
        .clone();
    let mode = step.args.get(3).filter(|m| !m.is_empty()).cloned();
    let hash = step
        .files_hash
        .as_deref()
        .ok_or("COPY without a filesHash")?;
    let upload = upload_path(state, &build.scope, hash).ok_or("a bad filesHash")?;
    let dest = resolve(&context.cwd, dest);
    let src = src.clone();

    let (archive, paths) = tokio::task::spawn_blocking(move || repack(&upload, &src, &dest))
        .await
        .map_err(|e| e.to_string())??;
    build.log(
        "info",
        Some(step_name),
        format!("{} paths, {} KiB", paths.len(), archive.len() >> 10),
    );
    // Through the agent's file operations: no archive staged, no `tar`,
    // `chown`, `chmod` or shell in the guest. Only an agent that predates
    // them, an archive it cannot reproduce (a hard link or special file), or
    // a mode or owner it cannot express (`u+x`, `user:group`) uses the shell.
    let why = match copy_by_agent(vm, &archive, &paths, &owner, mode.as_deref()).await {
        Ok(()) => {
            tracing::debug!("COPY step {step_name}: using agent op");
            return Ok(());
        }
        Err(Unpack::Failed(e)) => return Err(e),
        Err(Unpack::Shell(why)) => why,
    };
    tracing::debug!("COPY step {step_name}: falling back to shell ({why})");
    let staged = format!("/tmp/.hv2-copy-{}.tar", uuid::Uuid::new_v4().simple());
    let list = format!("{staged}.list");
    vm.write_file_in_guest(&staged, archive, AGENT_TIMEOUT * 10)
        .await
        .map_err(|e| format!("staging the files: {e}"))?;
    let mut listed = Vec::new();
    for path in &paths {
        listed.extend_from_slice(path.as_bytes());
        listed.push(0);
    }
    vm.write_file_in_guest(&list, listed, AGENT_TIMEOUT)
        .await
        .map_err(|e| format!("staging the file list: {e}"))?;
    let mut script = format!("/bin/busybox tar -xf {} -C / ", quote(&staged));
    if owner != "root" {
        script += &format!(
            "&& /bin/busybox xargs -0 chown -h {} < {} ",
            quote(&owner),
            quote(&list)
        );
    }
    if let Some(mode) = mode {
        script += &format!(
            "&& /bin/busybox xargs -0 chmod {} < {} ",
            quote(&mode),
            quote(&list)
        );
    }
    script += &format!(
        "; status=$?; rm -f {} {}; exit $status",
        quote(&staged),
        quote(&list)
    );
    run_command(vm, build, step_name, &script, "root", context)
        .await
        .map_err(|e| {
            if crate::guest_setup::guest_has_no_shell(&e) {
                format!("this COPY needs the shell ({why}): {e}")
            } else {
                e
            }
        })
}

/// The most a COPY's upload may unpack to, and the most one member may hold:
/// the repacked archive is built in memory and sent to the guest whole.
const MAX_COPY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The most members a COPY's upload may have.
const MAX_COPY_MEMBERS: usize = 1_000_000;

/// The upload's entries for `src`, as a tar of their paths at `dest`, and
/// those paths.
fn repack(
    upload: &std::path::Path,
    src: &str,
    dest: &str,
) -> Result<(Vec<u8>, Vec<String>), String> {
    repack_within(upload, src, dest, MAX_COPY_BYTES)
}

/// [`repack`], for an upload that may unpack to `limit` bytes.
fn repack_within(
    upload: &std::path::Path,
    src: &str,
    dest: &str,
    limit: u64,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let src = src.trim_start_matches("./").trim_end_matches('/');
    // The upload is read as a stream, never whole: it may be gigabytes, and a
    // few KiB of gzip may be gigabytes more. What it unpacks to, what one
    // member of it holds, and how many members it has are all bounded.
    let mut unpacked = limit;
    let mut file = std::fs::File::open(upload).map_err(|e| format!("the upload: {e}"))?;
    let mut magic = [0u8; 2];
    let gzip = file.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b];
    std::io::Seek::rewind(&mut file).map_err(|e| format!("the upload: {e}"))?;
    let file = std::io::BufReader::new(file);
    let reader: Box<dyn Read + '_> = if gzip {
        Box::new(crate::oci::Capped::new(
            flate2::read::GzDecoder::new(file),
            &mut unpacked,
        ))
    } else {
        Box::new(crate::oci::Capped::new(file, &mut unpacked))
    };
    // A glob's matches land under dest by their path below its fixed part.
    let glob = src.contains(['*', '?', '[']);
    let fixed = if glob {
        let parts: Vec<&str> = src.split('/').collect();
        let literal = parts
            .iter()
            .take_while(|p| !p.contains(['*', '?', '[']))
            .count();
        parts[..literal].join("/")
    } else {
        String::new()
    };
    let into_dir = dest.ends_with('/');
    let dest = dest.trim_end_matches('/');
    let dest = if dest.is_empty() { "" } else { dest };

    // Where an upload path lands, relative to `/`; `None` if the COPY leaves
    // it out.
    let place = |path: &str, is_dir: bool| -> Option<String> {
        if path.split('/').any(|p| p == "..") {
            return None;
        }
        let target = if glob {
            let below = if fixed.is_empty() {
                path
            } else {
                path.strip_prefix(&format!("{fixed}/"))?
            };
            format!("{dest}/{below}")
        } else if path == src {
            if is_dir {
                // The directory itself is dest; its contents come after.
                dest.to_string()
            } else if into_dir {
                let file = path.rsplit('/').next().unwrap_or(path);
                format!("{dest}/{file}")
            } else {
                dest.to_string()
            }
        } else if let Some(below) = path.strip_prefix(&format!("{src}/")) {
            format!("{dest}/{below}")
        } else if src == "." || src.is_empty() {
            format!("{dest}/{path}")
        } else {
            return None;
        };
        let target = target.trim_start_matches('/').to_string();
        (!target.is_empty()).then_some(target)
    };
    let normal = |path: &str| {
        path.trim_start_matches("./")
            .trim_end_matches('/')
            .to_string()
    };

    let mut out = tar::Builder::new(Vec::new());
    let mut paths = Vec::new();
    let mut members = 0usize;
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = normal(&entry.path().map_err(|e| e.to_string())?.to_string_lossy());
        members += 1;
        if members > MAX_COPY_MEMBERS {
            return Err(format!(
                "the upload has more than {MAX_COPY_MEMBERS} members, the most a COPY takes"
            ));
        }
        let is_dir = entry.header().entry_type().is_dir();
        let Some(target) = place(&path, is_dir) else {
            continue;
        };
        let mut header = entry.header().clone();
        header.set_uid(0);
        header.set_gid(0);
        // Nothing is sized from the header's claim: the buffer grows with
        // what the archive delivers, which the reader above also bounds.
        if entry.size() > limit {
            return Err(format!(
                "{path:?} claims {} bytes, more than a COPY takes",
                entry.size()
            ));
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(limit)
            .read_to_end(&mut data)
            .map_err(|e| format!("{path:?}: {e}"))?;
        match entry.header().entry_type() {
            kind @ (tar::EntryType::Symlink | tar::EntryType::Link) => {
                let mut link = entry
                    .link_name()
                    .map_err(|e| e.to_string())?
                    .map(|l| l.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // A hard link names another member by its path in the
                // archive, which moved with it: `tar -x -C /` resolves it
                // against `/`. (A symlink's target is the guest's business.)
                if kind == tar::EntryType::Link {
                    link = place(&normal(&link), false).ok_or_else(|| {
                        format!("{path:?} is a hard link to {link:?}, which this COPY leaves out")
                    })?;
                }
                out.append_link(&mut header, &target, &link)
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                out.append_data(&mut header, &target, &data[..])
                    .map_err(|e| e.to_string())?;
            }
        }
        paths.push(format!("/{target}"));
    }
    if paths.is_empty() {
        return Err(format!("no files in the upload match {src:?}"));
    }
    let tar = out.into_inner().map_err(|e| e.to_string())?;
    Ok((tar, paths))
}

/// Builds in memory: a node restarted forgets them, as it forgets its
/// sandboxes; the templates they made are kept.
pub(crate) type Builds = parking_lot::Mutex<HashMap<String, Arc<Build>>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn limits() -> UploadLimits {
        UploadLimits {
            one: 1024,
            count: 2,
            bytes: 4096,
            verify_hash: true,
        }
    }

    async fn put(
        dir: &std::path::Path,
        name: &str,
        bytes: &[u8],
        limits: &UploadLimits,
    ) -> Result<(), UploadError> {
        receive(
            Body::from(bytes.to_vec()),
            &dir.join(format!("{name}.tar")),
            name,
            limits,
        )
        .await
    }

    #[tokio::test]
    async fn an_upload_must_hash_to_its_name() {
        let dir = tempdir::Dir::new();
        let good = sha(b"payload");
        put(&dir.path, &good, b"payload", &limits()).await.unwrap();
        let other = sha(b"other");
        let err = put(&dir.path, &other, b"payload", &limits())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::UNPROCESSABLE_ENTITY, "{err:?}");
        assert!(!dir.path.join(format!("{other}.tar")).exists());
        // The operator's opt-out takes it.
        let lax = UploadLimits {
            verify_hash: false,
            ..limits()
        };
        put(&dir.path, &other, b"payload", &lax).await.unwrap();
    }

    #[tokio::test]
    async fn a_second_upload_to_a_name_does_not_replace_the_first() {
        let dir = tempdir::Dir::new();
        let lax = UploadLimits {
            verify_hash: false,
            ..limits()
        };
        put(&dir.path, "name", b"first", &lax).await.unwrap();
        put(&dir.path, "name", b"second", &lax).await.unwrap();
        assert_eq!(std::fs::read(dir.path.join("name.tar")).unwrap(), b"first");
        assert_eq!(stored_uploads(&dir.path), (1, 5), "no scratch left behind");
    }

    #[tokio::test]
    async fn an_upload_over_the_cap_is_refused_with_413() {
        let dir = tempdir::Dir::new();
        let big = vec![7u8; 2048];
        let err = put(&dir.path, &sha(&big), &big, &limits())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(stored_uploads(&dir.path), (0, 0), "no scratch left behind");
    }

    #[tokio::test]
    async fn the_node_holds_a_bounded_number_and_volume_of_uploads() {
        let dir = tempdir::Dir::new();
        for n in 0..2u8 {
            let bytes = vec![n; 10];
            put(&dir.path, &sha(&bytes), &bytes, &limits())
                .await
                .unwrap();
        }
        let more = vec![9u8; 10];
        let err = put(&dir.path, &sha(&more), &more, &limits())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::INSUFFICIENT_STORAGE);
        assert!(check_quota(&dir.path, &limits()).is_err());
        // Bytes bind too, with room in the count.
        let tight = UploadLimits {
            count: 100,
            bytes: 15,
            ..limits()
        };
        assert_eq!(
            put(&dir.path, &sha(&more), &more, &tight)
                .await
                .unwrap_err()
                .0,
            StatusCode::INSUFFICIENT_STORAGE
        );
    }

    #[test]
    fn a_token_opens_one_template_and_hash_and_no_other() {
        let grants = UploadGrants::default();
        let now = Instant::now();
        let token = grants.offer("team:red", "app", "h1", now);
        let grant = grants
            .redeem(&token, "app", "h1", now)
            .expect("the upload it was offered for");
        assert_eq!(grant.scope, "team:red");
        assert!(grants.redeem(&token, "other", "h1", now).is_none());
        assert!(grants.redeem(&token, "app", "h2", now).is_none());
        assert!(grants.redeem(&"0".repeat(64), "app", "h1", now).is_none());
        let later = now + UPLOAD_TOKEN_TTL + Duration::from_secs(1);
        assert!(grants.redeem(&token, "app", "h1", later).is_none());
    }

    #[test]
    fn a_grant_is_used_up_by_its_upload() {
        let grants = UploadGrants::default();
        let now = Instant::now();
        let token = grants.offer("team:red", "app", "h1", now);
        let grant = grants.redeem(&token, "app", "h1", now).unwrap();
        grants.spend(&grant);
        assert!(grants.redeem(&token, "app", "h1", now).is_none());
    }

    #[test]
    fn another_scopes_offer_for_the_same_hash_voids_nothing() {
        let grants = UploadGrants::default();
        let now = Instant::now();
        let red = grants.offer("team:red", "app", "h1", now);
        let blue = grants.offer("team:blue", "app", "h1", now);
        assert_eq!(
            grants.redeem(&red, "app", "h1", now).unwrap().scope,
            "team:red"
        );
        assert_eq!(
            grants.redeem(&blue, "app", "h1", now).unwrap().scope,
            "team:blue"
        );
        // A later offer to the same scope, template and hash does replace it.
        let again = grants.offer("team:red", "app", "h1", now);
        assert!(grants.redeem(&red, "app", "h1", now).is_none());
        assert!(grants.redeem(&again, "app", "h1", now).is_some());
    }

    #[tokio::test]
    async fn one_scopes_upload_of_a_hash_does_not_stand_in_for_anothers() {
        let dir = tempdir::Dir::new();
        let lax = UploadLimits {
            verify_hash: false,
            ..limits()
        };
        let red = dir.path.join(stored_name("team:red", "h1"));
        let blue = dir.path.join(stored_name("team:blue", "h1"));
        assert_ne!(red, blue, "two scopes, two files");
        receive(Body::from(b"red's files".to_vec()), &red, "h1", &lax)
            .await
            .unwrap();
        // Blue's upload of the same hash is stored, not refused as present.
        receive(Body::from(b"blue's".to_vec()), &blue, "h1", &lax)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&red).unwrap(), b"red's files");
        assert_eq!(std::fs::read(&blue).unwrap(), b"blue's");
    }

    #[tokio::test]
    async fn the_normal_flow_offer_upload_then_copy_reads_it() {
        let dir = upload(&[("main.txt", Some(b"hello"))]);
        let tar = std::fs::read(dir.path.join("u.tar")).unwrap();
        let hash = sha(&tar);
        let grants = UploadGrants::default();
        let now = Instant::now();
        let token = grants.offer("team:red", "app", &hash, now);
        let grant = grants
            .redeem(&token, "app", &hash, now)
            .expect("the link's own PUT");
        let store = tempdir::Dir::new();
        let stored = store.path.join(stored_name(&grant.scope, &grant.hash));
        let roomy = UploadLimits {
            one: 1 << 20,
            ..limits()
        };
        receive(Body::from(tar), &stored, &hash, &roomy)
            .await
            .unwrap();
        grants.spend(&grant);
        assert!(grants.redeem(&token, "app", &hash, now).is_none());
        // A COPY of the same scope reads what was stored.
        let (_, paths) = repack(&stored, "main.txt", "/srv/").unwrap();
        assert_eq!(paths, vec!["/srv/main.txt".to_string()]);
    }

    #[test]
    fn a_team_is_its_own_scope_and_an_administrator_has_none() {
        let red = hv2_cluster::ownership::TeamId::parse("red").unwrap();
        let blue = hv2_cluster::ownership::TeamId::parse("blue").unwrap();
        assert_eq!(scope_of(Some(&red)), "team:red");
        assert_ne!(scope_of(Some(&red)), scope_of(Some(&blue)));
        assert_eq!(scope_of(None), "");
    }

    #[test]
    fn a_build_cache_is_kept_per_scope() {
        let spec: StartBuild =
            serde_json::from_value(json!({"fromImage": "img", "steps": []})).unwrap();
        let red = step_cache_name(&spec, None, "team:red");
        assert_eq!(red, step_cache_name(&spec, None, "team:red"));
        assert_ne!(red, step_cache_name(&spec, None, "team:blue"));
        assert_ne!(red, step_cache_name(&spec, None, ""));
    }

    #[test]
    fn a_copy_clears_non_directories_first_and_gives_owner_and_mode_last() {
        use crate::env_vars::Step;
        let members = vec![
            Member::Dir {
                rel: "app".into(),
                mode: 0o755,
            },
            Member::File {
                rel: "app/a".into(),
                mode: 0o644,
                data: b"x".to_vec(),
            },
            Member::File {
                rel: "app/new".into(),
                mode: 0o600,
                data: b"y".to_vec(),
            },
            Member::Symlink {
                rel: "app/l".into(),
                target: "a".into(),
            },
        ];
        let replace: std::collections::HashSet<String> =
            ["/app/a".to_string(), "/app/l".to_string()].into();
        let paths = vec!["/app/a".to_string(), "/app/new".to_string()];
        let steps = copy_steps(&members, &replace, &paths, Some("node"), Some(0o640));
        let named: Vec<String> = steps
            .iter()
            .map(|s| match s {
                Step::Op { what, op } => format!("{}:{what}", hv2_guest_agent::op_name(op)),
                Step::Write { what, .. } => format!("write_file:{what}"),
            })
            .collect();
        assert_eq!(
            named,
            [
                "make_dir:making /app",
                "set_attr:setting the mode of /app",
                "remove:replacing /app/a",
                "write_file:writing /app/a",
                "set_attr:setting the mode of /app/a",
                "write_file:writing /app/new",
                "set_attr:setting the mode of /app/new",
                "make_dir:making /app",
                "remove:replacing /app/l",
                "make_symlink:linking /app/l",
                "set_attr:giving /app/a to node",
                "set_attr:giving /app/new to node",
            ]
        );
        // The removal is the one `clear_for_member` made: not recursive, and
        // quiet about a path that is already gone.
        assert!(matches!(
            &steps[2],
            Step::Op {
                op: hv2_guest_agent::Operation::Remove {
                    recursive: false,
                    missing_ok: true,
                    ..
                },
                ..
            }
        ));
        // Neither owner nor mode: nothing at the end.
        let plain = copy_steps(&members, &Default::default(), &paths, None, None);
        assert_eq!(
            plain.len(),
            steps.len() - 2 - 2,
            "no removes, no owner or mode"
        );
        // Mode only is a chmod.
        let mode_only = copy_steps(&members, &Default::default(), &paths, None, Some(0o700));
        assert!(matches!(
            mode_only.last(),
            Some(Step::Op { what, .. }) if what == "setting the mode of /app/new"
        ));
    }

    #[test]
    fn an_archive_that_repeats_a_path_is_not_batched() {
        let file = |rel: &str| Member::File {
            rel: rel.into(),
            mode: 0o644,
            data: Vec::new(),
        };
        assert!(!has_repeated_paths(&[file("a"), file("b")]));
        assert!(has_repeated_paths(&[
            file("a"),
            Member::Symlink {
                rel: "a".into(),
                target: "b".into()
            }
        ]));
        assert!(!has_repeated_paths(&[]));
    }

    fn upload(files: &[(&str, Option<&[u8]>)]) -> tempdir::Dir {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, data) in files {
            let mut header = tar::Header::new_gnu();
            match data {
                Some(data) => {
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_size(data.len() as u64);
                    header.set_mode(0o644);
                    header.set_uid(1000);
                    header.set_cksum();
                    builder.append_data(&mut header, path, *data).unwrap();
                }
                None => {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_cksum();
                    builder.append_data(&mut header, path, &[][..]).unwrap();
                }
            }
        }
        let dir = tempdir::Dir::new();
        std::fs::write(dir.path.join("u.tar"), builder.into_inner().unwrap()).unwrap();
        dir
    }

    #[test]
    fn a_hard_link_moves_with_the_file_it_names() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(2);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "hl/a.txt", &b"hi"[..])
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Link);
        header.set_size(0);
        builder
            .append_link(&mut header, "hl/b.txt", "hl/a.txt")
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Link);
        header.set_size(0);
        builder
            .append_link(&mut header, "hl/c.txt", "other/x")
            .unwrap();
        let dir = tempdir::Dir::new();
        let at = dir.path.join("u.tar");
        std::fs::write(&at, builder.into_inner().unwrap()).unwrap();

        // c.txt names a file the COPY leaves out: an error, not a dangling link.
        let err = repack(&at, "hl", "/opt/hl").unwrap_err();
        assert!(err.contains("leaves out"), "{err}");

        let mut builder = tar::Builder::new(Vec::new());
        let first = std::fs::read(&at).unwrap();
        let mut archive = tar::Archive::new(&first[..]);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().ends_with("c.txt") {
                continue;
            }
            let mut header = entry.header().clone();
            let path = entry.path().unwrap().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            builder.append_data(&mut header, path, &data[..]).unwrap();
        }
        std::fs::write(&at, builder.into_inner().unwrap()).unwrap();
        let (tar, paths) = repack(&at, "hl", "/opt/hl").unwrap();
        assert_eq!(paths, ["/opt/hl/a.txt", "/opt/hl/b.txt"]);
        let mut archive = tar::Archive::new(&tar[..]);
        let link = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap())
            .find(|e| e.header().entry_type() == tar::EntryType::Link)
            .unwrap();
        assert_eq!(link.path().unwrap().to_str(), Some("opt/hl/b.txt"));
        assert_eq!(
            link.link_name().unwrap().unwrap().to_str(),
            Some("opt/hl/a.txt")
        );
    }

    fn laid_out(src: &str, dest: &str, files: &[(&str, Option<&[u8]>)]) -> Vec<String> {
        let dir = upload(files);
        let (tar, mut paths) = repack(&dir.path.join("u.tar"), src, dest).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        for entry in archive.entries().unwrap() {
            assert_eq!(entry.unwrap().header().uid().unwrap(), 0, "owned by root");
        }
        paths.sort();
        paths
    }

    /// Docker's rules for where a COPY's files land.
    #[test]
    fn copies_land_where_docker_puts_them() {
        let file: &[(&str, Option<&[u8]>)] = &[("requirements.txt", Some(b"numpy\n"))];
        assert_eq!(
            laid_out("requirements.txt", "/app/", file),
            ["/app/requirements.txt"]
        );
        assert_eq!(
            laid_out("requirements.txt", "/app/reqs.txt", file),
            ["/app/reqs.txt"]
        );
        assert_eq!(
            laid_out("./requirements.txt", "/app/", file),
            ["/app/requirements.txt"]
        );

        let tree: &[(&str, Option<&[u8]>)] = &[
            ("src", None),
            ("src/main.py", Some(b"print(1)\n")),
            ("src/lib", None),
            ("src/lib/util.py", Some(b"x = 1\n")),
            ("other.txt", Some(b"no\n")),
        ];
        assert_eq!(
            laid_out("src", "/app", tree),
            ["/app", "/app/lib", "/app/lib/util.py", "/app/main.py"]
        );
        // A glob's upload holds its matches only: the SDK packs no more.
        let matched: &[(&str, Option<&[u8]>)] = &[("src/main.py", Some(b"print(1)\n"))];
        assert_eq!(laid_out("src/*.py", "/code/", matched), ["/code/main.py"]);
        let matched: &[(&str, Option<&[u8]>)] = &[("other.txt", Some(b"no\n"))];
        assert_eq!(laid_out("*.txt", "/etc/x/", matched), ["/etc/x/other.txt"]);
    }

    #[test]
    fn nothing_matching_is_an_error_not_an_empty_copy() {
        let dir = upload(&[("a.txt", Some(b"a"))]);
        assert!(repack(&dir.path.join("u.tar"), "b.txt", "/x/").is_err());
    }

    /// `ENV` values refer to what was set before them, as in Docker.
    #[test]
    fn env_values_expand_as_dockers_do() {
        let env: BTreeMap<String, String> = [("PATH", "/usr/bin"), ("V", "11")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        assert_eq!(expand("/opt/bin:$PATH", &env), "/opt/bin:/usr/bin");
        assert_eq!(expand("/jvm/jdk-${V}", &env), "/jvm/jdk-11");
        assert_eq!(expand("$UNSET-x", &env), "-x");
        assert_eq!(expand(r"cost \$5", &env), "cost $5");
        assert_eq!(expand("${V", &env), "${V");
    }

    #[test]
    fn commands_and_paths_as_a_step_writes_them() {
        assert_eq!(
            shell("id -u"),
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), "id -u".to_string()]
            )
        );
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(resolve("/home/user", "app"), "/home/user/app");
        assert_eq!(resolve("/", "app"), "/app");
        assert_eq!(resolve("/x", "/abs"), "/abs");
    }

    #[test]
    fn a_chmod_is_octal_or_the_shells() {
        assert_eq!(parse_octal_mode("755"), Some(0o755));
        assert_eq!(parse_octal_mode("0644"), Some(0o644));
        assert_eq!(parse_octal_mode("4755"), Some(0o4755));
        assert_eq!(parse_octal_mode("7"), Some(7));
        for bad in ["", "u+x", "758", "-1", "07555", "0x1f", " 755"] {
            assert_eq!(parse_octal_mode(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_user_is_in_passwd_by_name_or_uid() {
        let passwd =
            "root:x:0:0:root:/root:/bin/sh\napp:x:1000:1000::/srv/app:/bin/sh\nshort:x:5\n";
        assert!(passwd_has_user(passwd, "app"));
        assert!(passwd_has_user(passwd, "1000"));
        assert!(passwd_has_user(passwd, "root"));
        assert!(!passwd_has_user(passwd, "ap"));
        assert!(!passwd_has_user(passwd, "1001"));
        assert!(!passwd_has_user(passwd, ""));
        // A malformed line is no account.
        assert!(!passwd_has_user(passwd, "short"));
        assert!(!passwd_has_user("", "app"));
    }

    #[test]
    fn the_agents_environment_is_read_from_proc() {
        let environ = b"TERM=linux\0A=1=2\0=skipped\0noequals\0EMPTY=\0";
        let env = agent_environment(environ, b"42 (agentd) S 1 1");
        let want: BTreeMap<String, String> = [("TERM", "linux"), ("A", "1=2"), ("EMPTY", "")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        assert_eq!(env, want);
        // As PID 1 it sets PATH and HOME after starting, which environ lacks.
        let env = agent_environment(b"HOME=/\0TERM=linux\0", b"1 (hv2-guest-agent) S 0 1");
        assert_eq!(env["PATH"], "/bin");
        assert_eq!(env["HOME"], "/root");
        assert_eq!(env["TERM"], "linux");
        assert!(agent_environment(b"", b"").is_empty());
    }

    #[test]
    fn a_missing_shell_is_explained_at_start() {
        let e = "could not start /bin/sh: No such file or directory (os error 2)";
        let msg = start_failure(e);
        assert!(msg.contains("no /bin/sh") && msg.contains(e), "{msg}");
        assert_eq!(start_failure("boom"), "starting: boom");
    }

    /// What a COPY's repacked archive asks of the agent: the same members
    /// `repack` laid out, with their final paths below `/`.
    #[test]
    fn a_copys_archive_is_read_as_members_below_root() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(0o755);
        h.set_size(0);
        h.set_cksum();
        builder.append_data(&mut h, "app/lib", &[][..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(0o755);
        h.set_size(2);
        h.set_cksum();
        builder
            .append_data(&mut h, "app/lib/run.sh", &b"#!"[..])
            .unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_mode(0o777);
        h.set_size(0);
        h.set_cksum();
        builder.append_link(&mut h, "app/current", "lib").unwrap();
        let bytes = builder.into_inner().unwrap();
        let got = crate::env_vars::members_of(&bytes, "the COPY's files").unwrap();
        assert_eq!(
            got,
            [
                Member::Dir {
                    rel: "app/lib".into(),
                    mode: 0o755
                },
                Member::File {
                    rel: "app/lib/run.sh".into(),
                    mode: 0o755,
                    data: b"#!".to_vec()
                },
                Member::Symlink {
                    rel: "app/current".into(),
                    target: "lib".into()
                },
            ]
        );
        assert_eq!(join_guest("/", "app/lib"), "/app/lib");
        assert_eq!(join_guest("/", ""), "/");
    }

    /// A member written through a symlink an earlier one made is refused,
    /// naming the COPY; `..` and hard links are not the agent's.
    #[test]
    fn a_copys_archive_cannot_write_through_its_own_symlink() {
        let link = |b: &mut tar::Builder<Vec<u8>>, kind: tar::EntryType, name: &str, to: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(kind);
            h.set_mode(0o777);
            h.set_size(0);
            h.set_cksum();
            b.append_link(&mut h, name, to).unwrap();
        };
        let file = |b: &mut tar::Builder<Vec<u8>>, name: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Regular);
            h.set_mode(0o644);
            h.set_size(1);
            h.set_cksum();
            b.append_data(&mut h, name, &b"x"[..]).unwrap();
        };
        let mut b = tar::Builder::new(Vec::new());
        link(&mut b, tar::EntryType::Symlink, "app/etc", "/etc");
        file(&mut b, "app/etc/passwd");
        match crate::env_vars::members_of(&b.into_inner().unwrap(), "the COPY's files") {
            Err(Unpack::Failed(e)) => {
                assert!(
                    e.contains("the COPY's files") && e.contains("symlink"),
                    "{e}"
                );
            }
            other => panic!("{other:?}"),
        }
        let mut b = tar::Builder::new(Vec::new());
        link(&mut b, tar::EntryType::Link, "app/hard", "app/file");
        assert!(matches!(
            crate::env_vars::members_of(&b.into_inner().unwrap(), "x"),
            Err(Unpack::Shell(_))
        ));
    }

    /// An upload is read as a stream and bounded: a gzip that unpacks to more
    /// than the limit is refused, whole or as one member, and a header claiming
    /// a file far larger than the limit is refused without allocating it.
    #[test]
    fn an_upload_that_unpacks_too_large_is_refused() {
        use std::io::Write;
        let dir = tempdir::Dir::new();
        let mut builder = tar::Builder::new(Vec::new());
        let big = vec![0u8; 8 << 20];
        let mut header = tar::Header::new_gnu();
        header.set_size(big.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "zeros.bin", &big[..])
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        let gz = gz.finish().unwrap();
        assert!(gz.len() < 64 << 10, "a small bomb: {}", gz.len());
        let at = dir.path.join("bomb.tgz");
        std::fs::write(&at, &gz).unwrap();
        // Refused: by the member's own size here, and by the stream's total
        // for an archive of many small members (below).
        let err = repack_within(&at, ".", "/app/", 1 << 20).unwrap_err();
        assert!(err.contains("more than"), "{err}");
        // The same upload, within its limit, is fine -- gzip or plain.
        let (_, paths) = repack_within(&at, ".", "/app/", 16 << 20).unwrap();
        assert_eq!(paths, ["/app/zeros.bin"]);
        let plain = dir.path.join("plain.tar");
        std::fs::write(&plain, &tar).unwrap();
        assert!(repack_within(&plain, ".", "/app/", 1 << 20).is_err());
        assert!(repack_within(&plain, ".", "/app/", 16 << 20).is_ok());

        // Many members, each within the limit but together over it: the
        // stream's total stops it, not any one member.
        let mut builder = tar::Builder::new(Vec::new());
        for i in 0..64 {
            let chunk = vec![0u8; 64 << 10];
            let mut header = tar::Header::new_gnu();
            header.set_size(chunk.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("part-{i}"), &chunk[..])
                .unwrap();
        }
        let many = dir.path.join("many.tar");
        std::fs::write(&many, builder.into_inner().unwrap()).unwrap();
        let err = repack_within(&many, ".", "/app/", 1 << 20).unwrap_err();
        assert!(err.contains("unpacks to more than"), "{err}");
        assert!(repack_within(&many, ".", "/app/", 16 << 20).is_ok());

        // A header that claims 2^40 bytes: refused before anything is read.
        let mut header = tar::Header::new_gnu();
        header.set_path("huge").unwrap();
        header.set_mode(0o644);
        header.set_size(0);
        header.set_cksum();
        let mut block = header.as_bytes().to_vec();
        block[124] = 0x80;
        block[125..136].fill(0);
        block[128..136].copy_from_slice(&(1u64 << 40).to_be_bytes());
        block[148..156].fill(b' ');
        let sum: u32 = block.iter().map(|b| u32::from(*b)).sum();
        block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        block.extend_from_slice(b"tiny");
        let lying = dir.path.join("lying.tar");
        std::fs::write(&lying, &block).unwrap();
        let err = repack(&lying, ".", "/app/").unwrap_err();
        assert!(err.contains("claims"), "{err}");
    }

    /// A scratch directory, gone with the value.
    mod tempdir {
        pub struct Dir {
            pub path: std::path::PathBuf,
        }
        impl Dir {
            pub fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("hv2-builds-test-{}", uuid::Uuid::new_v4().simple()));
                std::fs::create_dir_all(&path).unwrap();
                Self { path }
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }
}
