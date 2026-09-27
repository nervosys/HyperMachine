//! `hv2-sandboxd`: E2B's sandbox API and envd, for microVMs on one host.
//!
//! This began as `hv2-api/examples/e2b_compat.rs`, Phase 1 of
//! `docs/CUBESANDBOX_PARITY_ROADMAP.md`, and became a daemon in Phase 4 when
//! it gained what a node in a cluster needs: a capacity it enforces,
//! sandbox lifetimes (E2B's `timeout`), and registration with a shared store
//! so `hv2-control-plane` can schedule onto it. It runs the same alone.
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
//! # In a cluster
//!
//! With `--cluster-store redis://...`, the node joins a cluster: it
//! announces itself (API URL, proxy address, capacity, load) with a TTL it
//! renews, writes a record for every sandbox it runs and deletes it when the
//! sandbox ends, and refuses API calls that do not carry the cluster token --
//! so a node's port is not a way around the control plane's API key. See
//! `hv2_cluster`.
//!
//! # Pausing, resuming, forking
//!
//! E2B's lifecycle API, on sandboxes restored from the template: `pause`
//! writes a sandbox to disk as only the pages it changed since its restore
//! and releases its VM; `connect` (or `resume`) brings it back under the same
//! ID and token; `lifecycle.on_timeout = "pause"` pauses at the timeout
//! instead of killing; `auto_resume` lets a request through the proxy resume
//! it; and `fork` checkpoints a running sandbox in place and starts copies of
//! it. With `--evict-idle-after`, a full node pauses its longest-idle
//! `auto_resume` sandbox to make room, and parks requests for a slot rather
//! than refusing them -- many more sandboxes than VMs, the way Agent
//! Substrate multiplexes actors onto workers. Paused sandboxes live on this
//! node's disk, over this node's template, and end with the process.
//!
//! # Running it
//!
//! ```text
//! HV2_KERNEL=/var/tmp/kbuild/bzImage HV2_INITRD=/var/tmp/kbuild/initramfs.cpio.gz \
//!   cargo run --release -p hv2-sandboxd -- --port 3980
//! ```
//!
//! Then, from another shell (a v1 create lives 15 s unless `timeout` says
//! otherwise, as in E2B):
//!
//! ```text
//! curl -s -X POST localhost:3980/sandboxes -d '{"templateID":"base","timeout":300}' | tee /tmp/sbx.json
//! SBX=$(jq -r .sandboxID /tmp/sbx.json)
//! PORT=$(jq -r .processPort /tmp/sbx.json)
//! TOKEN=$(jq -r .envdAccessToken /tmp/sbx.json)
//! curl -s -X POST localhost:3980/sandboxes/$SBX/exec -d '{"cmd":"echo hello from a real microVM"}'
//! grpcurl -plaintext -H "X-Access-Token: $TOKEN" -import-path crates/hv2-api/proto -proto process.proto \
//!   -d '{"process":{"cmd":"/bin/sh","args":["-c","echo via envd-shaped grpc"]}}' \
//!   localhost:$PORT process.Process/Start
//! curl -s -X DELETE localhost:3980/sandboxes/$SBX
//! ```

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use hv2_agent::{AgentVM, Capability, CapabilitySet};
use hv2_api::sandbox_proxy::{self, PortMap};
use hv2_cluster::control::CLUSTER_TOKEN_HEADER;
use hv2_cluster::metrics::{Counter, Exposition, Histogram};
use hv2_cluster::model::{metadata_matches, now_ms, parse_metadata_query, SandboxRecord};
use hv2_cluster::node::{NodeAgent, NodeConfig};
use hv2_net::gateway::socks::Socks5Proxy;
use hv2_net::gateway::{mitm::Authority, Gateway, GatewayConfig, GatewayHandle};
use hv2_net::network_policy::{Cidr, Headers, NetworkPolicy, Verdict};

