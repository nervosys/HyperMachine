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
//! It also boots, per sandbox, a real per-VM envd endpoint --
//! `hv2_api::envd_process` and `hv2_api::envd_filesystem`, speaking both gRPC
//! and the Connect protocol on one port -- rather than leaving `exec` as this
//! file's only way to run something. `POST /sandboxes` returns a non-standard
//! `processPort` naming where that listener is bound, alongside the hostname
//! E2B's own SDK would use; `hv2_api::sandbox_proxy` resolves that hostname,
//! so an unmodified SDK does point at this. `exec` remains as the simpler
//! non-gRPC path for a plain `curl` test.
//!
//! # A sandbox's network
//!
//! Without `--network`, a guest has a vsock channel to its agent and nothing
//! else: no NIC, no route out. That remains the default, because it is the
//! strongest form of the control NVIDIA's sandboxing guidance puts first --
//! block outbound access to unknown destinations.
//!
//! With `--network`, every sandbox gets a virtio-net device whose far end is
//! an `hv2_net::gateway::Gateway`: a userspace router that decides each
//! connection and DNS query against the sandbox's policy and opens the
//! allowed ones from the host. The policy is E2B's own `NewSandbox` fields --
//! `allow_internet_access`, `network.allowOut`, `network.denyOut`, and
//! `network.rules` for header injection into HTTPS -- and
//! `PUT /sandboxes/{id}/network` replaces it on a running sandbox, as E2B's
//! does. A sandbox that configures nothing gets `--egress-default`, which is
//! `deny` unless the operator says otherwise; see `hv2_net::network_policy`
//! for that and the other deliberate differences from E2B.
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
//! TOKEN=$(jq -r .envdAccessToken /tmp/sbx.json)
//! curl -s -X POST localhost:3980/sandboxes/$SBX/exec -d '{"cmd":"echo hello from a real microVM"}'
//! grpcurl -plaintext -H "X-Access-Token: $TOKEN" -import-path crates/hv2-api/proto -proto process.proto \
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
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use hv2_api::envd_process::serve_for_sandbox;
use hv2_api::sandbox_proxy::{self, PortMap};
use hv2_net::gateway::socks::Socks5Proxy;
use hv2_net::gateway::{mitm::Authority, Gateway, GatewayConfig, GatewayHandle};
use hv2_net::network_policy::{Headers, NetworkPolicy, Verdict};

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

/// What `envdVersion` reports.
///
/// It has to be a version number, not a description: the SDK parses this
/// with PEP 440 and compares it against thresholds to decide which features
/// to use. The honest string this used to return -- "hv2-guest-agentd/0 (not
/// envd)" -- made every `Sandbox.connect()` raise `InvalidVersion` before it
/// did anything else.
///
/// A single number cannot be accurate, because what is implemented here is
/// not a prefix of envd's history. The SDK's own thresholds, against what
/// this actually does:
///
/// | threshold | feature | here |
/// | --- | --- | --- |
/// | 0.1.4 | recursive watch | yes |
/// | 0.3.0 | stdin on commands | yes |
/// | 0.4.0 | default user | no users at all; the field is ignored |
/// | 0.5.2 | CloseStdin | yes |
/// | 0.5.7 | octet-stream upload | **no** -- no file-upload route exists |
/// | 0.6.2 | xattr file metadata | **no** -- `metadata` is always empty |
/// | 0.6.3 | entry info on watch events | yes |
///
/// 0.6.3 is chosen because the alternative is worse: a lower number would
/// turn off watch entry info, which really works, to avoid claiming upload
/// and xattrs, which fail gracefully anyway -- upload has no route at any
/// version, and absent metadata reads as empty rather than as an error.
/// Claiming less would cost a working feature and buy nothing.
const ENVD_VERSION: &str = "0.6.3";

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
    /// Give each sandbox a NIC behind a gateway. Off by default.
    network: bool,
    /// What a sandbox that configures no network policy gets.
    egress_default: Verdict,
    /// Accept an `egressProxy` on a private or internal address.
    allow_private_egress_proxy: bool,
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
        network: false,
        egress_default: Verdict::Deny,
        allow_private_egress_proxy: false,
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
            "--network" => opts.network = true,
            "--allow-private-egress-proxy" => opts.allow_private_egress_proxy = true,
            "--egress-default" => {
                opts.egress_default = match value(&mut i)?.as_str() {
                    "deny" => Verdict::Deny,
                    "allow" => Verdict::Allow,
                    other => return Err(format!("--egress-default is allow or deny, not {other}")),
                };
            }
            "--help" | "-h" => {
                println!(
                    "usage: e2b_compat [--port N] [--proxy-port N] [--memory-gb N] [--cpu-cores N] \
                     [--network [--egress-default deny|allow] [--allow-private-egress-proxy]] \
                     [--tls-cert F --tls-key F]"
                );
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
    /// What `POST /sandboxes` answered with.
    ///
    /// Kept so `POST /sandboxes/{id}/connect` can answer with exactly the
    /// same thing. The SDK calls that on every `Sandbox.connect()` and reads
    /// the reply as the sandbox's identity; reconstructing it from parts
    /// would be two descriptions of one sandbox, free to drift apart.
    descriptor: SandboxResponse,
    /// The sandbox's network, when the server gives it one.
    network: Option<LiveNetwork>,
}

