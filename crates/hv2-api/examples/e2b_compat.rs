//! Phase 1 of `docs/CUBESANDBOX_PARITY_ROADMAP.md`: a minimal slice of
//! E2B's REST API, backed by a real `hv2-agent` VM instead of a mock.
//!
//! # What this actually is
//!
//! E2B's real architecture splits into two protocols: a control-plane REST
//! API (`api.e2b.app`, `POST /sandboxes` etc. — `spec/openapi.yml` in
//! `e2b-dev/E2B`) for lifecycle, and a separate in-guest daemon ("envd")
//! the SDK talks to *directly* for code execution, using its own
//! Connect-RPC/REST protocol. This implements the control-plane shape for
//! sandbox creation/deletion against `NewSandbox`/`Sandbox` from that spec
//! (real field names: `templateID`, `sandboxID`, `clientID`,
//! `envdVersion`), backed by a real VM boot -- `POST /sandboxes` here boots
//! an actual `hv2-agent` guest, the same one `cold_start.rs` measures.
//!
//! It does **not** implement envd's actual wire protocol -- that's a
//! separate, larger reverse-engineering task the roadmap doesn't claim is
//! done. Instead, `/sandboxes/{id}/exec` is a custom (non-E2B) endpoint
//! that runs a command via `AgentVM::exec_in_guest`, to prove the same
//! underlying capability envd's `run_code` depends on: given a REST call, a
//! command actually executes inside a real, hardware-isolated microVM, and
//! the result comes back. An E2B SDK client cannot point at this today and
//! work unmodified -- that would need envd's real protocol implemented,
//! not this endpoint.
//!
//! # Running it
//!
//! ```text
//! HV2_KERNEL=/var/tmp/kbuild/bzImage HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run --release -p hv2-api --example e2b_compat -- --port 3980
//! ```
//!
//! Then, from another shell:
//!
//! ```text
//! curl -s -X POST localhost:3980/sandboxes -d '{"templateID":"base"}' | tee /tmp/sbx.json
//! SBX=$(jq -r .sandboxID /tmp/sbx.json)
//! curl -s -X POST localhost:3980/sandboxes/$SBX/exec -d '{"cmd":"echo hello from a real microVM"}'
//! curl -s -X DELETE localhost:3980/sandboxes/$SBX
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use hv2_agent::{AgentVM, Capability, CapabilitySet};

const GUEST_CID_BASE: u64 = 100;

struct Options {
    port: u16,
    kernel: String,
    initrd: String,
    memory_gb: u64,
    cpu_cores: u32,
    ready_timeout: Duration,
}

fn parse_options() -> Result<Options, String> {
    let kernel = std::env::var("HV2_KERNEL").map_err(|_| {
        "HV2_KERNEL must name a bzImage -- this server has no template store yet, \
                       only the one guest image cold_start.rs also uses"
            .to_string()
    })?;
    let initrd = std::env::var("HV2_INITRD")
        .map_err(|_| "HV2_INITRD must name an initramfs running hv2-guest-agentd".to_string())?;

    let mut opts = Options {
        port: 3980,
        kernel,
        initrd,
        memory_gb: 1,
        cpu_cores: 1,
        ready_timeout: Duration::from_secs(15),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match args[i].as_str() {
            "--port" => opts.port = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--memory-gb" => opts.memory_gb = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--cpu-cores" => opts.cpu_cores = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--help" | "-h" => {
                println!("usage: e2b_compat [--port N] [--memory-gb N] [--cpu-cores N]");
                std::process::exit(0);
            }
            other => return Err(format!("unrecognised argument {other}")),
        }
        i += 1;
    }
    Ok(opts)
}

/// One booted sandbox: the VM handle, and the identity fields an E2B
/// `Sandbox` response repeats on every later call.
struct LiveSandbox {
    vm: Arc<AgentVM>,
    template_id: String,
}

struct AppState {
    opts: Options,
    sandboxes: Mutex<HashMap<String, LiveSandbox>>,
    next_cid: Mutex<u64>,
}

// ── E2B wire shapes -- field names taken directly from e2b-dev/E2B's
// spec/openapi.yml (`NewSandbox`, `Sandbox` schemas), not invented. ──

#[derive(Debug, Deserialize)]
struct NewSandbox {
    #[allow(dead_code)]
    #[serde(rename = "templateID")]
    template_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SandboxResponse {
    #[serde(rename = "templateID")]
    template_id: String,
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "clientID")]
    client_id: String,
    #[serde(rename = "envdVersion")]
    envd_version: String,
}