const GUEST_CID_BASE: u64 = 100;

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
    /// Reserved ranges a sandbox's `allowOut` may open. Empty: none may, so
    /// no tenant rule reaches this host's network or the cluster's store.
    tenant_reserved: Vec<Cidr>,
    /// Sandboxes this node runs at once. A create beyond it is refused with
    /// 503, which a control plane reads as "try another node".
    capacity: u32,
    /// Join a cluster through this store (`redis://...`).
    cluster_store: Option<String>,
    cluster_namespace: String,
    node_id: Option<String>,
    /// How a control plane reaches this node's API and proxy. Required in a
    /// cluster: the addresses this binds (0.0.0.0) are not ones to dial.
    advertise_api: Option<String>,
    /// `host:port`; a name is resolved once, at startup, since the store
    /// records an address.
    advertise_proxy: Option<String>,
    /// Required on every API call when set.
    cluster_token: Option<String>,
    node_ttl: Duration,
    /// Boot every sandbox instead of restoring it from a template.
    no_template: bool,
    /// Prefault a restored guest's working set. Off by default: it halves
    /// the page faults and exits a restore takes, and did not change create
    /// latency measurably on the nested-KVM host it was tried on, where it
    /// also costs a probe restore at startup and ~3 ms of each launch. Kept,
    /// and opt-in, for a bare-metal host to measure.
    prefault: bool,
    /// When full, pause a sandbox that resumes on traffic and has been idle
    /// this long, to make room.
    evict_idle_after: Option<Duration>,
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
        tenant_reserved: Vec::new(),
        capacity: 16,
        cluster_store: None,
        cluster_namespace: "default".to_string(),
        node_id: None,
        advertise_api: None,
        advertise_proxy: None,
        cluster_token: std::env::var("HV2_CLUSTER_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
        node_ttl: Duration::from_secs(9),
        no_template: false,
        prefault: false,
        evict_idle_after: None,
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
            "--no-template" => opts.no_template = true,
            "--prefault" => opts.prefault = true,
            "--capacity" => opts.capacity = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
            "--cluster-store" => opts.cluster_store = Some(value(&mut i)?),
            "--cluster-namespace" => opts.cluster_namespace = value(&mut i)?,
            "--node-id" => opts.node_id = Some(value(&mut i)?),
            "--advertise-api" => opts.advertise_api = Some(value(&mut i)?),
            "--advertise-proxy" => {
                opts.advertise_proxy = Some(value(&mut i)?);
            }
            "--cluster-token" => opts.cluster_token = Some(value(&mut i)?),
            "--node-ttl" => {
                opts.node_ttl =
                    Duration::from_secs(value(&mut i)?.parse().map_err(|e| format!("{e}"))?);
            }
            "--allow-private-egress-proxy" => opts.allow_private_egress_proxy = true,
            "--tenant-reserved-cidr" => {
                let text = value(&mut i)?;
                opts.tenant_reserved
                    .push(Cidr::parse(&text).map_err(|e| format!("--tenant-reserved-cidr: {e}"))?);
            }
            "--evict-idle-after" => {
                opts.evict_idle_after = Some(Duration::from_secs(
                    value(&mut i)?.parse().map_err(|e| format!("{e}"))?,
                ));
            }
            "--egress-default" => {
                opts.egress_default = match value(&mut i)?.as_str() {
                    "deny" => Verdict::Deny,
                    "allow" => Verdict::Allow,
                    other => return Err(format!("--egress-default is allow or deny, not {other}")),
                };
            }
            "--help" | "-h" => {
                println!(
                    "usage: hv2-sandboxd [--port N] [--proxy-port N] [--memory-gb N] [--cpu-cores N] \
                     [--capacity N] [--no-template] [--prefault] [--evict-idle-after SECS] \
                     [--network [--egress-default deny|allow] [--allow-private-egress-proxy] \
                     [--tenant-reserved-cidr CIDR]...] \
                     [--tls-cert F --tls-key F] \
                     [--cluster-store redis://H:P --advertise-api URL --advertise-proxy H:P \
                     [--node-id ID] [--cluster-namespace NS] [--cluster-token T] [--node-ttl SECS]]\n\
                     HV2_KERNEL and HV2_INITRD name the guest; HV2_CLUSTER_TOKEN may carry the token."
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
    /// Where that listener is, so a failed pause can route to it again.
    process_addr: std::net::SocketAddr,
    /// Its share of capacity, given back when it pauses or ends.
    _slot: Slot,
    /// What its timeout does, and whether traffic resumes it.
    lifecycle: Lifecycle,
    /// Requests in flight to it through the proxy, and when it was last used.
    activity: Arc<Activity>,
    /// What `POST /sandboxes` answered with.
    ///
    /// Kept so `POST /sandboxes/{id}/connect` can answer with exactly the
    /// same thing. The SDK calls that on every `Sandbox.connect()` and reads
    /// the reply as the sandbox's identity; reconstructing it from parts
    /// would be two descriptions of one sandbox, free to drift apart.
    descriptor: SandboxResponse,
    /// The sandbox's network, when the server gives it one.
    network: Option<LiveNetwork>,
    /// What listing, detail and the cluster store say about it -- including
    /// when it ends, which the expiry task enforces.
    record: SandboxRecord,
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
    /// What the proxy resolves a sandbox hostname to. Shared with the proxy
    /// task, which only reads it; every write happens here, on create and
    /// destroy, so a name stops resolving the moment its VM goes away.
    routes: Arc<PortMap>,
    /// Capacity, as permits: one held by every running sandbox and every
    /// one being brought up. Held from the moment a create is accepted, not
    /// from when its VM is up, or a burst of creates all see room for
    /// themselves. A semaphore because its waiters are served in order: a
    /// request parked for a slot is not overtaken, again and again, by
    /// creates that arrived after it.
    slots: Arc<tokio::sync::Semaphore>,
    /// This node's membership of a cluster, if it has one.
    node: Option<NodeAgent>,
    /// What sandboxes are restored from, when not booted.
    template: Option<Template>,
    metrics: NodeMetrics,
    /// Sandboxes suspended to disk, under `suspend_dir`.
    paused: Mutex<HashMap<String, PausedSandbox>>,
    suspend_dir: std::path::PathBuf,
    /// One lock per sandbox that has paused, resumed or forked.
    transitions: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// What `/metrics` reports beyond live gauges.
#[derive(Default)]
struct NodeMetrics {
    creates_ok: Counter,
    creates_full: Counter,
    creates_rejected: Counter,
    creates_error: Counter,
    create_latency: Histogram,
    ended_deleted: Counter,
    ended_expired: Counter,
    pauses: Counter,
    resumes: Counter,
    auto_resumes: Counter,
    evictions: Counter,
    forks_ok: Counter,
    forks_failed: Counter,
    pause_latency: Histogram,
    resume_latency: Histogram,
    checkpoint_latency: Histogram,
}

impl AppState {
    fn running(&self) -> u32 {
        u32::try_from(self.sandboxes.lock().len()).unwrap_or(u32::MAX)
    }
}

/// A slot against capacity, given back when dropped -- so a create that fails
/// halfway, by error or by panic, does not leak one, and a sandbox that
/// pauses or ends gives its back without being told to.
type Slot = tokio::sync::OwnedSemaphorePermit;

// ── E2B wire shapes -- field names taken directly from e2b-dev/E2B's
// spec/openapi.yml (`NewSandbox`, `Sandbox` schemas), not invented. ──

#[derive(Debug, Deserialize)]
struct NewSandbox {
    #[serde(rename = "templateID")]
    template_id: Option<String>,
    /// Seconds to live. The spec's default differs by route: 15 on
    /// `POST /sandboxes`, 300 on `POST /v2/sandboxes`.
    timeout: Option<u64>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    /// Snake case in the spec, unlike every other field.
    allow_internet_access: Option<bool>,
    network: Option<SandboxNetworkConfig>,
    #[serde(rename = "autoPause")]
    auto_pause: Option<bool>,
    #[serde(rename = "autoPauseMemory")]
    auto_pause_memory: Option<bool>,
    #[serde(rename = "autoResume")]
    auto_resume: Option<AutoResume>,
}

/// `SandboxAutoResumeConfig`.
#[derive(Debug, Deserialize)]
struct AutoResume {
    #[serde(default)]
    enabled: bool,
}

/// The longest a sandbox may be asked to live, in seconds: E2B's own limit
/// for its paid tier, 24 hours. A lifetime is a resource claim.
const MAX_TIMEOUT_SECS: u64 = 24 * 60 * 60;

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
    tenant_reserved: &[Cidr],
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
    .map(|policy| policy.with_tenant_reserved(tenant_reserved))
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

/// What `Sandbox.connect()` sends. `timeout` extends the sandbox's life --
/// "TTL is only extended", in the spec's words, never shortened.
#[derive(Debug, Deserialize)]
struct ConnectSandbox {
    timeout: Option<u64>,
    /// `false` asks to resume from disk alone, dropping memory -- which here
    /// is the filesystem too, so it is refused rather than approximated.
    memory: Option<bool>,
}

/// `POST /sandboxes/{id}/connect` -- attach to a sandbox that already exists,
/// resuming it first if it is paused.
///
/// The SDK calls this before anything else, so without it every
/// `Sandbox.connect()` fails, and the failure surfaced as a JSON parse error
/// rather than a 404 because the 404 body was not JSON.
async fn connect_sandbox(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<ConnectSandbox>>,
) -> Response {
    // `Option` means "the SDK sent nothing", not "anything goes": a
    // malformed body is still refused by the extractor.
    let body = body.map(|Json(b)| b);
    if body.as_ref().and_then(|b| b.memory) == Some(false) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "memory=false (resume from disk only) is not available here: a sandbox's \
             filesystem lives in its memory",
        );
    }
    let requested = body.and_then(|b| b.timeout);
    // Paused: resumed, and answered 201 rather than 200, as E2B does.
    // `Ok(None)` means it was running after all, and falls through.
    if state.paused.lock().contains_key(&sandbox_id) {
        match resume_sandbox(&state, &sandbox_id, Some(requested.unwrap_or(300)), None).await {
            Ok(Some(descriptor)) => return (StatusCode::CREATED, Json(descriptor)).into_response(),
            Ok(None) => {}
            Err((status, e)) => return api_error(status, e),
        }
    }
    let extend_to = requested.map(|secs| now_ms() + secs.min(MAX_TIMEOUT_SECS) * 1000);
    let found = {
        let mut sandboxes = state.sandboxes.lock();
        sandboxes.get_mut(&sandbox_id).map(|live| {
            let mut changed = None;
            if let Some(end) = extend_to {
                if end > live.record.end_at_ms {
                    live.record.end_at_ms = end;
                    changed = Some(live.record.clone());
                }
            }
            (live.descriptor.clone(), changed)
        })
    };

    match found {
        Some((descriptor, changed)) => {
            if let (Some(record), Some(node)) = (changed, &state.node) {
                if let Err(e) = node.updated(&record).await {
                    tracing::warn!("recording {sandbox_id}'s new end time: {e}");
                }
            }
            (StatusCode::OK, Json(descriptor)).into_response()
        }
        None => api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
    }
}

#[derive(Debug, Deserialize)]
struct TimeoutRequest {
    timeout: u64,
}

/// `POST /sandboxes/{id}/timeout` -- the sandbox now ends `timeout` seconds
/// from this request, whether that is sooner or later than before.
async fn set_timeout(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    Json(req): Json<TimeoutRequest>,
) -> Response {
    let record = {
        let mut sandboxes = state.sandboxes.lock();
        sandboxes.get_mut(&sandbox_id).map(|live| {
            live.record.end_at_ms = now_ms() + req.timeout.min(MAX_TIMEOUT_SECS) * 1000;
            live.record.clone()
        })
    };
    let Some(record) = record else {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"));
    };
    if let Some(node) = &state.node {
        if let Err(e) = node.updated(&record).await {
            tracing::warn!("recording {sandbox_id}'s new end time: {e}");
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    metadata: Option<String>,
    state: Option<String>,
}

/// `GET /sandboxes` and `GET /v2/sandboxes`, for this node alone: running
/// and paused, filtered by `state` and `metadata`.
async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Response {
    let wanted = query
        .metadata
        .as_deref()
        .map(parse_metadata_query)
        .unwrap_or_default();
    let wants = |record: &SandboxRecord| {
        query
            .state
            .as_deref()
            .is_none_or(|s| s.split(',').any(|s| s == record.state()))
            && metadata_matches(record, &wanted)
    };
    let mut records: Vec<SandboxRecord> = state
        .sandboxes
        .lock()
        .values()
        .map(|live| live.record.clone())
        .chain(state.paused.lock().values().map(|p| p.record.clone()))
        .filter(|r| wants(r))
        .collect();
    records.sort_by(|a, b| (a.started_at_ms, &a.sandbox_id).cmp(&(b.started_at_ms, &b.sandbox_id)));
    Json(
        records
            .iter()
            .map(SandboxRecord::listed)
            .collect::<Vec<_>>(),
    )
    .into_response()
}

/// `GET /sandboxes/{id}` -- E2B's `SandboxDetail`.
async fn sandbox_detail(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
) -> Response {
    let running = state
        .sandboxes
        .lock()
        .get(&sandbox_id)
        .map(|live| live.record.clone());
    let record = running.or_else(|| {
        state
            .paused
            .lock()
            .get(&sandbox_id)
            .map(|p| p.record.clone())
    });
    match record {
        Some(record) => Json(record.detail()).into_response(),
        None => api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")),
    }
}

/// Refuse any API call without the cluster token, when one is configured.
async fn require_cluster_token(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(token) = &state.opts.cluster_token {
        use subtle::ConstantTimeEq;
        let sent = request
            .headers()
            .get(CLUSTER_TOKEN_HEADER)
            .map(axum::http::HeaderValue::as_bytes)
            .unwrap_or_default();
        if !bool::from(sent.ct_eq(token.as_bytes())) {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "this node answers its cluster's control plane only",
            );
        }
    }
    next.run(request).await
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

/// `POST /sandboxes`: E2B's v1 route, whose default lifetime is 15 seconds.
async fn create_v1(State(state): State<Arc<AppState>>, Json(req): Json<NewSandbox>) -> Response {
    counted_create(state, req, 15).await
}

/// `POST /v2/sandboxes`: what current SDKs call, default lifetime 300 s.
async fn create_v2(State(state): State<Arc<AppState>>, Json(req): Json<NewSandbox>) -> Response {
    counted_create(state, req, 300).await
}

async fn counted_create(state: Arc<AppState>, req: NewSandbox, default_timeout: u64) -> Response {
    let started = std::time::Instant::now();
    let response = create_sandbox(Arc::clone(&state), req, default_timeout).await;
    let m = &state.metrics;
    match response.status() {
        StatusCode::CREATED => {
            m.creates_ok.inc();
            m.create_latency.observe(started.elapsed());
        }
        StatusCode::SERVICE_UNAVAILABLE => m.creates_full.inc(),
        s if s.is_client_error() => m.creates_rejected.inc(),
        _ => m.creates_error.inc(),
    }
    response
}

/// `GET /metrics`: Prometheus text format. Outside the cluster token, as a
/// scraper holds none; counts are all it says.
async fn node_metrics(State(state): State<Arc<AppState>>) -> Response {
    let m = &state.metrics;
    let mut e = Exposition::new();
    e.gauge(
        "hv2_node_sandboxes_running",
        "Sandboxes running on this node.",
        f64::from(state.running()),
    );
    e.gauge(
        "hv2_node_sandboxes_paused",
        "Sandboxes suspended to this node's disk.",
        state.paused.lock().len() as f64,
    );
    e.gauge("hv2_node_sandboxes_booting", "Creations in flight.", {
        let held = state.opts.capacity as usize - state.slots.available_permits();
        held.saturating_sub(state.running() as usize) as f64
    });
    e.gauge(
        "hv2_node_capacity",
        "Sandboxes this node will run at once.",
        f64::from(state.opts.capacity),
    );
    e.gauge(
        "hv2_node_template",
        "1 if sandboxes are restored from a template snapshot, 0 if booted.",
        if state.template.is_some() { 1.0 } else { 0.0 },
    );
    e.counters(
        "hv2_node_creates_total",
        "Creations on this node, by outcome.",
        "result",
        &[
            ("ok", m.creates_ok.get()),
            ("full", m.creates_full.get()),
            ("rejected", m.creates_rejected.get()),
            ("error", m.creates_error.get()),
        ],
    );
    e.histogram(
        "hv2_node_create_seconds",
        "Time from request to a sandbox whose agent answers.",
        &m.create_latency,
    );
    e.counters(
        "hv2_node_sandbox_ends_total",
        "Sandboxes that ended, by why.",
        "reason",
        &[
            ("deleted", m.ended_deleted.get()),
            ("expired", m.ended_expired.get()),
        ],
    );
    e.counters(
        "hv2_node_transitions_total",
        "Pauses, resumes (and those a request triggered), evictions to make room, and forks.",
        "kind",
        &[
            ("pause", m.pauses.get()),
            ("resume", m.resumes.get()),
            ("auto_resume", m.auto_resumes.get()),
            ("evict", m.evictions.get()),
            ("fork_ok", m.forks_ok.get()),
            ("fork_failed", m.forks_failed.get()),
        ],
    );
    e.histogram(
        "hv2_node_pause_seconds",
        "Time to suspend a running sandbox to disk.",
        &m.pause_latency,
    );
    e.histogram(
        "hv2_node_resume_seconds",
        "Time from a resume request to a sandbox whose agent answers.",
        &m.resume_latency,
    );
    e.histogram(
        "hv2_node_checkpoint_seconds",
        "Time a fork's source is paused to be checkpointed.",
        &m.checkpoint_latency,
    );
    (
        [("content-type", hv2_cluster::metrics::CONTENT_TYPE)],
        e.finish(),
    )
        .into_response()
}

/// A slot against capacity, made or waited for.
///
/// Queued in order behind any request already waiting for one. With
/// `--evict-idle-after`, a full node makes room by pausing the sandbox idle
/// longest among those that resume themselves on traffic -- nothing is lost,
/// since the next request to it brings it back. With `park`, a request that
/// still finds no room waits up to that long rather than being refused: what
/// a request to a paused sandbox wants, since failing it would mean the
/// caller retries anyway.
async fn reserve(state: &Arc<AppState>, park: Option<Duration>) -> Result<Slot, String> {
    let deadline = park.map(|p| tokio::time::Instant::now() + p);
    // In the queue from here on, so a slot freed at any point below goes to
    // whoever asked first.
    let mut acquire = Box::pin(Arc::clone(&state.slots).acquire_owned());
    loop {
        let now = tokio::time::Instant::now();
        // Without a deadline, one look. With one, until it -- or, with
        // eviction on, briefly: a request ending makes a sandbox evictable
        // without freeing a slot, and nothing announces that.
        let wait = match deadline {
            None => Duration::ZERO,
            Some(deadline) => {
                let left = deadline.saturating_duration_since(now);
                match state.opts.evict_idle_after {
                    Some(_) => left.min(Duration::from_millis(25)),
                    None => left,
                }
            }
        };
        if let Ok(permit) = tokio::time::timeout(wait, &mut acquire).await {
            tracing::debug!("slot granted after {:?}", now.elapsed());
            return permit.map_err(|_| "this node is shutting down".to_string());
        }
        if let Some(victim) = idle_victim(state) {
            tracing::info!("node full: pausing {victim}, idle longest, to make room");
            if pause_sandbox(state, &victim, true).await.is_ok() {
                state.metrics.evictions.inc();
                continue;
            }
        }
        if deadline.is_none_or(|deadline| tokio::time::Instant::now() >= deadline) {
            let held =
                (state.opts.capacity as usize).saturating_sub(state.slots.available_permits());
            let running = state.running() as usize;
            return Err(format!(
                "this node is full: {running} running and {} starting, of {}",
                held.saturating_sub(running),
                state.opts.capacity
            ));
        }
    }
}

/// How long a create waits for room on a full node. Not at all, normally: a
/// control plane reads 503 as "try another node", which is faster than any
/// wait. With eviction on, a full node is expected to make room, so a create
/// is parked for up to the ready timeout rather than refused because every
/// running sandbox happened to be mid-request.
fn create_park(state: &AppState) -> Option<Duration> {
    state
        .opts
        .evict_idle_after
        .map(|_| state.opts.ready_timeout)
}

/// The running sandbox idle longest that may be paused to make room: one
/// that resumes itself on traffic, idle at least `--evict-idle-after`.
fn idle_victim(state: &AppState) -> Option<String> {
    let idle_after = state.opts.evict_idle_after?;
    let cutoff = now_ms().saturating_sub(u64::try_from(idle_after.as_millis()).unwrap_or(u64::MAX));
    state
        .sandboxes
        .lock()
        .iter()
        .filter(|(_, live)| {
            live.lifecycle.auto_resume
                && !live.activity.busy()
                && live.activity.last_active_ms() <= cutoff
        })
        .min_by_key(|(_, live)| live.activity.last_active_ms())
        .map(|(id, _)| id.clone())
}

/// A sandbox's VM up and wired: what create, resume and fork all end with.
struct Running {
    vm: Arc<AgentVM>,
    network: Option<LiveNetwork>,
    process_shutdown: tokio::sync::oneshot::Sender<()>,
    process_addr: std::net::SocketAddr,
}

/// Bring up a sandbox's VM: from `snapshot` when given, else from the
/// template, else by booting; then its network and its envd listener.
async fn bring_up(
    state: &AppState,
    sandbox_id: &str,
    snapshot: Option<&std::path::Path>,
    network: Option<NetworkSpec>,
    access_token: &str,
) -> Result<Running, (StatusCode, String)> {
    let internal = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
    // From a snapshot, every sandbox is the template's guest, restored, with
    // the same CID and MAC it was snapshotted with -- which is fine, because
    // each has its own vsock device and its own gateway, and nothing outside
    // this VM ever sees either.
    let snapshot = snapshot.or(state.template.as_ref().map(|t| t.snapshot.as_path()));
    let (cid, mac) = match snapshot {
        Some(_) => (TEMPLATE_CID, TEMPLATE_MAC),
        None => {
            let mut next = state.next_cid.lock();
            let cid = *next;
            *next += 1;
            let [_, b, c, d] = u32::try_from(cid).unwrap_or(u32::MAX).to_be_bytes();
            (GUEST_CID_BASE + cid, [0x52, 0x54, 0x00, b, c, d])
        }
    };

    let t0 = std::time::Instant::now();
    let (vm, nic) = new_vm(
        &state.opts,
        sandbox_id,
        cid,
        network.is_some().then_some(mac),
    )
    .await
    .map_err(internal)?;
    let built = t0.elapsed();
    let launched = match snapshot {
        Some(snapshot) => {
            let working_set = state
                .template
                .as_ref()
                .map_or(&[][..], |t| t.working_set.as_slice());
            vm.launch_from_snapshot_prefaulted(snapshot, working_set)
                .await
        }
        None => vm.launch().await,
    };
    if let Err(e) = launched {
        let _ = vm.stop().await;
        return Err(internal(format!("launching: {e}")));
    }
    let launched_at = t0.elapsed();
    // A caller creating a sandbox waits for one it can actually use --
    // returning before the guest agent answers would hand back a
    // sandboxID that fails the first real request against it.
    //
    // A restored guest has its snapshot's clock and RNG, which the agent
    // resets in one round trip -- and that round trip is also the proof it
    // answers, so a restore makes one call, not a ping and then another.
    // Half of what a create waited on was that second trip. Refused rather
    // than served if the reseed fails: a sandbox sharing random state with
    // its siblings -- or with the fork it came from -- is not one to hand out.
    let ready = match snapshot {
        Some(_) => vm.after_restore(state.opts.ready_timeout).await,
        None => vm.ping_guest(state.opts.ready_timeout).await.map(|_| ()),
    };
    if let Err(e) = ready {
        let _ = vm.stop().await;
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("guest never became ready: {e}"),
        ));
    }
    let answered = t0.elapsed();

    let vm = Arc::new(vm);

    let network = match (network, nic) {
        (Some(spec), Some(device)) => {
            match start_network(state, &vm, device, spec, snapshot.is_none()).await {
                Ok(network) => Some(network),
                Err(e) => {
                    let _ = vm.stop().await;
                    return Err(internal(e));
                }
            }
        }
        _ => None,
    };

    // Give this sandbox its own process.Process listener -- envd's real
    // shape, one daemon per sandbox, not one shared server multiplexing
    // by sandbox ID (see hv2_api::envd_process's doc comment).
    //
    // On loopback, on a port the kernel picks. Loopback because the proxy is
    // the way in from anywhere else; kernel-picked because a counter handing
    // out ports overflowed u16 after 56,000 sandboxes and reused ports still
    // bound by the ones before.
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(e) => {
            let _ = vm.stop().await;
            return Err(internal(format!(
                "binding the sandbox's envd listener: {e}"
            )));
        }
    };
    let process_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => {
            let _ = vm.stop().await;
            return Err(internal(e.to_string()));
        }
    };
    let (process_shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
    let listener_token = Some(access_token.to_string());
    let process_vm = Arc::clone(&vm);
    let process_sandbox_id = sandbox_id.to_string();
    tokio::spawn(async move {
        let served = hv2_api::connect::serve_on(
            listener,
            hv2_api::envd_process::EnvdProcess::new(Arc::clone(&process_vm)),
            hv2_api::envd_filesystem::EnvdFilesystem::new(process_vm),
            listener_token,
            shutdown_rx,
        )
        .await;
        if let Err(e) = served {
            tracing::warn!("envd listener for {process_sandbox_id} stopped: {e}");
        }
    });

    tracing::debug!(
        "{sandbox_id} up in {:?}: build {built:?}, launch {:?}, agent answering {:?},          network and envd {:?}",
        t0.elapsed(),
        launched_at - built,
        answered - launched_at,
        t0.elapsed() - answered,
    );
    Ok(Running {
        vm,
        network,
        process_shutdown,
        process_addr,
    })
}