struct LiveNetwork {
    gateway: GatewayHandle,
    /// The loop carrying frames between the guest's NIC and the gateway.
    bridge: tokio::task::JoinHandle<()>,
}

struct AppState {
    opts: Options,
    /// Signs the leaves the gateway presents when a rule injects headers.
    /// One per server process; its certificate is installed in each guest.
    authority: Option<Arc<Authority>>,
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
    /// Snake case in the spec, unlike every other field.
    allow_internet_access: Option<bool>,
    network: Option<SandboxNetworkConfig>,
}

/// `SandboxNetworkConfig`, and `SandboxNetworkUpdateConfig` -- the same
/// fields that matter here.
#[derive(Debug, Default, Deserialize)]
struct SandboxNetworkConfig {
    #[serde(rename = "allowOut", default)]
    allow_out: Vec<String>,
    #[serde(rename = "denyOut", default)]
    deny_out: Vec<String>,
    #[serde(default)]
    rules: HashMap<String, Vec<SandboxNetworkRule>>,
    #[serde(rename = "egressProxy")]
    egress_proxy: Option<SandboxEgressProxyConfig>,
    /// Only on the update shape.
    allow_internet_access: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SandboxEgressProxyConfig {
    address: String,
    username: Option<String>,
    password: Option<String>,
}

/// The egress proxy a request names, checked the way E2B checks it: an
/// address that does not resolve, or resolves into a private or internal
/// range, is refused before the sandbox exists -- unless the operator has
/// said their proxy lives on such a network.
async fn egress_proxy_from(
    opts: &Options,
    network: Option<&SandboxNetworkConfig>,
) -> Result<Option<Socks5Proxy>, String> {
    let Some(config) = network.and_then(|n| n.egress_proxy.as_ref()) else {
        return Ok(None);
    };
    let proxy = Socks5Proxy::new(
        &config.address,
        config.username.as_deref(),
        config.password.as_deref(),
    )?;
    let addresses: Vec<_> = tokio::net::lookup_host(&proxy.address)
        .await
        .map_err(|e| format!("egress proxy {}: {e}", proxy.address))?
        .collect();
    if addresses.is_empty() {
        return Err(format!("egress proxy {} does not resolve", proxy.address));
    }
    if !opts.allow_private_egress_proxy
        && addresses.iter().any(|a| NetworkPolicy::is_reserved(a.ip()))
    {
        return Err(format!(
            "egress proxy {} resolves into a private or internal range \
             (start the server with --allow-private-egress-proxy if that is intended)",
            proxy.address
        ));
    }
    Ok(Some(proxy))
}

#[derive(Debug, Deserialize)]
struct SandboxNetworkRule {
    transform: Option<SandboxNetworkTransform>,
}

#[derive(Debug, Deserialize)]
struct SandboxNetworkTransform {
    #[serde(default)]
    headers: Headers,
}

/// E2B's fields as a policy, or the reason they cannot be one.
fn policy_from(
    allow_internet_access: Option<bool>,
    network: Option<&SandboxNetworkConfig>,
    operator_default: Verdict,
) -> Result<NetworkPolicy, String> {
    let empty = SandboxNetworkConfig::default();
    let network = network.unwrap_or(&empty);
    let mut rules = Vec::new();
    for (pattern, list) in &network.rules {
        // "Matching rule sets are not merged": a key's own list is one set,
        // so its transforms are applied in order, later ones overriding.
        let mut headers = Headers::new();
        for rule in list {
            if let Some(transform) = &rule.transform {
                headers.extend(transform.headers.clone());
            }
        }
        rules.push((pattern.clone(), headers));
    }
    NetworkPolicy::from_e2b(
        allow_internet_access.or(network.allow_internet_access),
        &network.allow_out,
        &network.deny_out,
        &rules,
        operator_default,
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, Clone)]
struct SandboxResponse {
    #[serde(rename = "templateID")]
    template_id: String,
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "clientID")]
    client_id: String,
    /// A version number, because the SDK parses it as one. See
    /// [`ENVD_VERSION`] for which capabilities that claims and which of
    /// those are real.
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
    /// Required on every request to this sandbox's envd, as `X-Access-Token`.
    /// Always issued: E2B's v2 creation route is secure-only, and a sandbox
    /// reachable by anyone who learns its ID is not one worth issuing.
    #[serde(rename = "envdAccessToken")]
    envd_access_token: String,
}

