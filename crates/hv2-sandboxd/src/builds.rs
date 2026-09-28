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
    api_error, bring_up, build_from_image, create_park, new_access_token, policy_from, reserve,
    sizes_of, snapshots, valid_template_name, AppState, Arc, Deserialize, IntoResponse, Json,
    NetworkSpec, Path, Query, Response, Running, Sizes, State, StatusCode,
};
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
/// Per agent call, not per step.
const AGENT_TIMEOUT: Duration = Duration::from_secs(60);

/// One template build: waiting for its steps, building, then done.
pub(crate) struct Build {
    template: String,
    build_id: String,
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
    Json(req): Json<RequestBuild>,
) -> Response {
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

/// Where a `COPY`'s upload for `hash` is kept: in the store when there is
/// one, so a retried build on another node finds it.
fn upload_path(state: &AppState, hash: &str) -> Option<std::path::PathBuf> {
    let valid = !hash.is_empty()
        && hash.len() <= 128
        && hash
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let dir = match &state.store {
        Some(store) => store.dir.join("uploads"),
        None => state.suspend_dir.join("uploads"),
    };
    valid.then(|| dir.join(format!("{hash}.tar")))
}

/// `GET /templates/{id}/files/{hash}`: whether a `COPY`'s files are here;
/// if not, a URL to `PUT` them to, good for that one upload. The URL is
/// this node's, as the caller reached it -- through a control plane, the
/// control plane's, which passes the upload on.
pub(crate) async fn file_link(
    State(state): State<Arc<AppState>>,
    Path((template, hash)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(path) = upload_path(&state, &hash) else {
        return api_error(StatusCode::BAD_REQUEST, format!("a files hash {hash:?}"));
    };
    if path.exists() {
        return (StatusCode::CREATED, Json(json!({ "present": true }))).into_response();
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    state
        .upload_tokens
        .lock()
        .insert(hash.clone(), token.clone());
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
    let template = snapshots::untagged(&template);
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
/// sends none with an upload, so the token from `file_link` stands in.
pub(crate) async fn upload(
    State(state): State<Arc<AppState>>,
    Path((_template, hash)): Path<(String, String)>,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> Response {
    use subtle::ConstantTimeEq;
    let expected = state.upload_tokens.lock().get(&hash).cloned();
    let authorized =
        expected.is_some_and(|t| bool::from(t.as_bytes().ct_eq(query.token.as_bytes())));
    if !authorized {
        return api_error(StatusCode::FORBIDDEN, "not an upload this node offered");
    }
    let Some(path) = upload_path(&state, &hash) else {
        return api_error(StatusCode::BAD_REQUEST, format!("a files hash {hash:?}"));
    };
    match receive(body, &path).await {
        Ok(()) => {
            state.upload_tokens.lock().remove(&hash);
            StatusCode::OK.into_response()
        }
        Err(e) => api_error(StatusCode::BAD_REQUEST, e),
    }
}

/// Streamed to a scratch file beside `path`, then renamed onto it: a build
/// never reads half an upload.
async fn receive(body: Body, path: &std::path::Path) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;
    let dir = path.parent().ok_or("an upload path with no directory")?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let scratch = dir.join(format!(".{}", uuid::Uuid::new_v4().simple()));
    let mut file = tokio::fs::File::create(&scratch)
        .await
        .map_err(|e| format!("{}: {e}", scratch.display()))?;
    let mut stream = body.into_data_stream();
    let mut total = 0u64;
    let written: Result<(), String> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            total += chunk.len() as u64;
            if total > MAX_UPLOAD_BYTES {
                return Err(format!("larger than {MAX_UPLOAD_BYTES} bytes"));
            }
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        file.flush().await.map_err(|e| e.to_string())
    }
    .await;
    match written.and(
        tokio::fs::rename(&scratch, path)
            .await
            .map_err(|e| e.to_string()),
    ) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = tokio::fs::remove_file(&scratch).await;
            Err(e)
        }
    }
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

async fn run(state: &Arc<AppState>, build: &Build, spec: StartBuild) -> Result<(), Failure> {
    let base_step = at("base");
    // The base: an image, pulled into a template of its own; or a template
    // -- one built from an image, or a snapshot, restored as it was left.
    let (base, from_snapshot) = if let Some(image) = &spec.from_image {
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
        let policy =
            policy_from(Some(true), None, state.opts.egress_default).map_err(&base_step)?;
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
        &new_access_token(),
    )
    .await
    .map_err(|(_, e)| base_step(e))?;
    drop(from_snapshot);
    build.log("info", Some("base"), format!("Building in {sandbox_id}"));

    let outcome = steps_then_snapshot(state, build, &running, &spec, &base, &sandbox_id).await;
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
/// it left.
struct Context {
    env: BTreeMap<String, String>,
    cwd: String,
    user: String,
}

async fn steps_then_snapshot(
    state: &AppState,
    build: &Build,
    running: &Running,
    spec: &StartBuild,
    base: &str,
    sandbox_id: &str,
) -> Result<(), Failure> {
    let vm = &running.vm;
    // A template built from one built by steps starts where that one ended.
    let inherited: TemplateDefaults = vm
        .read_file_in_guest(TEMPLATE_DEFAULTS_PATH, 1 << 20, AGENT_TIMEOUT)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut context = Context {
        env: inherited.env,
        cwd: inherited.cwd.unwrap_or_else(|| "/".into()),
        user: "root".into(),
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
                    .map_err(&fail)?;
            }
            "ENV" => {
                for pair in step.args.chunks(2) {
                    if let [key, value] = pair {
                        context.env.insert(key.clone(), value.clone());
                    }
                }
            }
            "WORKDIR" => {
                let dir = step
                    .args
                    .first()
                    .ok_or_else(|| fail("WORKDIR without a path".into()))?;
                let dir = resolve(&context.cwd, dir);
                let owner = (context.user != "root")
                    .then(|| format!(" && chown {} {}", quote(&context.user), quote(&dir)));
                let script = format!("mkdir -p {}{}", quote(&dir), owner.unwrap_or_default());
                run_command(vm, build, &name, &script, "root", &context)
                    .await
                    .map_err(&fail)?;
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
            other => return Err(fail(format!("a step of type {other:?} is not supported"))),
        }
    }

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

    if let Some(command) = &spec.start_cmd {
        build.log("info", Some("finalize"), format!("Starting: {command}"));
        let (program, args) = shell(command);
        vm.start_in_guest_as(
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
    }
    if let Some(check) = &spec.ready_cmd {
        build.log("info", Some("finalize"), format!("Waiting for: {check}"));
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
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
                Ok(pid) => finished(vm, pid, None)
                    .await
                    .is_ok_and(|code| code == Some(0)),
                Err(_) => false,
            };
            if passed {
                break;
            }
            if Instant::now() > deadline {
                return Err(finalize(format!(
                    "the ready check {check:?} did not pass in {READY_TIMEOUT:?}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

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
        .map_err(|e| format!("starting: {e}"))?;
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
    let u = quote(user);
    let script = format!(
        "id -u {u} >/dev/null 2>&1 || {{ \
           if command -v useradd >/dev/null 2>&1; then useradd -m -s /bin/sh {u}; \
           else /bin/busybox adduser -D -h /home/{user} -s /bin/sh {u}; fi; }}"
    );
    run_command(vm, build, step, &script, "root", context).await
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
    let upload = upload_path(state, hash).ok_or("a bad filesHash")?;
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
    run_command(vm, build, step_name, &script, "root", context).await
}

/// The upload's entries for `src`, as a tar of their paths at `dest`, and
/// those paths.
fn repack(
    upload: &std::path::Path,
    src: &str,
    dest: &str,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let src = src.trim_start_matches("./").trim_end_matches('/');
    let bytes = std::fs::read(upload).map_err(|e| format!("the upload: {e}"))?;
    let reader: Box<dyn Read> = if bytes.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(&bytes[..]))
    } else {
        Box::new(&bytes[..])
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

    let mut out = tar::Builder::new(Vec::new());
    let mut paths = Vec::new();
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let path = path
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        if path.split('/').any(|p| p == "..") {
            continue;
        }
        let is_dir = entry.header().entry_type().is_dir();
        let target = if glob {
            let below = if fixed.is_empty() {
                path.as_str()
            } else {
                match path.strip_prefix(&format!("{fixed}/")) {
                    Some(below) => below,
                    None => continue,
                }
            };
            format!("{dest}/{below}")
        } else if path == src {
            if is_dir {
                // The directory itself is dest; its contents come after.
                dest.to_string()
            } else if into_dir {
                let file = path.rsplit('/').next().unwrap_or(&path);
                format!("{dest}/{file}")
            } else {
                dest.to_string()
            }
        } else if let Some(below) = path.strip_prefix(&format!("{src}/")) {
            format!("{dest}/{below}")
        } else if src == "." || src.is_empty() {
            format!("{dest}/{path}")
        } else {
            continue;
        };
        let target = target.trim_start_matches('/').to_string();
        if target.is_empty() {
            continue;
        }
        let mut header = entry.header().clone();
        header.set_uid(0);
        header.set_gid(0);
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
        match entry.header().entry_type() {
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let link = entry
                    .link_name()
                    .map_err(|e| e.to_string())?
                    .map(|l| l.to_string_lossy().into_owned())
                    .unwrap_or_default();
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
