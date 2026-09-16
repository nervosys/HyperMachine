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
//! It also now boots, per sandbox, a real per-VM `process.Process` gRPC
//! listener -- `hv2_api::envd_process`, the same service `envd_process.rs`
//! serves standalone -- rather than leaving `exec` as this file's only way
//! to run something. `POST /sandboxes` returns a non-standard `processPort`
//! field (E2B's own routing to a per-sandbox envd goes through a shared
//! proxy keyed by domain, which is not built here) naming where that
//! sandbox's real envd-shaped gRPC service is listening. An E2B SDK client
//! still cannot point at this today -- besides `processPort` not being a
//! real E2B field, several RPCs are `unimplemented` and streaming is
//! batched, not live; see `hv2_api::envd_process`'s doc comment. `exec`
//! remains as the simpler non-gRPC path for a plain `curl` test.
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
//! PORT=$(jq -r .processPort /tmp/sbx.json)
//! curl -s -X POST localhost:3980/sandboxes/$SBX/exec -d '{"cmd":"echo hello from a real microVM"}'
//! grpcurl -plaintext -import-path crates/hv2-api/proto -proto process.proto \
//!   -d '{"process":{"cmd":"/bin/sh","args":["-c","echo via envd-shaped grpc"]}}' \
//!   localhost:$PORT process.Process/Start
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
use hv2_api::envd_process::serve_for_sandbox;
use hv2_api::sandbox_proxy::{self, PortMap};

const GUEST_CID_BASE: u64 = 100;
/// First port handed to a sandbox's own `process.Process` listener.
/// Incremented per sandbox -- fine for a demo server; a real one would
/// need to handle exhaustion and reuse.
const PROCESS_PORT_BASE: u16 = 9000;

/// The port an E2B client asks for when it wants a sandbox's envd.
///
/// Not a port anything here binds: it is the number in the hostname
/// `{port}-{sandboxID}.{domain}`, which the proxy resolves to whichever local
/// port that sandbox's listener actually got. E2B's own envd listens on this,
/// so an SDK asks for it by name and should not have to know what it became.
const ENVD_PORT: u16 = 49983;

struct Options {
    port: u16,
    proxy_port: u16,
    /// Both must be given for the proxy to speak TLS; either alone is an
    /// error rather than a silent downgrade to plaintext.
    tls_cert: Option<String>,
    tls_key: Option<String>,
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
        proxy_port: 3981,
        tls_cert: None,
        tls_key: None,
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
            "--proxy-port" => {
                opts.proxy_port = value(&mut i)?.parse().map_err(|e| format!("{e}"))?;
            }
            "--tls-cert" => opts.tls_cert = Some(value(&mut i)?),
            "--tls-key" => opts.tls_key = Some(value(&mut i)?),
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

/// One booted sandbox: the VM handle, and the handle needed to shut down
/// its own `process.Process` gRPC listener when the sandbox is destroyed.
/// `templateID` is echoed back to the caller at creation time but nothing
/// here needs to remember it afterward -- there's no template store yet
/// (every sandbox boots the same `HV2_KERNEL`/`HV2_INITRD`), so tracking it
/// per-sandbox would be a field nothing reads.
struct LiveSandbox {
    vm: Arc<AgentVM>,
    process_shutdown: tokio::sync::oneshot::Sender<()>,
}

struct AppState {
    opts: Options,
    sandboxes: Mutex<HashMap<String, LiveSandbox>>,
    next_cid: Mutex<u64>,
    next_process_port: Mutex<u16>,
    /// What the proxy resolves a sandbox hostname to. Shared with the proxy
    /// task, which only reads it; every write happens here, on create and
    /// destroy, so a name stops resolving the moment its VM goes away.
    routes: Arc<PortMap>,
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
    /// Not a real E2B field. Real E2B routes to a per-sandbox envd through
    /// a shared proxy keyed by domain; this names where this specific
    /// sandbox's `process.Process` gRPC listener is bound instead, since
    /// that proxy isn't built here.
    #[serde(rename = "processPort")]
    process_port: u16,
    /// The hostname to send this sandbox's gRPC to, and where to send it.
    ///
    /// Both non-standard, like `processPort`. A real deployment would put the
    /// proxy behind DNS for `*.{domain}` and an SDK would need neither; with
    /// no DNS here, a client has to be told the authority to ask for and the
    /// address to connect to.
    #[serde(rename = "envdHost")]
    envd_host: String,
    #[serde(rename = "proxyPort")]
    proxy_port: u16,
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