/// An error in the shape E2B's own API returns.
///
/// `{"code": ..., "message": ...}`, which is what the SDK's generated client
/// parses for *every* non-2xx status. A body of any other shape -- including
/// axum's own plain-text 404 -- makes it raise a `JSONDecodeError` from
/// inside its parser rather than reporting the status, which is how a missing
/// route here first showed up: as a stack trace ending in `json.decoder`,
/// naming nothing.
fn api_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "code": status.as_u16(), "message": message.to_string() })),
    )
        .into_response()
}

/// What `Sandbox.connect()` sends. Nothing here is used -- a sandbox that
/// already exists has its own timeout and memory -- but the SDK always sends
/// a body, and a handler that refused to decode one would refuse every
/// connect.
#[derive(Debug, Deserialize)]
struct ConnectSandbox {
    #[allow(dead_code)]
    timeout: Option<u64>,
    #[allow(dead_code)]
    memory: Option<u64>,
}

/// `POST /sandboxes/{id}/connect` -- attach to a sandbox that already exists.
///
/// The SDK calls this before anything else, so without it every
/// `Sandbox.connect()` fails, and the failure surfaced as a JSON parse error
/// rather than a 404 because the 404 body was not JSON.
async fn connect_sandbox(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<ConnectSandbox>>,
) -> Response {
    // The body is accepted and ignored, but a malformed one is still a
    // malformed request: `Option` here means "the SDK sent nothing", not
    // "anything goes".
    let _ = body;
    let descriptor = state
        .sandboxes
        .lock()
        .get(&sandbox_id)
        .map(|live| live.descriptor.clone());

    match descriptor {
        Some(descriptor) => (StatusCode::OK, Json(descriptor)).into_response(),
        None => api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
    }
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
    // Decided before anything boots, so a policy that does not parse costs a
    // 400 and not a VM.
    let wants_network = req.allow_internet_access == Some(true) || req.network.is_some();
    let policy = if state.opts.network {
        match policy_from(
            req.allow_internet_access,
            req.network.as_ref(),
            state.opts.egress_default,
        ) {
            Ok(policy) => Some(policy),
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        }
    } else if wants_network {
        return api_error(
            StatusCode::BAD_REQUEST,
            "this server gives sandboxes no network interface (start it with --network)",
        );
    } else {
        None
    };
    let egress_proxy = if policy.is_some() {
        match egress_proxy_from(&state.opts, req.network.as_ref()).await {
            Ok(proxy) => proxy,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        }
    } else {
        None
    };
    let gateway_config = GatewayConfig::default();

    let template_id = req.template_id.unwrap_or_else(|| "base".to_string());
    // A hyphen, not an underscore. This id becomes a DNS label -- the SDK
    // addresses a sandbox as `{port}-{sandboxID}.{domain}` -- and an
    // underscore is not legal in one. With `sbx_...` the SDK built a name its
    // own resolver then refused: "Label contains invalid characters". The
    // proxy splits on the *first* hyphen, so further hyphens are harmless.
    let sandbox_id = format!("sbx-{}", uuid_like());

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
                "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0 {}{}",
                hv2_core::BootSource::MICROVM_FAST_BOOT_ARGS,
                // The guest configures its NIC from this before init runs.
                if policy.is_some() {
                    format!(" {}", gateway_config.kernel_ip_arg())
                } else {
                    String::new()
                }
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
    // Attached before launch: virtio-mmio has no hotplug, and the kernel
    // learns where to probe from the command line `attach_net` extends.
    let nic = match &policy {
        Some(_) => {
            let [_, b, c, d] = u32::try_from(cid).unwrap_or(u32::MAX).to_be_bytes();
            match vm.vm().attach_net([0x52, 0x54, 0x00, b, c, d]).await {
                Ok(device) => Some(device),
                Err(e) => {
                    return api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("attaching the network device: {e}"),
                    );
                }
            }
        }
        None => None,
    };
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

    let network = match (policy, nic) {
        (Some(policy), Some(device)) => {
            match start_network(&state, &vm, device, policy, egress_proxy, gateway_config).await {
                Ok(network) => Some(network),
                Err(e) => {
                    let _ = vm.stop().await;
                    return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
                }
            }
        }
        _ => None,
    };

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
    // 244 bits from the OS RNG, via two v4 UUIDs.
    let access_token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let listener_token = Some(access_token.clone());
    let process_vm = Arc::clone(&vm);
    let process_addr: std::net::SocketAddr = format!("0.0.0.0:{process_port}").parse().unwrap();
    // What the proxy dials. The listener binds 0.0.0.0; the proxy reaches it
    // over loopback, and 0.0.0.0 is not an address you can connect *to*.
    let local_process_addr: std::net::SocketAddr =
        format!("127.0.0.1:{process_port}").parse().unwrap();
    let process_sandbox_id = sandbox_id.clone();
    tokio::spawn(async move {
        if let Err(e) =
            serve_for_sandbox(process_vm, process_addr, listener_token, shutdown_rx).await
        {
            tracing::warn!("process.Process listener for {process_sandbox_id} stopped: {e}");
        }
    });

    // Resolvable by name before the sandbox is announced, so a client that
    // uses the response immediately does not race the registration.
    state
        .routes
        .insert(&sandbox_id, ENVD_PORT, local_process_addr);

    // Built once and kept, so `POST /sandboxes/{id}/connect` answers with the
    // same description rather than a second one assembled from parts.
    let descriptor = SandboxResponse {
        template_id,
        sandbox_id: sandbox_id.clone(),
        client_id: sandbox_id.clone(),
        envd_version: ENVD_VERSION.to_string(),
        process_port,
        envd_host: format!("{ENVD_PORT}-{sandbox_id}"),
        proxy_port: state.opts.proxy_port,
        envd_access_token: access_token,
    };

    state.sandboxes.lock().insert(
        sandbox_id,
        LiveSandbox {
            vm,
            process_shutdown: shutdown_tx,
            descriptor: descriptor.clone(),
            network,
        },
    );

    (StatusCode::CREATED, Json(descriptor)).into_response()
}