/// 244 bits from the OS RNG, via two v4 UUIDs.
fn new_access_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// A new sandbox ID.
///
/// A hyphen, not an underscore. This id becomes a DNS label -- the SDK
/// addresses a sandbox as `{port}-{sandboxID}.{domain}` -- and an underscore
/// is not legal in one. With `sbx_...` the SDK built a name its own resolver
/// then refused: "Label contains invalid characters". The proxy splits on the
/// *first* hyphen, so further hyphens are harmless. Random, not a clock: in a
/// cluster two nodes mint IDs into one namespace, and two creates in the same
/// nanosecond on two hosts is not a case to rule out by hoping.
fn new_sandbox_id() -> String {
    format!("sbx-{}", &uuid::Uuid::new_v4().simple().to_string()[..20])
}

/// Make a running sandbox reachable and known: routed, listed, recorded.
async fn register(
    state: &AppState,
    slot: Slot,
    running: Running,
    descriptor: SandboxResponse,
    record: SandboxRecord,
    lifecycle: Lifecycle,
    event: Option<&str>,
) {
    let sandbox_id = record.sandbox_id.clone();
    // Resolvable by name before the sandbox is announced, so a client that
    // uses the response immediately does not race the registration.
    state
        .routes
        .insert(&sandbox_id, ENVD_PORT, running.process_addr);
    let count = {
        let mut sandboxes = state.sandboxes.lock();
        sandboxes.insert(
            sandbox_id.clone(),
            LiveSandbox {
                vm: running.vm,
                process_shutdown: running.process_shutdown,
                process_addr: running.process_addr,
                _slot: slot,
                descriptor,
                network: running.network,
                record: record.clone(),
                lifecycle,
                activity: Activity::new(),
            },
        );
        u32::try_from(sandboxes.len()).unwrap_or(u32::MAX)
    };
    // Recorded before answering, so a control plane that routes the next
    // call by the store finds it.
    if let Some(node) = &state.node {
        let recorded = match event {
            None => node.created(&record, count).await,
            Some(kind) => node.transitioned(&record, kind, count).await,
        };
        if let Err(e) = recorded {
            tracing::warn!("recording {sandbox_id} in the cluster store: {e}");
        }
    }
}