    // Give this sandbox its own process.Process listener -- envd's real
    // shape, one daemon per sandbox, not one shared server multiplexing
    // by sandbox ID (see hv2_api::envd_process's doc comment).
    let process_port = {
        let mut next = state.next_process_port.lock();
        let port = *next;
        *next += 1;
        port
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let process_vm = Arc::clone(&vm);
    let process_addr: std::net::SocketAddr = format!("0.0.0.0:{process_port}").parse().unwrap();
    // What the proxy dials. The listener binds 0.0.0.0; the proxy reaches it
    // over loopback, and 0.0.0.0 is not an address you can connect *to*.
    let local_process_addr: std::net::SocketAddr =
        format!("127.0.0.1:{process_port}").parse().unwrap();
    let process_sandbox_id = sandbox_id.clone();
    tokio::spawn(async move {
        if let Err(e) = serve_for_sandbox(process_vm, process_addr, shutdown_rx).await {
            tracing::warn!("process.Process listener for {process_sandbox_id} stopped: {e}");
        }
    });

    // Resolvable by name before the sandbox is announced, so a client that
    // uses the response immediately does not race the registration.
    state
        .routes
        .insert(&sandbox_id, ENVD_PORT, local_process_addr);

    state.sandboxes.lock().insert(
        sandbox_id.clone(),
        LiveSandbox {
            vm,
            process_shutdown: shutdown_tx,
        },
    );

    // Built before the response moves `sandbox_id` into `client_id`.
    let envd_host = format!("{ENVD_PORT}-{sandbox_id}");

    (
        StatusCode::CREATED,
        Json(SandboxResponse {
            template_id,
            sandbox_id: sandbox_id.clone(),
            client_id: sandbox_id,
            envd_version: "hv2-guest-agentd/0 (not envd)".to_string(),
            process_port,
            envd_host,
            proxy_port: state.opts.proxy_port,
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
            // Stop resolving the name first: a request that arrives during
            // teardown should fail to route rather than be sent at a VM that
            // is in the middle of stopping.
            state.routes.remove_sandbox(&sandbox_id);
            let _ = live.process_shutdown.send(());
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
    let proxy_port = opts.proxy_port;
    let tls_cert = opts.tls_cert.clone();
    let tls_key = opts.tls_key.clone();

    let routes = Arc::new(PortMap::new());
    let state = Arc::new(AppState {
        opts,
        sandboxes: Mutex::new(HashMap::new()),
        next_cid: Mutex::new(0),
        next_process_port: Mutex::new(PROCESS_PORT_BASE),
        routes: Arc::clone(&routes),
    });

    // The proxy, on its own port beside the control plane.
    //
    // `_proxy_shutdown` is load-bearing despite the name: a oneshot receiver
    // resolves when its sender is dropped, so dropping this stops the proxy.
    // Holding it until `main` returns is what keeps the proxy up. Deleting the
    // binding as unused would take the proxy down before the first request --
    // which is exactly how the proxy's own tests failed the first time they
    // were written.
    let (_proxy_shutdown, proxy_rx) = tokio::sync::oneshot::channel();
    let proxy_addr: std::net::SocketAddr = match format!("0.0.0.0:{proxy_port}").parse() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("e2b_compat: bad proxy port {proxy_port}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let tls = match (&tls_cert, &tls_key) {
        (Some(cert), Some(key)) => {
            match sandbox_proxy::tls_config(std::path::Path::new(cert), std::path::Path::new(key)) {
                Ok(config) => Some(config),
                Err(e) => {
                    eprintln!("e2b_compat: TLS: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
        (None, None) => None,
        // Half a TLS configuration is a mistake, and starting in plaintext
        // because one flag was missing is the wrong way to report it.
        _ => {
            eprintln!("e2b_compat: --tls-cert and --tls-key must be given together");
            return std::process::ExitCode::FAILURE;
        }
    };
    let scheme = if tls.is_some() {
        "HTTP/2 over TLS"
    } else {
        "HTTP/2"
    };

    tokio::spawn(async move {
        let result = match tls {
            Some(config) => sandbox_proxy::serve_tls(proxy_addr, routes, config, proxy_rx).await,
            None => sandbox_proxy::serve(proxy_addr, routes, proxy_rx).await,
        };
        if let Err(e) = result {
            tracing::error!("sandbox proxy on {proxy_addr} stopped: {e}");
        }
    });

    let app = Router::new()
        .route("/sandboxes", post(create_sandbox))
        .route("/sandboxes/{sandboxID}/exec", post(exec))
        .route("/sandboxes/{sandboxID}", delete(destroy_sandbox))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    println!("e2b_compat: listening on {addr}");
    println!(
        "  POST   /sandboxes                -- E2B-shaped (NewSandbox -> Sandbox), boots a real \
         VM and its own process.Process gRPC listener (port in the response's processPort)"
    );
    println!(
        "  POST   /sandboxes/{{id}}/exec     -- NOT E2B's envd protocol; a real exec_in_guest \
         (simpler than grpcurl-ing processPort)"
    );
    println!("  DELETE /sandboxes/{{id}}          -- stop the VM and its process.Process listener");
    println!();
    println!(
        "sandbox proxy on 0.0.0.0:{proxy_port} -- {scheme}, routed by the authority a client asks \
         for, as {ENVD_PORT}-<sandboxID>.<anything>"
    );
    println!(
        "  grpcurl -plaintext -authority {ENVD_PORT}-$SBX.local -import-path \
         crates/hv2-api/proto -proto process.proto \\"
    );
    println!("      -d '{{\"process\":{{\"cmd\":\"/bin/sh\",\"args\":[\"-c\",\"echo hi\"]}}}}' \\");
    println!("      localhost:{proxy_port} process.Process/Start");

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