/// Put a gateway behind a sandbox's NIC, and point the guest at it.
async fn start_network(
    state: &AppState,
    vm: &Arc<AgentVM>,
    device: Arc<parking_lot::Mutex<hv2_core::devices::virtio_net_mmio::VirtioNetMmio>>,
    policy: NetworkPolicy,
    egress_proxy: Option<Socks5Proxy>,
    config: GatewayConfig,
) -> Result<LiveNetwork, String> {
    let mut builder = Gateway::builder(policy).config(config);
    if let Some(authority) = &state.authority {
        builder = builder.intercept_with(Arc::clone(authority));
    }
    let gateway = builder
        .build()
        .map_err(|e| format!("starting the gateway: {e}"))?;
    let handle = gateway.handle();
    handle.set_egress_proxy(egress_proxy);

    // `allow_all` on the bridge because the gateway is the enforcement point:
    // it sees the guest's ARP, which a frame-level policy refuses by design,
    // and it decides every connection with more than a frame to go on.
    let bridge = hv2_net::bridge::Bridge::new(
        device,
        gateway,
        None,
        hv2_net::egress::EgressPolicy::allow_all(),
    );
    let bridge = tokio::spawn(async move {
        if let Err(e) = bridge.run(Duration::from_millis(1)).await {
            tracing::warn!("sandbox network bridge stopped: {e}");
        }
    });

    // The kernel wrote the nameserver to /proc/net/pnp, in resolv.conf's own
    // format. The CA goes where OpenSSL, curl and Python's ssl look by
    // default, so injection works for a client that was not told about it.
    // `mkdir -p /etc` because a minimal initramfs need not have one -- the
    // reference image here does not, which is how this was found.
    let mut script = String::from("mkdir -p /etc && ln -sf /proc/net/pnp /etc/resolv.conf");
    if let Some(ca) = handle.ca_pem() {
        script.push_str(&format!(
            " && mkdir -p /etc/ssl/certs && printf '%s' '{ca}' >> /etc/ssl/certs/ca-certificates.crt"
        ));
    }
    let setup = match vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".to_string(), script],
            Duration::from_secs(10),
        )
        .await
    {
        Ok(setup) => setup,
        Err(e) => {
            bridge.abort();
            return Err(format!("configuring the guest's network: {e}"));
        }
    };
    if setup.exit_code != Some(0) {
        bridge.abort();
        return Err(format!(
            "configuring the guest's network exited {:?}: {}",
            setup.exit_code, setup.stderr
        ));
    }

    Ok(LiveNetwork {
        gateway: handle,
        bridge,
    })
}