#[derive(Debug, Deserialize)]
struct ExecRequest {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ExecResponse {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NewSandbox>,
) -> Response {
    let template_id = req.template_id.unwrap_or_else(|| "base".to_string());
    let sandbox_id = format!("sbx_{}", uuid_like());

    let cid = {
        let mut next = state.next_cid.lock();
        let cid = *next;
        *next += 1;
        cid
    };

    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);

    let build = AgentVM::builder()
        .name(sandbox_id.clone())
        .cpu_cores(state.opts.cpu_cores)
        .memory_gb(state.opts.memory_gb)
        .capabilities(capabilities)
        .boot_linux(
            &state.opts.kernel,
            Some(&state.opts.initrd),
            format!(
                "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0 {}",
                hv2_core::BootSource::MICROVM_FAST_BOOT_ARGS
            ),
        )
        .build()
        .await;
    let vm = match build {
        Ok(vm) => vm,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("building the VM: {e}") })),
            )
                .into_response();
        }
    };

    if let Err(e) = vm.attach_guest_channel(GUEST_CID_BASE + cid).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("attaching the guest channel: {e}") })),
        )
            .into_response();
    }
    if let Err(e) = vm.launch().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("launching: {e}") })),
        )
            .into_response();
    }
    // A caller creating a sandbox waits for one it can actually use --
    // returning before the guest agent answers would hand back a
    // sandboxID that fails the first real request against it.
    if let Err(e) = vm.ping_guest(state.opts.ready_timeout).await {
        let _ = vm.stop().await;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": format!("guest never became ready: {e}") })),
        )
            .into_response();
    }

    let vm = Arc::new(vm);
    state.sandboxes.lock().insert(
        sandbox_id.clone(),
        LiveSandbox {
            vm,
            template_id: template_id.clone(),
        },
    );

    (
        StatusCode::CREATED,
        Json(SandboxResponse {
            template_id,
            sandbox_id: sandbox_id.clone(),
            client_id: sandbox_id,
            envd_version: "hv2-guest-agentd/0 (not envd)".to_string(),
        }),
    )
        .into_response()
}

async fn exec(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Response {
    let vm = {
        let sandboxes = state.sandboxes.lock();
        match sandboxes.get(&sandbox_id) {
            Some(s) => Arc::clone(&s.vm),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": format!("no sandbox {sandbox_id}") })),
                )
                    .into_response();
            }
        }
    };

    let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(30));
    // Direct exec, not through a shell -- see AgentVM::exec_in_guest's own
    // doc comment. Run through /bin/sh -c only when the caller's "cmd" is
    // meant as a shell line, which is the common case for a code-exec
    // endpoint (E2B's own run_code passes a whole script, not argv).
    let result = vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".to_string(), req.cmd]
                .into_iter()
                .chain(req.args)
                .collect::<Vec<_>>(),
            timeout,
        )
        .await;

    match result {
        Ok(exec) => Json(ExecResponse {
            exit_code: exec.exit_code,
            stdout: exec.stdout,
            stderr: exec.stderr,
            timed_out: exec.timed_out,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("exec_in_guest: {e}") })),
        )
            .into_response(),
    }
}

async fn destroy_sandbox(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    let removed = state.sandboxes.lock().remove(&sandbox_id);
    match removed {
        Some(live) => {
            if let Err(e) = live.vm.stop().await {
                tracing::warn!("stopping sandbox {sandbox_id}: {e}");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("no sandbox {sandbox_id}") })),
        )
            .into_response(),
    }
}

/// A short, collision-resistant-enough id for a demo server. Not a real
/// UUID implementation -- this crate doesn't otherwise depend on `uuid`
/// and pulling it in for a demo binary's id string isn't worth the
/// dependency.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let opts = match parse_options() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("e2b_compat: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let port = opts.port;

    let state = Arc::new(AppState {
        opts,
        sandboxes: Mutex::new(HashMap::new()),
        next_cid: Mutex::new(0),
    });

    let app = Router::new()
        .route("/sandboxes", post(create_sandbox))
        .route("/sandboxes/{sandboxID}/exec", post(exec))
        .route("/sandboxes/{sandboxID}", delete(destroy_sandbox))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("e2b_compat: listening on {addr}");
    println!(
        "  POST   /sandboxes                -- E2B-shaped (NewSandbox -> Sandbox), boots a real VM"
    );
    println!(
        "  POST   /sandboxes/{{id}}/exec     -- NOT E2B's envd protocol; a real exec_in_guest"
    );
    println!("  DELETE /sandboxes/{{id}}          -- stop and drop the VM");

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("e2b_compat: could not bind {addr}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("e2b_compat: server error: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