async fn create_sandbox(state: Arc<AppState>, req: NewSandbox, default_timeout: u64) -> Response {
    // Room first, before anything is parsed or booted: a full node should
    // answer at once so a control plane can try the next one.
    let slot = match reserve(&state, create_park(&state)).await {
        Ok(slot) => slot,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let lifetime_secs = req.timeout.unwrap_or(default_timeout).min(MAX_TIMEOUT_SECS);
    let started_at_ms = now_ms();

    let lifecycle = match Lifecycle::from_request(&req, lifetime_secs) {
        Ok(lifecycle) => lifecycle,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    if lifecycle.pause_on_timeout && state.template.is_none() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "autoPause needs sandboxes restored from a template, and this node boots them \
             (--no-template, or the template failed to build)",
        );
    }

    // Decided before anything boots, so a policy that does not parse costs a
    // 400 and not a VM.
    let wants_network = req.allow_internet_access == Some(true) || req.network.is_some();
    let network = if state.opts.network {
        let policy = match policy_from(
            req.allow_internet_access,
            req.network.as_ref(),
            state.opts.egress_default,
            &state.opts.tenant_reserved,
        ) {
            Ok(policy) => policy,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        };
        let proxy = match egress_proxy_from(&state.opts, req.network.as_ref()).await {
            Ok(proxy) => proxy,
            Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
        };
        Some(NetworkSpec { policy, proxy })
    } else if wants_network {
        return api_error(
            StatusCode::BAD_REQUEST,
            "this server gives sandboxes no network interface (start it with --network)",
        );
    } else {
        None
    };

    let template_id = req.template_id.unwrap_or_else(|| "base".to_string());
    let sandbox_id = new_sandbox_id();
    let access_token = new_access_token();
    let running = match bring_up(&state, &sandbox_id, None, network, &access_token).await {
        Ok(running) => running,
        Err((status, e)) => return api_error(status, e),
    };

    // Built once and kept, so `POST /sandboxes/{id}/connect` answers with the
    // same description rather than a second one assembled from parts.
    let descriptor = SandboxResponse {
        template_id: template_id.clone(),
        sandbox_id: sandbox_id.clone(),
        client_id: sandbox_id.clone(),
        envd_version: ENVD_VERSION.to_string(),
        process_port: running.process_addr.port(),
        envd_host: format!("{ENVD_PORT}-{sandbox_id}"),
        proxy_port: state.opts.proxy_port,
        envd_access_token: access_token,
    };
    let record = SandboxRecord {
        sandbox_id: sandbox_id.clone(),
        node_id: state
            .node
            .as_ref()
            .map_or_else(|| "local".to_string(), |n| n.id().to_string()),
        template_id,
        started_at_ms,
        end_at_ms: started_at_ms + lifetime_secs * 1000,
        cpu_count: state.opts.cpu_cores,
        memory_mb: state.opts.memory_gb * 1024,
        metadata: req.metadata,
        envd_version: ENVD_VERSION.to_string(),
        descriptor: serde_json::to_value(&descriptor).unwrap_or_default(),
        paused: false,
    };
    register(
        &state,
        slot,
        running,
        descriptor.clone(),
        record,
        lifecycle,
        None,
    )
    .await;

    (StatusCode::CREATED, Json(descriptor)).into_response()
}

/// What a sandbox does when its time is up, and whether traffic wakes it.
#[derive(Debug, Clone, Copy, Default)]
struct Lifecycle {
    /// Pause rather than end at the timeout (E2B's `autoPause`).
    pause_on_timeout: bool,
    /// A paused sandbox resumes when a request arrives for it through the
    /// proxy (E2B's `autoResume`). Also what makes a running one eligible to
    /// be paused to make room, since nothing is lost by it.
    auto_resume: bool,
    /// The lifetime it was created with, which an automatic resume grants
    /// again.
    lifetime_secs: u64,
}

impl Lifecycle {
    fn from_request(req: &NewSandbox, lifetime_secs: u64) -> Result<Self, String> {
        // A filesystem-only pause keeps the disk and drops memory. The root
        // filesystem here *is* memory -- an initramfs -- so there is nothing
        // to keep, and the spec says to refuse rather than silently take a
        // memory snapshot instead.
        if req.auto_pause_memory == Some(false) {
            return Err(
                "autoPauseMemory=false (a filesystem-only pause) is not available here: a \
                 sandbox's filesystem lives in its memory, so a pause always keeps both"
                    .into(),
            );
        }
        Ok(Self {
            pause_on_timeout: req.auto_pause.unwrap_or(false),
            auto_resume: req.auto_resume.as_ref().is_some_and(|a| a.enabled),
            lifetime_secs,
        })
    }
}

/// A sandbox's network, as decided at creation or last replaced: enough to
/// give a resumed or forked copy the same one.
#[derive(Clone)]
struct NetworkSpec {
    policy: NetworkPolicy,
    proxy: Option<Socks5Proxy>,
}