/// `PUT /sandboxes/{id}/network` -- replace a running sandbox's egress rules.
///
/// The body is `SandboxNetworkUpdateConfig`: "Omitting field clears it", so
/// this builds a whole new policy rather than patching the old one.
async fn update_network(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    Json(update): Json<SandboxNetworkConfig>,
) -> Response {
    let gateway = {
        let sandboxes = state.sandboxes.lock();
        match sandboxes.get(&sandbox_id) {
            None => return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
            Some(live) => live.network.as_ref().map(|n| n.gateway.clone()),
        }
    };
    let Some(gateway) = gateway else {
        return api_error(StatusCode::BAD_REQUEST, "this sandbox has no network");
    };
    let policy = match policy_from(None, Some(&update), state.opts.egress_default) {
        Ok(policy) => policy,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    let proxy = match egress_proxy_from(&state.opts, Some(&update)).await {
        Ok(proxy) => proxy,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    // Both or neither: a policy applied with the old proxy, or the reverse,
    // is a configuration nobody asked for.
    gateway.set_policy(policy);
    gateway.set_egress_proxy(proxy);
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /sandboxes/{id}/network/decisions` -- not E2B's; what the gateway
/// allowed and refused, most recent last.
async fn network_decisions(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    let gateway = {
        let sandboxes = state.sandboxes.lock();
        match sandboxes.get(&sandbox_id) {
            None => return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
            Some(live) => live.network.as_ref().map(|n| n.gateway.clone()),
        }
    };
    let Some(gateway) = gateway else {
        return api_error(StatusCode::BAD_REQUEST, "this sandbox has no network");
    };
    let decisions: Vec<_> = gateway
        .decisions()
        .into_iter()
        .map(|d| {
            json!({
                "kind": d.kind,
                "destination": d.destination.to_string(),
                "name": d.name,
                "verdict": format!("{:?}", d.verdict).to_lowercase(),
                "reason": d.reason,
            })
        })
        .collect();
    let stats = gateway.stats();
    Json(json!({
        "decisions": decisions,
        "stats": {
            "connectionsAllowed": stats.connections_allowed,
            "connectionsRefused": stats.connections_refused,
            "dnsAnswered": stats.dns_answered,
            "dnsRefused": stats.dns_refused,
            "intercepted": stats.intercepted,
        }
    }))
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
            // Dropping the bridge drops the gateway, whose stack task ends
            // with it; open upstream connections close as their tasks see
            // the guest side go away.
            if let Some(network) = live.network {
                network.bridge.abort();
            }
            if let Err(e) = live.vm.stop().await {
                tracing::warn!("stopping sandbox {sandbox_id}: {e}");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        None => api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
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

    let authority = if opts.network {
        match Authority::generate() {
            Ok(authority) => Some(Arc::new(authority)),
            Err(e) => {
                eprintln!("e2b_compat: generating the egress CA: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let network_line = if opts.network {
        format!(
            "sandboxes get a network, egress default {:?} for a sandbox that configures none",
            opts.egress_default
        )
    } else {
        "sandboxes get no network interface (--network to change that)".to_string()
    };

    let routes = Arc::new(PortMap::new());
    let state = Arc::new(AppState {
        authority,
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
        // What current SDKs (2.51+) call: `NewSandboxV2`, the same fields
        // used here, secure-only -- which every sandbox here already is.
        .route("/v2/sandboxes", post(create_sandbox))
        .route("/sandboxes/{sandboxID}/connect", post(connect_sandbox))
        .route("/sandboxes/{sandboxID}/exec", post(exec))
        .route("/sandboxes/{sandboxID}", delete(destroy_sandbox))
        .route("/sandboxes/{sandboxID}/network", put(update_network))
        .route(
            "/sandboxes/{sandboxID}/network/decisions",
            get(network_decisions),
        )
        // Even "no such route" has to be JSON: the SDK parses the body of
        // every non-2xx reply before it looks at the status.
        .fallback(|uri: axum::http::Uri| async move {
            api_error(StatusCode::NOT_FOUND, format!("no route {uri}"))
        })
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
    println!(
        "  POST   /sandboxes/{{id}}/connect  -- attach to an existing sandbox; what the E2B SDK's \
         Sandbox.connect() calls"
    );
    println!("  DELETE /sandboxes/{{id}}          -- stop the VM and its process.Process listener");
    println!("  PUT    /sandboxes/{{id}}/network  -- replace a running sandbox's egress rules");
    println!("  GET    /sandboxes/{{id}}/network/decisions -- NOT E2B's; the gateway's audit log");
    println!("{network_line}");
    println!();
    println!(
        "sandbox proxy on 0.0.0.0:{proxy_port} -- {scheme}, routed by the authority a client asks \
         for, as {ENVD_PORT}-<sandboxID>.<anything>"
    );
    println!(
        "  grpcurl -plaintext -authority {ENVD_PORT}-$SBX.local -H \"X-Access-Token: $TOKEN\" \
         -import-path crates/hv2-api/proto -proto process.proto \\"
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