/// A sandbox suspended to this node's disk.
struct PausedSandbox {
    snapshot: std::path::PathBuf,
    descriptor: SandboxResponse,
    record: SandboxRecord,
    lifecycle: Lifecycle,
    network: Option<NetworkSpec>,
}

/// The lock a sandbox's pause, resume and fork take, so two of them never
/// act on one sandbox at once.
fn transition_lock(state: &AppState, sandbox_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    Arc::clone(
        state
            .transitions
            .lock()
            .entry(sandbox_id.to_string())
            .or_default(),
    )
}

/// Suspend a running sandbox to disk: its memory goes back to the host, its
/// slot to the node, and its ID, token and metadata stay.
async fn pause_sandbox(
    state: &AppState,
    sandbox_id: &str,
    evicting: bool,
) -> Result<(), (StatusCode, String)> {
    let lock = transition_lock(state, sandbox_id);
    let _held = lock.lock().await;
    if state.paused.lock().contains_key(sandbox_id) {
        return Err((
            StatusCode::CONFLICT,
            format!("sandbox {sandbox_id} is already paused"),
        ));
    }
    if state.template.is_none() {
        return Err((
            StatusCode::CONFLICT,
            "pausing needs sandboxes restored from a template, and this node boots them".into(),
        ));
    }
    let live = {
        let mut sandboxes = state.sandboxes.lock();
        match sandboxes.get(sandbox_id) {
            None => return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))),
            // Chosen as idle, and a request arrived since: it is not idle,
            // and making room is not worth cutting that request off. Asked
            // for by name, a pause goes ahead regardless, as E2B's does.
            Some(live) if evicting && live.activity.busy() => {
                return Err((
                    StatusCode::CONFLICT,
                    format!("sandbox {sandbox_id} has a request in flight"),
                ))
            }
            Some(_) => {}
        }
        sandboxes
            .remove(sandbox_id)
            .expect("present under this lock")
    };
    let started = std::time::Instant::now();
    // Unroutable first: a request arriving now should resume it, not reach a
    // VM that is stopping.
    state.routes.remove_sandbox(sandbox_id);
    let snapshot = state.suspend_dir.join(format!("{sandbox_id}.snap"));
    let _ = std::fs::remove_file(&snapshot);
    if let Err(e) = live.vm.suspend_to(&snapshot).await {
        // Still running -- `suspend_to` resumes it on failure -- so put it
        // back as it was.
        let _ = std::fs::remove_file(&snapshot);
        state
            .routes
            .insert(sandbox_id, ENVD_PORT, live.process_addr);
        state.sandboxes.lock().insert(sandbox_id.to_string(), live);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pausing {sandbox_id}: {e}"),
        ));
    }
    let _ = live.process_shutdown.send(());
    let network = live.network.map(|network| {
        network.bridge.abort();
        NetworkSpec {
            policy: network.gateway.policy(),
            proxy: network.gateway.egress_proxy(),
        }
    });
    let mut record = live.record;
    record.paused = true;
    state.paused.lock().insert(
        sandbox_id.to_string(),
        PausedSandbox {
            snapshot,
            descriptor: live.descriptor,
            record: record.clone(),
            lifecycle: live.lifecycle,
            network,
        },
    );
    state.metrics.pauses.inc();
    state.metrics.pause_latency.observe(started.elapsed());
    if let Some(node) = &state.node {
        if let Err(e) = node
            .transitioned(&record, "sandbox-paused", state.running())
            .await
        {
            tracing::warn!("recording {sandbox_id}'s pause: {e}");
        }
    }
    Ok(())
}

/// Bring a paused sandbox back, under the same ID and token, to live
/// `lifetime_secs` from now. `Ok(None)` when it was running already.
async fn resume_sandbox(
    state: &Arc<AppState>,
    sandbox_id: &str,
    lifetime_secs: Option<u64>,
    park: Option<Duration>,
) -> Result<Option<SandboxResponse>, (StatusCode, String)> {
    let lock = transition_lock(state, sandbox_id);
    let _held = lock.lock().await;
    if state.sandboxes.lock().contains_key(sandbox_id) {
        return Ok(None);
    }
    if !state.paused.lock().contains_key(sandbox_id) {
        return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")));
    }
    let started = std::time::Instant::now();
    tracing::debug!("resume {sandbox_id}: waiting for a slot");
    let slot = reserve(state, park)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
    let Some(paused) = state.paused.lock().remove(sandbox_id) else {
        return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")));
    };
    let running = match bring_up(
        state,
        sandbox_id,
        Some(&paused.snapshot),
        paused.network.clone(),
        &paused.descriptor.envd_access_token,
    )
    .await
    {
        Ok(running) => running,
        Err(e) => {
            state.paused.lock().insert(sandbox_id.to_string(), paused);
            return Err(e);
        }
    };
    let _ = std::fs::remove_file(&paused.snapshot);

    let mut descriptor = paused.descriptor;
    descriptor.process_port = running.process_addr.port();
    let mut record = paused.record;
    record.paused = false;
    let lifetime = lifetime_secs
        .unwrap_or(paused.lifecycle.lifetime_secs)
        .min(MAX_TIMEOUT_SECS);
    record.end_at_ms = now_ms() + lifetime * 1000;
    record.descriptor = serde_json::to_value(&descriptor).unwrap_or_default();
    register(
        state,
        slot,
        running,
        descriptor.clone(),
        record,
        paused.lifecycle,
        Some("sandbox-resumed"),
    )
    .await;
    state.metrics.resumes.inc();
    state.metrics.resume_latency.observe(started.elapsed());
    Ok(Some(descriptor))
}

#[derive(Debug, Default, Deserialize)]
struct PauseRequest {
    memory: Option<bool>,
}

/// `POST /sandboxes/{id}/pause`.
async fn pause_route(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<PauseRequest>>,
) -> Response {
    if body.and_then(|Json(b)| b.memory) == Some(false) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "memory=false (a filesystem-only pause) is not available here: a sandbox's \
             filesystem lives in its memory, so a pause always keeps both",
        );
    }
    match pause_sandbox(&state, &sandbox_id, false).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, e)) => api_error(status, e),
    }
}

#[derive(Debug, Default, Deserialize)]
struct ResumeRequest {
    timeout: Option<u64>,
    memory: Option<bool>,
}

/// `POST /sandboxes/{id}/resume`: E2B's deprecated route, 15 s by default.
async fn resume_route(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<ResumeRequest>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    if body.memory == Some(false) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "memory=false (resume from disk only) is not available here",
        );
    }
    match resume_sandbox(&state, &sandbox_id, Some(body.timeout.unwrap_or(15)), None).await {
        Ok(Some(descriptor)) => (StatusCode::CREATED, Json(descriptor)).into_response(),
        Ok(None) => api_error(
            StatusCode::CONFLICT,
            format!("sandbox {sandbox_id} is already running"),
        ),
        Err((status, e)) => api_error(status, e),
    }
}

#[derive(Debug, Default, Deserialize)]
struct ForkRequest {
    timeout: Option<u64>,
    count: Option<u32>,
}

/// `POST /sandboxes/{id}/fork`: checkpoint a running sandbox in place and
/// start `count` new ones from that checkpoint.
///
/// The checkpoint is a layered snapshot -- only what the source changed since
/// its template -- so its cost does not grow with the guest's RAM, and every
/// fork maps the template and copies in just those pages. Each fork gets its
/// own ID, token, network gateway (with the source's policy) and a reseeded
/// RNG: a fork that drew the same random numbers as its siblings would be a
/// fork of their secrets too.
async fn fork_route(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    body: Option<Json<ForkRequest>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let count = body.count.unwrap_or(1);
    if !(1..=100).contains(&count) {
        return api_error(StatusCode::BAD_REQUEST, "count is between 1 and 100");
    }
    let lifetime_secs = body.timeout.unwrap_or(15).min(MAX_TIMEOUT_SECS);
    if state.template.is_none() {
        return api_error(
            StatusCode::CONFLICT,
            "forking needs sandboxes restored from a template, and this node boots them",
        );
    }

    let checkpoint = state.suspend_dir.join(format!(
        "{sandbox_id}-fork-{}.snap",
        uuid::Uuid::new_v4().simple()
    ));
    let (template_id, metadata, network) = {
        let lock = transition_lock(&state, &sandbox_id);
        let _held = lock.lock().await;
        let source = {
            let sandboxes = state.sandboxes.lock();
            sandboxes.get(&sandbox_id).map(|live| {
                (
                    Arc::clone(&live.vm),
                    live.record.template_id.clone(),
                    live.record.metadata.clone(),
                    live.network.as_ref().map(|n| NetworkSpec {
                        policy: n.gateway.policy(),
                        proxy: n.gateway.egress_proxy(),
                    }),
                )
            })
        };
        let Some((vm, template_id, metadata, network)) = source else {
            return if state.paused.lock().contains_key(&sandbox_id) {
                api_error(
                    StatusCode::CONFLICT,
                    format!("sandbox {sandbox_id} is paused; resume it to fork it"),
                )
            } else {
                api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))
            };
        };
        let started = std::time::Instant::now();
        if let Err(e) = vm.checkpoint_to(&checkpoint).await {
            let _ = std::fs::remove_file(&checkpoint);
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("checkpointing {sandbox_id}: {e}"),
            );
        }
        state.metrics.checkpoint_latency.observe(started.elapsed());
        (template_id, metadata, network)
    };

    // Concurrently: each fork is independent, and they are what a caller
    // fanning work out to N agents is waiting on.
    let forks: Vec<_> = (0..count)
        .map(|_| {
            let state = Arc::clone(&state);
            let checkpoint = checkpoint.clone();
            let template_id = template_id.clone();
            let metadata = metadata.clone();
            let network = network.clone();
            async move {
                let slot = reserve(&state, create_park(&state))
                    .await
                    .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
                let fork_id = new_sandbox_id();
                let access_token = new_access_token();
                let running =
                    bring_up(&state, &fork_id, Some(&checkpoint), network, &access_token).await?;
                let started_at_ms = now_ms();
                let descriptor = SandboxResponse {
                    template_id: template_id.clone(),
                    sandbox_id: fork_id.clone(),
                    client_id: fork_id.clone(),
                    envd_version: ENVD_VERSION.to_string(),
                    process_port: running.process_addr.port(),
                    envd_host: format!("{ENVD_PORT}-{fork_id}"),
                    proxy_port: state.opts.proxy_port,
                    envd_access_token: access_token,
                };
                let record = SandboxRecord {
                    sandbox_id: fork_id,
                    node_id: state
                        .node
                        .as_ref()
                        .map_or_else(|| "local".to_string(), |n| n.id().to_string()),
                    template_id,
                    started_at_ms,
                    end_at_ms: started_at_ms + lifetime_secs * 1000,
                    cpu_count: state.opts.cpu_cores,
                    memory_mb: state.opts.memory_gb * 1024,
                    metadata,
                    envd_version: ENVD_VERSION.to_string(),
                    descriptor: serde_json::to_value(&descriptor).unwrap_or_default(),
                    paused: false,
                };
                let lifecycle = Lifecycle {
                    lifetime_secs,
                    ..Lifecycle::default()
                };
                register(
                    &state,
                    slot,
                    running,
                    descriptor.clone(),
                    record,
                    lifecycle,
                    None,
                )
                .await;
                Ok::<_, (StatusCode, String)>(descriptor)
            }
        })
        .map(tokio::spawn)
        .collect();
    let mut results = Vec::with_capacity(forks.len());
    for fork in forks {
        results.push(fork.await.unwrap_or_else(|e| {
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("the fork's task failed: {e}"),
            ))
        }));
    }
    // Every fork has copied what it needed out of the checkpoint by now.
    let _ = std::fs::remove_file(&checkpoint);

    let results: Vec<_> = results
        .into_iter()
        .map(|result| match result {
            Ok(descriptor) => {
                state.metrics.forks_ok.inc();
                json!({ "sandbox": descriptor })
            }
            Err((status, message)) => {
                state.metrics.forks_failed.inc();
                json!({ "error": { "code": status.as_u16(), "message": message } })
            }
        })
        .collect();
    (StatusCode::CREATED, Json(results)).into_response()
}

/// The proxy's view of this node's sandboxes: a paused one that resumes on
/// traffic is brought back by the request that wants it, and every request
/// is counted against its sandbox for as long as it is in flight.
struct ResumingRoutes {
    state: Arc<AppState>,
}

impl ResumingRoutes {
    /// The route and an in-flight guard for a sandbox that is running now.
    ///
    /// Counted under the same lock a pause removes the sandbox under, so a
    /// pause either sees this request or this request sees no sandbox --
    /// and then waits for the pause and resumes it.
    async fn running(
        &self,
        sandbox: &str,
        port: u16,
    ) -> Option<(std::net::SocketAddr, sandbox_proxy::InFlight)> {
        use sandbox_proxy::SandboxRoutes;
        let guard = {
            let sandboxes = self.state.sandboxes.lock();
            let live = sandboxes.get(sandbox)?;
            ActivityGuard::enter(&live.activity)
        };
        let addr = self.state.routes.resolve(sandbox, port).await?;
        Some((addr, sandbox_proxy::InFlight::new(guard)))
    }
}

#[async_trait::async_trait]
impl sandbox_proxy::SandboxRoutes for ResumingRoutes {
    async fn resolve(&self, sandbox: &str, port: u16) -> Option<std::net::SocketAddr> {
        self.open(sandbox, port).await.map(|(addr, _)| addr)
    }

    async fn open(
        &self,
        sandbox: &str,
        port: u16,
    ) -> Option<(std::net::SocketAddr, sandbox_proxy::InFlight)> {
        tracing::debug!("proxy: request for {sandbox} port {port}");
        if let Some(open) = self.running(sandbox, port).await {
            return Some(open);
        }
        // Neither running nor paused can also mean "between the two": a
        // pause takes the sandbox out of one map before it is in the other,
        // and a request arriving then would be refused as unknown -- which
        // is how the first command to a sandbox evicted as it was created
        // failed. The pause holds the sandbox's transition lock throughout,
        // so wait for it and look again. Only a lock that already exists:
        // creating one per unknown name would let any request grow the
        // table.
        let transition = self.state.transitions.lock().get(sandbox).cloned();
        if let Some(transition) = transition {
            drop(transition.lock().await);
            if let Some(open) = self.running(sandbox, port).await {
                return Some(open);
            }
        }
        let wakes = self
            .state
            .paused
            .lock()
            .get(sandbox)
            .is_some_and(|p| p.lifecycle.auto_resume);
        if !wakes {
            tracing::debug!("proxy: {sandbox} is neither running nor paused to wake");
            return None;
        }
        tracing::debug!("proxy: a request for paused {sandbox}; resuming it");
        // Parked, not refused, while the node is full: the request waits for
        // a slot as long as a sandbox may take to become ready.
        match resume_sandbox(
            &self.state,
            sandbox,
            None,
            Some(self.state.opts.ready_timeout),
        )
        .await
        {
            Ok(_) => {
                self.state.metrics.auto_resumes.inc();
                self.running(sandbox, port).await
            }
            Err((_, e)) => {
                tracing::warn!("auto-resuming {sandbox} for a request: {e}");
                None
            }
        }
    }
}

/// How a running sandbox is being used, as the proxy sees it.
struct Activity {
    /// Requests to it in flight now, streamed responses included.
    in_flight: std::sync::atomic::AtomicUsize,
    /// When a request to it last began or ended.
    last_active_ms: std::sync::atomic::AtomicU64,
}

impl Activity {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            last_active_ms: std::sync::atomic::AtomicU64::new(now_ms()),
        })
    }

    fn busy(&self) -> bool {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    fn last_active_ms(&self) -> u64 {
        self.last_active_ms
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// One request in flight; its drop is the request's end.
struct ActivityGuard(Arc<Activity>);

impl ActivityGuard {
    fn enter(activity: &Arc<Activity>) -> Self {
        use std::sync::atomic::Ordering;
        activity.in_flight.fetch_add(1, Ordering::SeqCst);
        activity.last_active_ms.store(now_ms(), Ordering::SeqCst);
        Self(Arc::clone(activity))
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.0.last_active_ms.store(now_ms(), Ordering::SeqCst);
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Put a gateway behind a sandbox's NIC, and point the guest at it.
async fn start_network(
    state: &AppState,
    vm: &Arc<AgentVM>,
    device: Arc<parking_lot::Mutex<hv2_core::devices::virtio_net_mmio::VirtioNetMmio>>,
    spec: NetworkSpec,
    configure_guest: bool,
) -> Result<LiveNetwork, String> {
    let mut builder = Gateway::builder(spec.policy).config(GatewayConfig::default());
    if let Some(authority) = &state.authority {
        builder = builder.intercept_with(Arc::clone(authority));
    }
    let gateway = builder
        .build()
        .map_err(|e| format!("starting the gateway: {e}"))?;
    let handle = gateway.handle();
    handle.set_egress_proxy(spec.proxy);

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

    // A restored guest was configured once, in its template.
    if configure_guest {
        if let Err(e) = configure_guest_network(vm, handle.ca_pem().as_deref()).await {
            bridge.abort();
            return Err(e);
        }
    }

    Ok(LiveNetwork {
        gateway: handle,
        bridge,
    })
}

/// Point a guest at its gateway's resolver and make it trust the egress CA.
///
/// The kernel wrote the nameserver to /proc/net/pnp, in resolv.conf's own
/// format. The CA goes where OpenSSL, curl and Python's ssl look by default,
/// so injection works for a client that was not told about it. `mkdir -p
/// /etc` because a minimal initramfs need not have one -- the reference
/// image here did not, which is how this was found.
async fn configure_guest_network(vm: &AgentVM, ca: Option<&str>) -> Result<(), String> {
    let mut script = String::from("mkdir -p /etc && ln -sf /proc/net/pnp /etc/resolv.conf");
    if let Some(ca) = ca {
        script.push_str(&format!(
            " && mkdir -p /etc/ssl/certs && printf '%s' '{ca}' >> /etc/ssl/certs/ca-certificates.crt"
        ));
    }
    let setup = vm
        .exec_in_guest(
            "/bin/sh",
            &["-c".to_string(), script],
            Duration::from_secs(10),
        )
        .await
        .map_err(|e| format!("configuring the guest's network: {e}"))?;
    if setup.exit_code != Some(0) {
        return Err(format!(
            "configuring the guest's network exited {:?}: {}",
            setup.exit_code, setup.stderr
        ));
    }
    Ok(())
}

/// The vsock CID and NIC MAC every template-restored sandbox has: the
/// template's. Only this VM's own devices ever see them.
const TEMPLATE_CID: u64 = GUEST_CID_BASE;
const TEMPLATE_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x00, 0x00, 0x01];

/// A booted, configured guest written to disk, that sandboxes are restored
/// from instead of booting.
struct Template {
    dir: std::path::PathBuf,
    snapshot: std::path::PathBuf,
    /// The guest pages a sandbox restored from this touches before it first
    /// answers, as guest-physical (address, length) ranges: prefaulted into
    /// every restore, so the guest does not take an exit for each. Empty
    /// unless `--prefault`, or when it could not be measured.
    working_set: Vec<(u64, u64)>,
}

impl Drop for Template {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The kernel command line every sandbox guest boots with.
fn guest_cmdline(network: bool) -> String {
    format!(
        "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=0 {}{}",
        hv2_core::BootSource::MICROVM_FAST_BOOT_ARGS,
        // The guest configures its NIC from this before init runs.
        if network {
            format!(" {}", GatewayConfig::default().kernel_ip_arg())
        } else {
            String::new()
        }
    )
}

type NetDevice = Arc<parking_lot::Mutex<hv2_core::devices::virtio_net_mmio::VirtioNetMmio>>;

/// A sandbox VM, built and wired but not started: guest channel on `cid`,
/// and a NIC with `mac` when one is given.
async fn new_vm(
    opts: &Options,
    name: &str,
    cid: u64,
    mac: Option<[u8; 6]>,
) -> Result<(AgentVM, Option<NetDevice>), String> {
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let vm = AgentVM::builder()
        .name(name.to_string())
        .cpu_cores(opts.cpu_cores)
        .memory_gb(opts.memory_gb)
        .capabilities(capabilities)
        .boot_linux(
            &opts.kernel,
            Some(&opts.initrd),
            guest_cmdline(mac.is_some()),
        )
        .build()
        .await
        .map_err(|e| format!("building the VM: {e}"))?;
    vm.attach_guest_channel(cid)
        .await
        .map_err(|e| format!("attaching the guest channel: {e}"))?;
    // Attached before launch: virtio-mmio has no hotplug, and the kernel
    // learns where to probe from the command line `attach_net` extends.
    let nic = match mac {
        Some(mac) => Some(
            vm.vm()
                .attach_net(mac)
                .await
                .map_err(|e| format!("attaching the network device: {e}"))?,
        ),
        None => None,
    };
    Ok((vm, nic))
}

/// Boot the template once, configure it as every sandbox needs, and write it
/// to disk with its memory as an image a restore can map.
async fn build_template(opts: &Options, authority: Option<&Authority>) -> Result<Template, String> {
    let dir = std::env::temp_dir().join(format!("hv2-sandboxd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut template = Template {
        snapshot: dir.join("template.snap"),
        dir,
        working_set: Vec::new(),
    };

    let started = std::time::Instant::now();
    let (vm, _nic) = new_vm(
        opts,
        "template",
        TEMPLATE_CID,
        opts.network.then_some(TEMPLATE_MAC),
    )
    .await?;
    vm.launch().await.map_err(|e| format!("launching: {e}"))?;
    let result = async {
        vm.ping_guest(opts.ready_timeout)
            .await
            .map_err(|e| format!("the template's agent never answered: {e}"))?;
        if opts.network {
            configure_guest_network(&vm, authority.map(Authority::ca_pem)).await?;
        }
        // Touch the paths a sandbox's first requests take, so their pages are
        // in the image rather than faulted in by every sandbox.
        vm.exec_in_guest(
            "/bin/sh",
            &["-c".into(), "true".into()],
            Duration::from_secs(5),
        )
        .await
        .map_err(|e| format!("warming the template: {e}"))?;
        vm.snapshot_to(&template.snapshot)
            .await
            .map_err(|e| format!("snapshotting the template: {e}"))
    }
    .await;
    let _ = vm.stop().await;
    result?;

    // What a sandbox restored from it touches before it first answers:
    // measured on one, restored and asked exactly that. A failure costs only
    // the optimisation.
    if opts.prefault {
        match working_set(opts, &template.snapshot).await {
            Ok(ranges) => template.working_set = ranges,
            Err(e) => tracing::warn!("measuring the template's working set: {e}; not prefaulting"),
        }
    }
    let pages: u64 = template.working_set.iter().map(|(_, len)| len / 4096).sum();
    tracing::info!(
        "template ready in {:?} at {}; working set {pages} pages in {} ranges",
        started.elapsed(),
        template.snapshot.display(),
        template.working_set.len()
    );
    Ok(template)
}

/// The pages a guest restored from `snapshot` touches to answer its first
/// request, found by restoring one and asking it.
async fn working_set(
    opts: &Options,
    snapshot: &std::path::Path,
) -> Result<Vec<(u64, u64)>, String> {
    let (vm, _nic) = new_vm(
        opts,
        "template-probe",
        TEMPLATE_CID,
        opts.network.then_some(TEMPLATE_MAC),
    )
    .await?;
    let measured = async {
        vm.launch_from_snapshot(snapshot)
            .await
            .map_err(|e| format!("restoring a probe: {e}"))?;
        // Exactly what a create does before it answers, and no more: every
        // page prefaulted costs a little, whether or not it is then used.
        vm.after_restore(opts.ready_timeout)
            .await
            .map_err(|e| format!("the probe's agent: {e}"))?;
        vm.touched_ranges().map_err(|e| e.to_string())
    }
    .await;
    let _ = vm.stop().await;
    measured
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
    let policy = match policy_from(
        None,
        Some(&update),
        state.opts.egress_default,
        &state.opts.tenant_reserved,
    ) {
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
    if end_sandbox(&state, &sandbox_id, "sandbox-deleted").await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))
    }
}

/// Stop a sandbox and everything it had, and say why (`kind` is the cluster
/// event). Returns whether there was such a sandbox.
async fn end_sandbox(state: &AppState, sandbox_id: &str, kind: &str) -> bool {
    // Not while it is pausing, resuming or being forked from.
    let lock = transition_lock(state, sandbox_id);
    let held = lock.lock().await;
    let (removed, running) = {
        let mut sandboxes = state.sandboxes.lock();
        let removed = sandboxes.remove(sandbox_id);
        (removed, u32::try_from(sandboxes.len()).unwrap_or(u32::MAX))
    };
    let Some(live) = removed else {
        // A paused one has only its snapshot and its record to lose.
        let paused = state.paused.lock().remove(sandbox_id);
        drop(held);
        state.transitions.lock().remove(sandbox_id);
        let Some(paused) = paused else {
            return false;
        };
        let _ = std::fs::remove_file(&paused.snapshot);
        state.metrics.ended_deleted.inc();
        if let Some(node) = &state.node {
            if let Err(e) = node.ended(sandbox_id, kind, running).await {
                tracing::warn!("recording the end of {sandbox_id}: {e}");
            }
        }
        return true;
    };
    drop(held);
    state.transitions.lock().remove(sandbox_id);
    if kind == "sandbox-expired" {
        state.metrics.ended_expired.inc();
    } else {
        state.metrics.ended_deleted.inc();
    }
    // Stop resolving the name first: a request that arrives during teardown
    // should fail to route rather than be sent at a VM that is stopping.
    state.routes.remove_sandbox(sandbox_id);
    let _ = live.process_shutdown.send(());
    // Dropping the bridge drops the gateway, whose stack task ends with it;
    // open upstream connections close as their tasks see the guest side go.
    if let Some(network) = live.network {
        network.bridge.abort();
    }
    if let Err(e) = live.vm.stop().await {
        tracing::warn!("stopping sandbox {sandbox_id}: {e}");
    }
    if let Some(node) = &state.node {
        if let Err(e) = node.ended(sandbox_id, kind, running).await {
            tracing::warn!("recording the end of {sandbox_id}: {e}");
        }
    }
    true
}

/// End every sandbox whose time is up, once a second -- or pause it, if it
/// asked to be paused instead. A paused sandbox has no timeout: it waits on
/// disk until it is resumed or deleted, as E2B's do.
async fn expire(state: Arc<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = now_ms();
        let due: Vec<(String, bool)> = state
            .sandboxes
            .lock()
            .iter()
            .filter(|(_, live)| live.record.end_at_ms <= now)
            .map(|(id, live)| (id.clone(), live.lifecycle.pause_on_timeout))
            .collect();
        for (id, pause) in due {
            if pause {
                match pause_sandbox(&state, &id, false).await {
                    Ok(()) => {
                        tracing::info!("sandbox {id} reached its timeout and paused");
                        continue;
                    }
                    // Ended rather than left running past its time.
                    Err((_, e)) => tracing::warn!("pausing {id} at its timeout: {e}; ending it"),
                }
            }
            if end_sandbox(&state, &id, "sandbox-expired").await {
                tracing::info!("sandbox {id} reached its timeout");
            }
        }
    }
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
            eprintln!("hv2-sandboxd: {e}");
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
                eprintln!("hv2-sandboxd: generating the egress CA: {e}");
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

    // The template, before anything listens: a node that advertised itself
    // and then spent a second booting would be scheduled onto meanwhile.
    // Failing to build one is not fatal -- sandboxes boot instead, slower,
    // and the log says why.
    let template = if opts.no_template {
        None
    } else {
        match build_template(&opts, authority.as_deref()).await {
            Ok(template) => Some(template),
            Err(e) => {
                tracing::warn!("no template ({e}); every sandbox will boot instead");
                None
            }
        }
    };
    let template_line = if template.is_some() {
        "sandboxes are restored from a template snapshot"
    } else {
        "sandboxes boot from the kernel (no template)"
    };

    // Cluster membership, if asked for: checked and joined before anything
    // listens, so a node that cannot reach its store fails at start rather
    // than serving sandboxes no control plane can find.
    let node = match &opts.cluster_store {
        None => None,
        Some(url) => {
            let (Some(api), Some(proxy)) =
                (opts.advertise_api.clone(), opts.advertise_proxy.clone())
            else {
                eprintln!(
                    "hv2-sandboxd: --cluster-store needs --advertise-api and --advertise-proxy: \
                     the addresses a control plane dials, which 0.0.0.0 is not"
                );
                return std::process::ExitCode::FAILURE;
            };
            let proxy = match tokio::net::lookup_host(&proxy).await.map(|mut a| a.next()) {
                Ok(Some(addr)) => addr,
                Ok(None) | Err(_) => {
                    eprintln!("hv2-sandboxd: --advertise-proxy {proxy} does not resolve");
                    return std::process::ExitCode::FAILURE;
                }
            };
            if opts.cluster_token.is_none() {
                tracing::warn!(
                    "no --cluster-token: anyone who can reach this node's port can use it, \
                     bypassing the control plane's API key"
                );
            }
            let store = match hv2_cluster::store::open(url, &opts.cluster_namespace).await {
                Ok(store) => store,
                Err(e) => {
                    eprintln!("hv2-sandboxd: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            };
            let id = opts
                .node_id
                .clone()
                .unwrap_or_else(|| format!("node-{}", proxy.to_string().replace([':', '.'], "-")));
            let agent = NodeAgent::new(
                store,
                NodeConfig {
                    id,
                    api,
                    proxy,
                    capacity: opts.capacity,
                    ttl: opts.node_ttl,
                },
            );
            if let Err(e) = agent.join().await {
                eprintln!("hv2-sandboxd: joining the cluster: {e}");
                return std::process::ExitCode::FAILURE;
            }
            Some(agent)
        }
    };

    // Where paused sandboxes and fork checkpoints go: beside the template,
    // whose image every one of them is layered over.
    let suspend_dir = template
        .as_ref()
        .map_or_else(
            || std::env::temp_dir().join(format!("hv2-sandboxd-{}", std::process::id())),
            |t| t.dir.clone(),
        )
        .join("suspended");
    if let Err(e) = std::fs::create_dir_all(&suspend_dir) {
        eprintln!("hv2-sandboxd: {}: {e}", suspend_dir.display());
        return std::process::ExitCode::FAILURE;
    }

    let routes = Arc::new(PortMap::new());
    let opts_capacity = opts.capacity as usize;
    let state = Arc::new(AppState {
        authority,
        opts,
        sandboxes: Mutex::new(HashMap::new()),
        next_cid: Mutex::new(0),
        routes: Arc::clone(&routes),
        slots: Arc::new(tokio::sync::Semaphore::new(opts_capacity)),
        template,
        metrics: NodeMetrics::default(),
        node: node.clone(),
        paused: Mutex::new(HashMap::new()),
        suspend_dir,
        transitions: Mutex::new(HashMap::new()),
    });
    if let Some(node) = node.clone() {
        let beating = Arc::clone(&state);
        tokio::spawn(node.heartbeat(move || beating.running()));
    }
    tokio::spawn(expire(Arc::clone(&state)));

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
            eprintln!("hv2-sandboxd: bad proxy port {proxy_port}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let tls = match (&tls_cert, &tls_key) {
        (Some(cert), Some(key)) => {
            match sandbox_proxy::tls_config(std::path::Path::new(cert), std::path::Path::new(key)) {
                Ok(config) => Some(config),
                Err(e) => {
                    eprintln!("hv2-sandboxd: TLS: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
        (None, None) => None,
        // Half a TLS configuration is a mistake, and starting in plaintext
        // because one flag was missing is the wrong way to report it.
        _ => {
            eprintln!("hv2-sandboxd: --tls-cert and --tls-key must be given together");
            return std::process::ExitCode::FAILURE;
        }
    };
    let scheme = if tls.is_some() {
        "HTTP/2 over TLS"
    } else {
        "HTTP/2"
    };

    // The proxy asks the node, not the bare map, so that a request for a
    // paused sandbox can resume it.
    let routes: Arc<dyn sandbox_proxy::SandboxRoutes> = Arc::new(ResumingRoutes {
        state: Arc::clone(&state),
    });
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
        .route("/sandboxes", post(create_v1).get(list_sandboxes))
        // What current SDKs (2.51+) call: `NewSandboxV2`, the same fields
        // used here, secure-only -- which every sandbox here already is.
        .route("/v2/sandboxes", post(create_v2).get(list_sandboxes))
        .route("/sandboxes/{sandboxID}/connect", post(connect_sandbox))
        .route("/v2/sandboxes/{sandboxID}/connect", post(connect_sandbox))
        .route("/sandboxes/{sandboxID}/timeout", post(set_timeout))
        .route("/sandboxes/{sandboxID}/pause", post(pause_route))
        .route("/sandboxes/{sandboxID}/resume", post(resume_route))
        .route("/sandboxes/{sandboxID}/fork", post(fork_route))
        .route("/sandboxes/{sandboxID}/exec", post(exec))
        .route(
            "/sandboxes/{sandboxID}",
            get(sandbox_detail).delete(destroy_sandbox),
        )
        .route("/sandboxes/{sandboxID}/network", put(update_network))
        .route(
            "/sandboxes/{sandboxID}/network/decisions",
            get(network_decisions),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            require_cluster_token,
        ))
        .route("/metrics", get(node_metrics))
        // Even "no such route" has to be JSON: the SDK parses the body of
        // every non-2xx reply before it looks at the status.
        .fallback(|uri: axum::http::Uri| async move {
            api_error(StatusCode::NOT_FOUND, format!("no route {uri}"))
        })
        .with_state(Arc::clone(&state));

    let addr = format!("0.0.0.0:{port}");
    println!(
        "hv2-sandboxd: listening on {addr}, capacity {}{}",
        state.opts.capacity,
        node.as_ref()
            .map(|n| format!(", cluster node {}", n.id()))
            .unwrap_or_default()
    );
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
    println!(
        "  POST   /sandboxes/{{id}}/pause    -- suspend to disk; connect or resume brings it back,          and with autoResume so does a request through the proxy"
    );
    println!("  POST   /sandboxes/{{id}}/fork     -- checkpoint in place, start `count` copies");
    println!("  GET    /sandboxes/{{id}}/network/decisions -- NOT E2B's; the gateway's audit log");
    println!("{network_line}");
    println!("{template_line}");
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
            eprintln!("hv2-sandboxd: could not bind {addr}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // Ctrl-C or SIGTERM ends the server gracefully, then the node leaves the
    // cluster at once instead of lingering until its TTL runs out -- which is
    // what makes scaling a cluster down a non-event.
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    () = async {
                        match term.as_mut() {
                            Some(term) => { term.recv().await; }
                            None => std::future::pending::<()>().await,
                        }
                    } => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await;
    // Paused ones too: their snapshots are layered over a template this
    // process is about to delete, so they could not be resumed anyway.
    let ids: Vec<String> = state
        .sandboxes
        .lock()
        .keys()
        .chain(state.paused.lock().keys())
        .cloned()
        .collect();
    for id in ids {
        end_sandbox(&state, &id, "sandbox-deleted").await;
    }
    if let Some(node) = &node {
        if let Err(e) = node.leave().await {
            tracing::warn!("leaving the cluster: {e}");
        }
    }
    if let Err(e) = served {
        eprintln!("hv2-sandboxd: server error: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_net::network_policy::AddressVerdict;

    fn allowing_everything() -> SandboxNetworkConfig {
        SandboxNetworkConfig {
            allow_out: vec!["0.0.0.0/0".into()],
            ..SandboxNetworkConfig::default()
        }
    }

    /// The daemon's policies carry the operator's grant: none by default, so
    /// a tenant's `0.0.0.0/0` reaches the internet and not the cluster store.
    #[test]
    fn a_tenant_rule_opens_reserved_ranges_only_as_the_operator_allows() {
        let store: std::net::IpAddr = "10.0.3.7".parse().unwrap();
        let public: std::net::IpAddr = "93.184.216.34".parse().unwrap();

        let policy = policy_from(None, Some(&allowing_everything()), Verdict::Deny, &[]).unwrap();
        assert_eq!(
            policy.decide_address(store),
            AddressVerdict::Deny("reserved address")
        );
        assert_eq!(
            policy.decide_address(public),
            AddressVerdict::Allow("allowOut address")
        );

        let grant = [Cidr::parse("10.0.3.0/24").unwrap()];
        let policy =
            policy_from(None, Some(&allowing_everything()), Verdict::Deny, &grant).unwrap();
        assert_eq!(
            policy.decide_address(store),
            AddressVerdict::Allow("allowOut address")
        );
    }
}
