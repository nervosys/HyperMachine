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
//! API (`api.e2b.app`, `POST /sandboxes` etc. â€” `spec/openapi.yml` in
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
use axum::http::{HeaderMap, StatusCode};
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
use hv2_cluster::model::{
    ClusterEvent, SandboxRecord, metadata_matches, now_ms, parse_metadata_query,
};
use hv2_cluster::names::{NAME_OPERATION_HEADER, NameReservation, SandboxName};
use hv2_cluster::node::{NodeAgent, NodeConfig};
use hv2_net::gateway::socks::Socks5Proxy;
use hv2_net::gateway::{Gateway, GatewayConfig, GatewayHandle, mitm::Authority};
use hv2_net::network_policy::{Cidr, Headers, NetworkPolicy, Verdict};

mod boot_diagnostics;
mod builds;
mod checkpoints;
mod cloud_login;
mod env_vars;
mod forwards;
mod identity;
mod idle;
mod initramfs;
// Volumes are served with openat2 and O_PATH, which only Linux has; see
// volumes_unsupported.rs for what other hosts answer.
#[cfg(target_os = "linux")]
mod ninep;
mod oci;
mod snapshots;
mod telemetry;
#[cfg(target_os = "linux")]
mod volumes;
#[cfg(not(target_os = "linux"))]
#[path = "volumes_unsupported.rs"]
mod volumes;

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
/// | 0.5.7 | octet-stream upload | yes -- `hv2_api::envd_files`, multipart too; not gzip |
/// | 0.6.2 | xattr file metadata | **no** -- `metadata` is always empty |
/// | 0.6.3 | entry info on watch events | yes |
///
/// 0.6.3 is chosen because the alternative is worse: a lower number would
/// turn off watch entry info, which really works, to avoid claiming xattrs,
/// which fail gracefully anyway -- absent metadata reads as empty rather
/// than as an error. Claiming less would cost a working feature and buy
/// nothing. A gzip-compressed upload is refused with 415, which only a
/// caller asking for `gzip=True` sends.
const ENVD_VERSION: &str = "0.6.3";

#[derive(Clone)]
struct Options {
    port: u16,
    proxy_port: u16,
    /// Both must be given for the proxy to speak TLS; either alone is an
    /// error rather than a silent downgrade to plaintext.
    tls_cert: Option<String>,
    tls_key: Option<String>,
    kernel: String,
    initrd: String,
    /// A sandbox's memory and vCPUs, unless its template sets its own.
    memory_mb: u64,
    cpu_cores: u32,
    ready_timeout: Duration,
    cold_start_concurrency: Option<usize>,
    /// Give each sandbox a NIC behind a gateway. Off by default.
    network: bool,
    /// Private operator policy; exact sandbox IDs, never guest-supplied secrets.
    egress_secrets_file: Option<String>,
    /// Additional operator trust roots for intercepted upstream TLS.
    egress_upstream_ca: Option<String>,
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
    /// Refuse startup if any configured snapshot template cannot be prepared.
    require_template: bool,
    /// A directory shared by every node, mounted at the same path on each:
    /// templates, the egress CA and paused sandboxes, so a sandbox paused on
    /// one node resumes on any.
    snapshot_store: Option<std::path::PathBuf>,
    /// Mutual TLS for the API and the envd proxy: the CA peers must be
    /// signed by, and this node's certificate (carrying the nodes' shared
    /// name) and key.
    mtls_ca: Option<String>,
    mtls_cert: Option<String>,
    mtls_key: Option<String>,
    /// Workload identity: the issuer URL tokens name (and that serves their
    /// keys), the SPIFFE trust domain, and a signing key to use rather than
    /// share through the snapshot store or generate.
    identity_issuer: Option<String>,
    trust_domain: String,
    identity_key: Option<std::path::PathBuf>,
    /// Do not offer sandboxes' NICs checksum and segmentation offload.
    no_net_offload: bool,
    /// Templates beyond `base` (which is `HV2_INITRD`): name and initramfs,
    /// from `--template NAME=PATH`.
    templates: Vec<(String, String)>,
    /// What a template built from an OCI image gets beside it: the guest
    /// agent, busybox, init, and optionally a bash shim and CA bundle.
    guest_kit: Option<std::path::PathBuf>,
    /// Where volumes are kept; the snapshot store's `volumes` when unset,
    /// so every node sharing it has them.
    volume_dir: Option<String>,
    /// Let webhooks reach loopback, private and link-local addresses.
    allow_private_webhooks: bool,
    /// Prefault a restored guest's working set. Off by default: it halves
    /// the page faults and exits a restore takes, and did not change create
    /// latency measurably on the nested-KVM host it was tried on, where it
    /// also costs a probe restore at startup and ~3 ms of each launch. Kept,
    /// and opt-in, for a bare-metal host to measure.
    prefault: bool,
    /// When full, pause a sandbox that resumes on traffic and has been idle
    /// this long, to make room.
    evict_idle_after: Option<Duration>,
    /// Pause a sandbox that has been idle this long, whether or not the node
    /// is full, unless its create set `idleTimeout` itself. See idle.rs.
    idle_pause_after: Option<Duration>,
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
        memory_mb: 1024,
        cpu_cores: 1,
        ready_timeout: Duration::from_secs(15),
        cold_start_concurrency: None,
        network: false,
        egress_secrets_file: None,
        egress_upstream_ca: None,
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
        require_template: false,
        prefault: false,
        no_net_offload: false,
        templates: Vec::new(),
        guest_kit: std::env::var_os("HV2_GUEST_KIT").map(Into::into),
        volume_dir: None,
        allow_private_webhooks: false,
        snapshot_store: None,
        mtls_ca: None,
        mtls_cert: None,
        mtls_key: None,
        identity_issuer: None,
        trust_domain: "hv2.local".to_string(),
        identity_key: None,
        evict_idle_after: None,
        idle_pause_after: None,
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
            "--memory-gb" => {
                opts.memory_mb = parse_guest_memory(&value(&mut i)?, 1024)?;
            }
            "--memory-mb" => opts.memory_mb = parse_guest_memory(&value(&mut i)?, 1)?,
            "--cpu-cores" => {
                opts.cpu_cores = value(&mut i)?.parse().map_err(|_| "--cpu-cores requires a positive u32 integer".to_string())?;
                if opts.cpu_cores == 0 {
                    return Err("--cpu-cores requires a positive u32 integer".to_string());
                }
            }
            "--network" => opts.network = true,
            "--egress-secrets-file" => opts.egress_secrets_file = Some(value(&mut i)?),
            "--egress-upstream-ca" => opts.egress_upstream_ca = Some(value(&mut i)?),
            "--no-template" => opts.no_template = true,
            "--require-template" => opts.require_template = true,
            "--prefault" => opts.prefault = true,
            "--no-net-offload" => opts.no_net_offload = true,
            "--guest-kit" => opts.guest_kit = Some(value(&mut i)?.into()),
            "--volume-dir" => opts.volume_dir = Some(value(&mut i)?),
            "--allow-private-webhooks" => opts.allow_private_webhooks = true,
            "--template" => {
                let spec = value(&mut i)?;
                let (name, path) = spec
                    .split_once('=')
                    .ok_or_else(|| format!("--template is NAME=INITRAMFS, not {spec}"))?;
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                {
                    return Err(format!(
                        "--template name {name:?}: letters, digits, - _ . only"
                    ));
                }
                opts.templates.push((name.to_string(), path.to_string()));
            }
            "--snapshot-store" => opts.snapshot_store = Some(value(&mut i)?.into()),
            "--mtls-ca" => opts.mtls_ca = Some(value(&mut i)?),
            "--mtls-cert" => opts.mtls_cert = Some(value(&mut i)?),
            "--mtls-key" => opts.mtls_key = Some(value(&mut i)?),
            "--identity-issuer" => opts.identity_issuer = Some(value(&mut i)?),
            "--trust-domain" => opts.trust_domain = value(&mut i)?,
            "--identity-key" => opts.identity_key = Some(value(&mut i)?.into()),
            "--cold-start-concurrency" => {
                let limit: usize = value(&mut i)?
                    .parse()
                    .map_err(|_| "--cold-start-concurrency requires 1..1024")?;
                if !(1..=1024).contains(&limit) {
                    return Err("--cold-start-concurrency requires 1..1024".into());
                }
                opts.cold_start_concurrency = Some(limit);
            }
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
            "--idle-pause-after" => {
                let secs: u64 = value(&mut i)?.parse().map_err(|e| format!("{e}"))?;
                idle::check_window(secs).map_err(|e| format!("--idle-pause-after: {e}"))?;
                opts.idle_pause_after = (secs > 0).then(|| Duration::from_secs(secs));
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
                    "usage: hv2-sandboxd [--port N] [--proxy-port N] [--memory-gb N | --memory-mb N] [--cpu-cores N] \
                     [--capacity N] [--cold-start-concurrency N] [--no-template | --require-template] [--prefault] [--no-net-offload] [--template NAME=INITRAMFS ...] [--guest-kit DIR] [--snapshot-store DIR] [--mtls-ca F --mtls-cert F --mtls-key F] [--identity-issuer URL] [--trust-domain D] [--identity-key PKCS8-DER] [--evict-idle-after SECS] [--idle-pause-after SECS] \
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
    if opts.no_template && opts.require_template {
        return Err("--require-template conflicts with --no-template".to_string());
    }
    Ok(opts)
}

fn parse_guest_memory(value: &str, multiplier: u64) -> Result<u64, String> {
    value.parse::<u64>().ok().filter(|amount| *amount > 0)
        .and_then(|amount| amount.checked_mul(multiplier))
        .ok_or_else(|| "guest memory requires a positive integer with a representable MiB size".to_string())
}

#[cfg(test)]
mod guest_sizing_tests {
    use super::parse_guest_memory;

    #[test]
    fn memory_conversion_rejects_zero_invalid_and_overflow() {
        for value in ["0", "-1", "true", "1.5", "18446744073709551616"] {
            assert!(parse_guest_memory(value, 1).is_err());
            assert!(parse_guest_memory(value, 1024).is_err());
        }
        assert!(parse_guest_memory("18014398509481984", 1024).is_err());
        assert_eq!(parse_guest_memory("18014398509481983", 1024).unwrap(), u64::MAX - 1023);
        assert_eq!(parse_guest_memory("2", 1024).unwrap(), 2048);
        assert_eq!(parse_guest_memory("512", 1).unwrap(), 512);
    }
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
    /// Its network as asked for, kept current by `PUT .../network`, for
    /// when it pauses into a store another node may resume it from.
    network_request: Option<NetworkRequest>,
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
    /// Original trusted owner, retained only while registration is uncertain.
    /// This is local state and is never serialized into records or guests.
    pending_registration: Option<NameReservation>,
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
    secret_scopes: Option<Arc<hv2_net::secret_substitution::ScopedStores>>,
    upstream_roots: Vec<tokio_rustls::rustls::pki_types::CertificateDer<'static>>,
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
    /// Optional cold-boot budget, held through agent readiness. Restores bypass it.
    cold_boot_slots: Option<Arc<tokio::sync::Semaphore>>,
    /// This node's membership of a cluster, if it has one.
    node: Option<NodeAgent>,
    /// Sends lifecycle events to webhooks; over the cluster store, or this
    /// node's own in memory when it has no cluster.
    events: hv2_cluster::events::Dispatcher,
    /// What sandboxes are restored from, by template name, when not booted.
    /// Grows while the node runs, as templates are built.
    templates: parking_lot::RwLock<BTreeMap<String, Arc<Template>>>,
    /// Every template this node offers, and its initramfs -- `base` always.
    initrds: parking_lot::RwLock<BTreeMap<String, String>>,
    /// Templates being built, or whose build failed, by name.
    builds: Mutex<BTreeMap<String, TemplateBuild>>,
    /// Guest ports reached through the proxy, by sandbox and port.
    forwards: forwards::Forwards,
    /// Each sandbox's metric samples and log, by sandbox ID.
    telemetry: telemetry::Telemetry,
    /// Templates' sandboxes' sizes, where they are not the node's.
    sizes: parking_lot::RwLock<BTreeMap<String, Sizes>>,
    /// Sandboxes' snapshots, by name: templates too, to a create.
    snapshots: parking_lot::RwLock<BTreeMap<String, Arc<snapshots::Snapshot>>>,
    /// Template builds by steps (E2B's `Template.build`), by build ID.
    step_builds: builds::Builds,
    /// What each offered `COPY` upload, by files hash, must present.
    upload_tokens: Mutex<HashMap<String, String>>,
    /// For pulling images to build templates from.
    http: reqwest::Client,
    metrics: NodeMetrics,
    /// Sandboxes this node suspended to disk, under `suspend_dir`.
    paused: Mutex<HashMap<String, PausedSandbox>>,
    suspend_dir: std::path::PathBuf,
    /// Each sandbox's checkpoints on this node: see checkpoints.rs.
    checkpoints: checkpoints::Index,
    /// Shared with other nodes, when `--snapshot-store` names one.
    store: Option<SnapshotStore>,
    /// This node's name in the cluster, or `local`.
    node_id: String,
    /// Signs sandboxes' workload tokens, when sandboxes have a network.
    identity: Option<Arc<identity::Identity>>,
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

// â”€â”€ E2B wire shapes -- field names taken directly from e2b-dev/E2B's
// spec/openapi.yml (`NewSandbox`, `Sandbox` schemas), not invented. â”€â”€

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
    /// Workload identity: named tokens the egress gateway mints per request.
    iam: Option<SandboxIam>,
    /// Volumes to mount, by name, at paths in the guest.
    #[serde(rename = "volumeMounts", default)]
    volume_mounts: Vec<hv2_cluster::model::VolumeMount>,
    /// Environment variables for every command the sandbox runs. Never
    /// returned: see env_vars.rs.
    #[serde(rename = "envVars", default)]
    env_vars: BTreeMap<String, String>,
    /// Seconds without use after which the sandbox pauses -- to disk, and
    /// resumed by the next request when `autoResume` is on. 0 turns off a
    /// node's `--idle-pause-after`. Not E2B's: see idle.rs.
    #[serde(rename = "idleTimeout")]
    idle_timeout: Option<u64>,
}

/// `SandboxIam`: a non-empty `tokens` map turns workload identity on.
#[derive(Debug, Default, Deserialize)]
struct SandboxIam {
    #[serde(default)]
    tokens: BTreeMap<String, SandboxIamToken>,
}

/// `SandboxIamToken`: who a token is for, and what kind it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxIamToken {
    audience: String,
    #[serde(rename = "tokenType")]
    token_type: String,
}

/// E2B's `iam.tokens`, checked: names a placeholder can carry, and the one
/// type the API accepts. A sandbox with tokens needs a network to use them
/// through, and a node with an identity to mint them.
fn iam_tokens(
    iam: Option<&SandboxIam>,
    network: bool,
) -> Result<BTreeMap<String, SandboxIamToken>, String> {
    let Some(iam) = iam else {
        return Ok(BTreeMap::new());
    };
    for (name, token) in &iam.tokens {
        if !identity::valid_token_name(name) {
            return Err(format!(
                "iam token name {name:?} cannot carry '{{', '}}' or control characters"
            ));
        }
        if token.token_type != identity::JWT_SVID {
            return Err(format!(
                "iam token {name:?}: tokenType {:?} is not supported; {} is",
                token.token_type,
                identity::JWT_SVID
            ));
        }
        if token.audience.is_empty() {
            return Err(format!("iam token {name:?} needs an audience"));
        }
    }
    if !iam.tokens.is_empty() && !network {
        return Err(
            "iam tokens are injected by the egress gateway, and this server gives sandboxes no \
             network (start it with --network)"
                .into(),
        );
    }
    Ok(iam.tokens.clone())
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
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxNetworkRule {
    transform: Option<SandboxNetworkTransform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    let paused_here = state.paused.lock().contains_key(&sandbox_id);
    let paused_in_store = !state.sandboxes.lock().contains_key(&sandbox_id)
        && state
            .store
            .as_ref()
            .is_some_and(|store| store.peek(&sandbox_id).is_some());
    if paused_here || paused_in_store {
        match resume_sandbox(&state, &sandbox_id, Some(requested.unwrap_or(300)), None).await {
            Ok(Some(descriptor)) => return (StatusCode::CREATED, Json(descriptor)).into_response(),
            Ok(None) => {}
            Err((status, e)) => return api_error(status, e),
        }
    }
    let extend_to = requested.map(|secs| now_ms() + secs.min(MAX_TIMEOUT_SECS) * 1000);
    let lock = transition_lock(&state, &sandbox_id);
    let _held = lock.lock().await;
    let found = {
        let mut sandboxes = state.sandboxes.lock();
        if extend_to.is_some()
            && sandboxes
                .get(&sandbox_id)
                .is_some_and(|live| live.pending_registration.is_some())
        {
            return api_error(
                StatusCode::CONFLICT,
                "reconcile registration before changing timeout",
            );
        }
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
    let lock = transition_lock(&state, &sandbox_id);
    let _held = lock.lock().await;
    let record = {
        let mut sandboxes = state.sandboxes.lock();
        if sandboxes
            .get(&sandbox_id)
            .is_some_and(|live| live.pending_registration.is_some())
        {
            return api_error(
                StatusCode::CONFLICT,
                "reconcile registration before changing timeout",
            );
        }
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
async fn create_v1(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewSandbox>,
) -> Response {
    counted_create(state, req, 15, headers).await
}

/// `POST /v2/sandboxes`: what current SDKs call, default lifetime 300 s.
async fn create_v2(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<NewSandbox>,
) -> Response {
    counted_create(state, req, 300, headers).await
}

async fn counted_create(
    state: Arc<AppState>,
    req: NewSandbox,
    default_timeout: u64,
    headers: HeaderMap,
) -> Response {
    let started = std::time::Instant::now();
    let response = create_sandbox(Arc::clone(&state), req, default_timeout, headers).await;
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
        "Templates sandboxes are restored from; 0 if they are booted.",
        state.templates.read().len() as f64,
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

/// One cold-boot permit. Failures and cancellation release it through Drop.
struct ColdBootAdmission {
    _permit: tokio::sync::OwnedSemaphorePermit,
    sandbox_id: String,
}

impl Drop for ColdBootAdmission {
    fn drop(&mut self) {
        tracing::debug!(target: "hv2_sandboxd::cold_admission", vm = %self.sandbox_id,
            "cold boot admission released");
    }
}

/// Bring up a sandbox's VM: from `snapshot` when given, else from the
/// template, else by booting; then its network and its envd listener.
async fn bring_up(
    state: &AppState,
    sandbox_id: &str,
    template_id: &str,
    snapshot: Option<&std::path::Path>,
    network: Option<NetworkSpec>,
    mounts: &[hv2_cluster::model::VolumeMount],
    env: &BTreeMap<String, String>,
    access_token: &str,
) -> Result<Running, (StatusCode, String)> {
    let internal = |e: String| (StatusCode::INTERNAL_SERVER_ERROR, e);
    // From a snapshot, every sandbox is the template's guest, restored, with
    // the same CID and MAC it was snapshotted with -- which is fine, because
    // each has its own vsock device and its own gateway, and nothing outside
    // this VM ever sees either.
    let template = state.templates.read().get(template_id).cloned();
    let snapshot = snapshot.or(template.as_ref().map(|t| t.snapshot.as_path()));
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
    let cold_boot_permit = if snapshot.is_none() {
        match &state.cold_boot_slots {
            Some(slots) => {
                let permit = Arc::clone(slots).acquire_owned().await.map_err(|_| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "cold boot admission unavailable".to_string(),
                    )
                })?;
                tracing::debug!(target: "hv2_sandboxd::cold_admission", vm = sandbox_id,
                    queue_ms = t0.elapsed().as_secs_f64() * 1000.0, "cold boot admitted");
                Some(ColdBootAdmission {
                    _permit: permit,
                    sandbox_id: sandbox_id.to_owned(),
                })
            }
            None => None,
        }
    } else {
        None
    };
    let initrd = state
        .initrds
        .read()
        .get(template_id)
        .cloned()
        .unwrap_or_else(|| state.opts.initrd.clone());
    // The template's size, when it has its own: a restore is of a guest
    // that size, and must be given the memory and vCPUs it was snapshotted
    // with.
    let sizes = sizes_of(state, template_id);
    let sized;
    let opts = if sizes == Sizes::of(&state.opts) {
        &state.opts
    } else {
        sized = sizes.applied(&state.opts);
        &sized
    };
    let (vm, nic) = new_vm(
        opts,
        &initrd,
        sandbox_id,
        cid,
        network.is_some().then_some(mac),
    )
    .await
    .map_err(internal)?;
    let built = t0.elapsed();
    let launched = match snapshot {
        Some(snapshot) => {
            let working_set = template
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
        let tail = guest_report(&vm).await;
        tracing::warn!("{sandbox_id}: guest never became ready: {e}; {tail}");
        let _ = vm.stop().await;
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            format!("guest never became ready: {e}; {tail}"),
        ));
    }
    drop(cold_boot_permit);
    let answered = t0.elapsed();

    let vm = Arc::new(vm);

    let network = match (network, nic) {
        (Some(spec), Some(device)) => {
            match start_network(state, sandbox_id, &vm, device, spec, snapshot.is_none()).await {
                Ok(network) => Some(network),
                Err(e) => {
                    let _ = vm.stop().await;
                    return Err(internal(e));
                }
            }
        }
        _ => None,
    };

    // Its volumes, mounted before anyone can run a command that expects
    // them. After a restore, a mount the snapshot held is detached and
    // mounted anew: its host end was the node that took the snapshot.
    if !mounts.is_empty() {
        if let Err(e) = volumes::mount(state, &vm, mounts).await {
            if let Some(network) = network {
                network.bridge.abort();
            }
            let _ = vm.stop().await;
            return Err(internal(e));
        }
    }

    // Its envVars, in the guest before anyone can start a command. Only a
    // create passes any: a resume, a fork and a build restore a guest that
    // already holds its own.
    if let Err(e) = env_vars::apply(&vm, env).await {
        if let Some(network) = network {
            network.bridge.abort();
        }
        let _ = vm.stop().await;
        return Err(internal(e));
    }

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

#[derive(Default)]
struct RegistrationContext<'a> {
    event: Option<&'a str>,
    name_operation: Option<&'a NameReservation>,
}

/// Make a running sandbox reachable and known: routed, listed, recorded.
async fn register(
    state: &AppState,
    slot: Slot,
    running: Running,
    descriptor: SandboxResponse,
    record: SandboxRecord,
    lifecycle: Lifecycle,
    network_request: Option<NetworkRequest>,
    context: RegistrationContext<'_>,
) -> Result<(), (StatusCode, String)> {
    let event = context.event;
    let sandbox_id = record.sandbox_id.clone();
    telemetry::log(
        state,
        &sandbox_id,
        "info",
        match event {
            None => format!("sandbox created from template {}", record.template_id),
            Some(kind) => kind.replace('-', " "),
        },
    );
    if !record.volume_mounts.is_empty() {
        let mounts: Vec<String> = record
            .volume_mounts
            .iter()
            .map(|m| format!("{} at {}", m.name, m.path))
            .collect();
        telemetry::log(
            state,
            &sandbox_id,
            "info",
            format!("volumes mounted: {}", mounts.join(", ")),
        );
    }
    // Resolvable by name before the sandbox is announced, so a client that
    // uses the response immediately does not race the registration.
    state
        .routes
        .insert(&sandbox_id, ENVD_PORT, running.process_addr);
    let count = {
        let mut sandboxes = state.sandboxes.lock();
        // Reload also attaches stores under this registry lock. Refresh after
        // asynchronous bring-up, so a reload that missed this unpublished
        // gateway cannot leave it holding a removed or absent scope.
        if let (Some(scopes), Some(network)) = (&state.secret_scopes, &running.network) {
            network
                .gateway
                .set_secret_store(scopes.get(&sandbox_id))
                .map_err(|_| {
                    state.routes.remove_sandbox(&sandbox_id);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "configuring sandbox secret scope failed".to_string(),
                    )
                })?;
        }
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
                pending_registration: context.name_operation.cloned(),
                lifecycle,
                network_request,
                activity: Activity::new(),
            },
        );
        u32::try_from(sandboxes.len()).unwrap_or(u32::MAX)
    };
    // Recorded before answering, so a control plane that routes the next
    // call by the store finds it.
    if let Some(operation) = context.name_operation {
        let Some(node) = &state.node else {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "named registration needs a cluster node".into(),
            ));
        };
        match node.created_named(&record, count, operation).await {
            Ok(Some(event)) => {
                if let Some(live) = state.sandboxes.lock().get_mut(&sandbox_id) {
                    live.pending_registration = None;
                }
                state.events.deliver(&event);
            }
            Ok(None) => {
                // Registration was refused before any shared writes. Remove
                // only this local guest; never delete another owner's record
                // or release uncertain pending ownership.
                let live = state.sandboxes.lock().remove(&sandbox_id);
                state.routes.remove_sandbox(&sandbox_id);
                telemetry::forget(state, &sandbox_id);
                state.transitions.lock().remove(&sandbox_id);
                if let Some(live) = live {
                    let _ = live.process_shutdown.send(());
                    if let Some(network) = live.network {
                        network.bridge.abort();
                    }
                    if let Err(error) = live.vm.stop().await {
                        tracing::warn!(%sandbox_id,%error,"stopping refused named guest failed");
                    }
                }
                if let Err(error) = node.announce(state.running()).await {
                    tracing::warn!(%error,"announcing named registration refusal failed");
                }
                return Err((
                    StatusCode::CONFLICT,
                    "named sandbox registration refused".into(),
                ));
            }
            Err(error) => {
                // The atomic write may have committed before a later failure.
                // Preserve the guest so a completed binding remains usable.
                tracing::warn!(%sandbox_id,%error,"named registration outcome requires reconciliation");
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    "named registration outcome requires reconciliation".into(),
                ));
            }
        }
    } else {
        record_event(state, &record, event.unwrap_or("sandbox-created"), count).await;
    }
    Ok(())
}

fn authorize_registration_reconciliation(
    headers: &HeaderMap,
    clustered: bool,
    token: Option<&str>,
) -> Result<(), (StatusCode, &'static str)> {
    use subtle::ConstantTimeEq;
    if !clustered {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "registration reconciliation needs a cluster node",
        ));
    }
    let token = token.ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "registration reconciliation needs cluster authentication",
    ))?;
    let sent = headers
        .get(CLUSTER_TOKEN_HEADER)
        .map(axum::http::HeaderValue::as_bytes)
        .unwrap_or_default();
    if !bool::from(sent.ct_eq(token.as_bytes())) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "registration reconciliation requires cluster authorization",
        ));
    }
    Ok(())
}

/// Recover the same preserved guest with locally retained trusted ownership.
/// Per-request authorization is checked here and by the node route middleware.
async fn reconcile_registration(
    State(state): State<Arc<AppState>>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err((status, message)) = authorize_registration_reconciliation(
        &headers,
        state.node.is_some(),
        state.opts.cluster_token.as_deref(),
    ) {
        return api_error(status, message);
    }
    let Some(node) = &state.node else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "registration reconciliation needs a cluster node",
        );
    };
    let lock = transition_lock(&state, &sandbox_id);
    let _held = lock.lock().await;
    let (operation, record, descriptor) = {
        let sandboxes = state.sandboxes.lock();
        let Some(live) = sandboxes.get(&sandbox_id) else {
            return api_error(StatusCode::NOT_FOUND, "no local running sandbox");
        };
        let Some(operation) = &live.pending_registration else {
            return api_error(StatusCode::CONFLICT, "no uncertain local registration");
        };
        (
            operation.clone(),
            live.record.clone(),
            live.descriptor.clone(),
        )
    };
    match node
        .created_named(&record, state.running(), &operation)
        .await
    {
        Ok(Some(event)) => {
            if let Some(live) = state.sandboxes.lock().get_mut(&sandbox_id) {
                live.pending_registration = None;
            }
            state.events.deliver(&event);
            Json(descriptor).into_response()
        }
        Ok(None) => api_error(
            StatusCode::CONFLICT,
            "registration reconciliation ownership refused",
        ),
        Err(error) => {
            tracing::warn!(%sandbox_id,%error,"registration reconciliation remains uncertain");
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "registration outcome still requires reconciliation",
            )
        }
    }
}

/// Record a sandbox's lifecycle event -- in the cluster store, with its
/// record, or in this node's own when it has no cluster -- and send it to
/// the webhooks that want it.
async fn record_event(state: &AppState, record: &SandboxRecord, kind: &str, running: u32) {
    let event = match &state.node {
        Some(node) => {
            let recorded = if kind == "sandbox-created" {
                node.created(record, running).await
            } else {
                node.transitioned(record, kind, running).await
            };
            match recorded {
                Ok(event) => event,
                Err(e) => {
                    tracing::warn!("recording {} in the cluster store: {e}", record.sandbox_id);
                    return;
                }
            }
        }
        None => {
            let event =
                ClusterEvent::new(kind, "local", Some(&record.sandbox_id)).with_record(record);
            let _ = state.events.store().publish(&event).await;
            event
        }
    };
    state.events.deliver(&event);
}

/// Record that a sandbox ended, as [`record_event`] does.
async fn ended(
    state: &AppState,
    sandbox_id: &str,
    record: Option<&SandboxRecord>,
    kind: &str,
    running: u32,
) {
    checkpoints::forget(state, sandbox_id);
    let event = match &state.node {
        Some(node) => match node.ended(sandbox_id, record, kind, running).await {
            Ok(Some(event)) => event,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("recording the end of {sandbox_id}: {e}");
                return;
            }
        },
        None => {
            let mut event = ClusterEvent::new(kind, "local", Some(sandbox_id));
            if let Some(record) = record {
                event = event.with_record(record);
            }
            let _ = state.events.store().publish(&event).await;
            event
        }
    };
    state.events.deliver(&event);
}

fn named_creation_operation(
    headers: &HeaderMap,
    metadata: &BTreeMap<String, String>,
    clustered: bool,
    authenticated_cluster: bool,
) -> Result<Option<NameReservation>, (StatusCode, &'static str)> {
    let header = headers.get(NAME_OPERATION_HEADER);
    let Some(name) = metadata.get("hm.name") else {
        return if header.is_some() {
            Err((
                StatusCode::BAD_REQUEST,
                "name operation requires named metadata",
            ))
        } else {
            Ok(None)
        };
    };
    let name =
        SandboxName::parse(name).map_err(|_| (StatusCode::BAD_REQUEST, "invalid sandbox name"))?;
    if !clustered {
        return if header.is_some() {
            Err((
                StatusCode::BAD_REQUEST,
                "name operation requires a cluster node",
            ))
        } else {
            Ok(None)
        };
    }
    let header = header.ok_or((
        StatusCode::CONFLICT,
        "named cluster creation requires reserved operation context",
    ))?;
    if !authenticated_cluster {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "named cluster creation requires cluster authentication",
        ));
    }
    let token = header
        .to_str()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid name operation context"))?;
    NameReservation::from_operation(name, token)
        .map(Some)
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid name operation context"))
}

fn creator_owner(headers: &HeaderMap, clustered: bool, authenticated_cluster: bool)
    -> Result<Option<hv2_cluster::ownership::OwnerId>, (StatusCode, &'static str)> {
    use hv2_cluster::ownership::{OwnerId, OWNER_HEADER};
    let mut values = headers.get_all(OWNER_HEADER).iter();
    let Some(value) = values.next() else { return Ok(None); };
    if !clustered || !authenticated_cluster {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "owner attribution requires an authenticated cluster node"));
    }
    if values.next().is_some() { return Err((StatusCode::BAD_REQUEST, "duplicate owner context")); }
    let value = value.to_str().map_err(|_| (StatusCode::BAD_REQUEST, "invalid owner context"))?;
    OwnerId::parse(value).map(Some).map_err(|_| (StatusCode::BAD_REQUEST, "invalid owner context"))
}

async fn create_sandbox(
    state: Arc<AppState>,
    req: NewSandbox,
    default_timeout: u64,
    headers: HeaderMap,
) -> Response {
    let operation = match named_creation_operation(
        &headers,
        &req.metadata,
        state.node.is_some(),
        state.opts.cluster_token.is_some(),
    ) {
        Ok(operation) => operation,
        Err((status, error)) => return api_error(status, error),
    };
    let owner_id = match creator_owner(&headers, state.node.is_some(), state.opts.cluster_token.as_ref().is_some_and(|token| !token.is_empty())) {
        Ok(owner) => owner,
        Err((status, error)) => return api_error(status, error),
    };
    drop(headers);
    if let (Some(node), Some(operation)) = (&state.node, &operation) {
        match node.name_operation_pending(operation).await {
            Ok(true) => {}
            Ok(false) => {
                return api_error(
                    StatusCode::CONFLICT,
                    "name operation is not pending for this owner",
                );
            }
            Err(_) => {
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "name operation store unavailable",
                );
            }
        }
    }
    // A template this node does not have is the caller's mistake, answered
    // before a slot is taken -- as E2B answers an unknown template.
    // A snapshot is created from as a fork is: its template restored, its
    // pages copied in. Held until then: a delete meanwhile waits for it.
    let requested = snapshots::untagged(req.template_id.as_deref().unwrap_or("base")).to_string();
    let from_snapshot = snapshots::lookup(&state, &requested);
    let template_id = from_snapshot
        .as_ref()
        .map_or_else(|| requested.clone(), |s| s.base.clone());
    if !state.initrds.read().contains_key(&template_id) {
        return api_error(
            StatusCode::NOT_FOUND,
            format!(
                "template {template_id:?} not found; this node has {}",
                state
                    .initrds
                    .read()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    // Volumes that do not exist are the caller's mistake, found before a
    // slot is taken.
    let mounts = req.volume_mounts.clone();
    if let Err(e) = volumes::check(&state, &mounts) {
        return api_error(StatusCode::BAD_REQUEST, e);
    }
    if let Err(e) = env_vars::validate(&req.env_vars) {
        return api_error(StatusCode::BAD_REQUEST, e);
    }
    // Room first, before anything is parsed or booted: a full node should
    // answer at once so a control plane can try the next one.
    let slot = match reserve(&state, create_park(&state)).await {
        Ok(slot) => slot,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let lifetime_secs = req.timeout.unwrap_or(default_timeout).min(MAX_TIMEOUT_SECS);
    let started_at_ms = now_ms();

    let lifecycle = match Lifecycle::from_request(&req, lifetime_secs, state.opts.idle_pause_after)
    {
        Ok(lifecycle) => lifecycle,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    if (lifecycle.pause_on_timeout || lifecycle.idle_pause_secs > 0)
        && !state.templates.read().contains_key(&template_id)
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "autoPause and idleTimeout need sandboxes restored from a template, and this node \
             boots them (--no-template, or the template failed to build)",
        );
    }

    // Decided before anything boots, so a policy that does not parse costs a
    // 400 and not a VM.
    let tokens = match iam_tokens(req.iam.as_ref(), state.opts.network) {
        Ok(tokens) => tokens,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
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
        Some(NetworkSpec {
            policy,
            proxy,
            tokens: tokens.clone(),
        })
    } else if wants_network {
        return api_error(
            StatusCode::BAD_REQUEST,
            "this server gives sandboxes no network interface (start it with --network)",
        );
    } else {
        None
    };

    let network_request = state.opts.network.then(|| NetworkRequest {
        allow_internet_access: req.allow_internet_access,
        network: req.network.clone(),
        iam: tokens,
    });
    let sandbox_id = new_sandbox_id();
    let access_token = new_access_token();
    let running = match bring_up(
        &state,
        &sandbox_id,
        &template_id,
        from_snapshot.as_ref().map(|s| s.file.as_path()),
        network,
        &mounts,
        &req.env_vars,
        &access_token,
    )
    .await
    {
        Ok(running) => running,
        Err((status, e)) => return api_error(status, e),
    };
    drop(from_snapshot);

    // Built once and kept, so `POST /sandboxes/{id}/connect` answers with the
    // same description rather than a second one assembled from parts. It
    // names the snapshot asked for; the record, the template under it, which
    // a pause, resume or fork of this sandbox is layered over.
    let descriptor = SandboxResponse {
        template_id: requested,
        sandbox_id: sandbox_id.clone(),
        client_id: sandbox_id.clone(),
        envd_version: ENVD_VERSION.to_string(),
        process_port: running.process_addr.port(),
        envd_host: format!("{ENVD_PORT}-{sandbox_id}"),
        proxy_port: state.opts.proxy_port,
        envd_access_token: access_token,
    };
    let sizes = sizes_of(&state, &template_id);
    let record = SandboxRecord {
        owner_id,
        sandbox_id: sandbox_id.clone(),
        node_id: state
            .node
            .as_ref()
            .map_or_else(|| "local".to_string(), |n| n.id().to_string()),
        template_id,
        started_at_ms,
        end_at_ms: started_at_ms + lifetime_secs * 1000,
        cpu_count: sizes.cpus,
        memory_mb: sizes.memory_mb,
        metadata: req.metadata,
        envd_version: ENVD_VERSION.to_string(),
        descriptor: serde_json::to_value(&descriptor).unwrap_or_default(),
        paused: false,
        portable: false,
        volume_mounts: mounts,
    };
    let registration_lock = transition_lock(&state, &sandbox_id);
    let _held = registration_lock.lock().await;
    if let Err((status, error)) = register(
        &state,
        slot,
        running,
        descriptor.clone(),
        record,
        lifecycle,
        network_request,
        RegistrationContext {
            name_operation: operation.as_ref(),
            ..RegistrationContext::default()
        },
    )
    .await
    {
        return api_error(status, error);
    }

    (StatusCode::CREATED, Json(descriptor)).into_response()
}

/// What a sandbox does when its time is up, and whether traffic wakes it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
    /// Pause after this many seconds unused; 0 never. See idle.rs.
    #[serde(default)]
    idle_pause_secs: u64,
}

impl Lifecycle {
    fn from_request(
        req: &NewSandbox,
        lifetime_secs: u64,
        node_idle: Option<Duration>,
    ) -> Result<Self, String> {
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
        let idle_pause_secs = match req.idle_timeout {
            Some(secs) => {
                idle::check_window(secs).map_err(|e| format!("idleTimeout: {e}"))?;
                secs
            }
            None => node_idle.map_or(0, |d| d.as_secs()),
        };
        Ok(Self {
            pause_on_timeout: req.auto_pause.unwrap_or(false),
            auto_resume: req.auto_resume.as_ref().is_some_and(|a| a.enabled),
            lifetime_secs,
            idle_pause_secs,
        })
    }
}

/// A sandbox's network, as decided at creation or last replaced: enough to
/// give a resumed or forked copy the same one.
#[derive(Clone)]
struct NetworkSpec {
    policy: NetworkPolicy,
    proxy: Option<Socks5Proxy>,
    /// Workload tokens its injected headers may name.
    tokens: BTreeMap<String, SandboxIamToken>,
}

/// A sandbox's network as it was asked for, E2B's fields verbatim: what a
/// paused sandbox's record keeps, so any node can build the same network
/// for it -- a [`NetworkSpec`] is decided state, and not written down.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NetworkRequest {
    allow_internet_access: Option<bool>,
    network: Option<SandboxNetworkConfig>,
    /// `iam.tokens`, which only a network can use.
    #[serde(default)]
    iam: BTreeMap<String, SandboxIamToken>,
}

impl NetworkRequest {
    /// The network this asks for, decided now -- the egress proxy is
    /// resolved and checked again, as it would be on a create.
    async fn decide(&self, opts: &Options) -> Result<NetworkSpec, String> {
        Ok(NetworkSpec {
            policy: policy_from(
                self.allow_internet_access,
                self.network.as_ref(),
                opts.egress_default,
                &opts.tenant_reserved,
            )?,
            proxy: egress_proxy_from(opts, self.network.as_ref()).await?,
            tokens: self.iam.clone(),
        })
    }
}

/// A sandbox suspended to disk.
struct PausedSandbox {
    snapshot: std::path::PathBuf,
    descriptor: SandboxResponse,
    record: SandboxRecord,
    lifecycle: Lifecycle,
    /// Decided already, when this node paused it; built from
    /// `network_request` when another node did.
    network: Option<NetworkSpec>,
    network_request: Option<NetworkRequest>,
}

/// What a snapshot store keeps beside a paused sandbox's snapshot: enough
/// for any node sharing the store to resume it.
#[derive(Serialize, Deserialize)]
struct PausedMeta {
    descriptor: SandboxResponse,
    record: SandboxRecord,
    lifecycle: Lifecycle,
    network: Option<NetworkRequest>,
}

/// A directory every node of a cluster mounts at the same path, holding
/// what a paused sandbox needs to resume on any of them: the template its
/// snapshot is layered over, the egress CA its guest trusts, and the
/// snapshot and its description.
///
/// A paused sandbox is claimed by renaming its description, which is atomic
/// on a POSIX filesystem: two nodes asked to resume the same sandbox at
/// once cannot both get it. The descriptions hold what the sandbox's
/// network was configured with, egress proxy credentials and injected
/// headers included, so they are written owner-only.
struct SnapshotStore {
    dir: std::path::PathBuf,
    // Held for the node lifetime: an offline backup takes the exclusive lock.
    _backup_lock: std::fs::File,
}

impl SnapshotStore {
    fn open(dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating snapshot store: {e}"))?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options
            .open(dir.join(".backup.lock"))
            .map_err(|e| format!("opening snapshot store backup lock: {e}"))?;
        lock.try_lock_shared()
            .map_err(|_| "snapshot store is locked for offline backup or recovery".to_string())?;
        let store = Self {
            dir: dir.to_path_buf(),
            _backup_lock: lock,
        };
        std::fs::create_dir_all(store.paused_dir())
            .map_err(|e| format!("creating paused snapshot directory: {e}"))?;
        Ok(store)
    }

    fn paused_dir(&self) -> std::path::PathBuf {
        self.dir.join("paused")
    }

    fn snapshot(&self, sandbox_id: &str) -> std::path::PathBuf {
        self.paused_dir().join(format!("{sandbox_id}.snap"))
    }

    fn meta(&self, sandbox_id: &str) -> std::path::PathBuf {
        self.paused_dir().join(format!("{sandbox_id}.json"))
    }

    /// Describe a paused sandbox, unclaimed.
    fn put(&self, sandbox_id: &str, meta: &PausedMeta) -> Result<(), String> {
        let bytes = serde_json::to_vec(meta).map_err(|e| e.to_string())?;
        let tmp = self.paused_dir().join(format!(
            ".{sandbox_id}.{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        write_private(&tmp, &bytes)?;
        std::fs::rename(&tmp, self.meta(sandbox_id)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("recording paused {sandbox_id}: {e}")
        })
    }

    /// Read a paused sandbox's description without claiming it.
    fn peek(&self, sandbox_id: &str) -> Option<PausedMeta> {
        if !valid_sandbox_id(sandbox_id) {
            return None;
        }
        let bytes = std::fs::read(self.meta(sandbox_id)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Take a paused sandbox for this node: its description moves aside,
    /// and no other node can take it until [`Self::release`] puts it back.
    fn claim(&self, sandbox_id: &str, node: &str) -> Option<(std::path::PathBuf, PausedMeta)> {
        if !valid_sandbox_id(sandbox_id) {
            return None;
        }
        let claimed = self
            .paused_dir()
            .join(format!("{sandbox_id}.json.claimed-{node}"));
        std::fs::rename(self.meta(sandbox_id), &claimed).ok()?;
        let meta = std::fs::read(&claimed)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        match meta {
            Some(meta) => Some((claimed, meta)),
            None => {
                let _ = std::fs::rename(&claimed, self.meta(sandbox_id));
                None
            }
        }
    }

    /// Give a claim back, for a resume that failed.
    fn release(&self, sandbox_id: &str, claimed: &std::path::Path) {
        if let Err(e) = std::fs::rename(claimed, self.meta(sandbox_id)) {
            tracing::warn!("giving back the claim on paused {sandbox_id}: {e}");
        }
    }

    /// Forget a claimed sandbox: resumed, or deleted.
    fn finish(&self, sandbox_id: &str, claimed: &std::path::Path) {
        let _ = std::fs::remove_file(claimed);
        let _ = std::fs::remove_file(self.snapshot(sandbox_id));
    }
}

/// Whether `id` could be one this daemon minted -- checked before it goes
/// into a path, since a request names it.
fn valid_sandbox_id(id: &str) -> bool {
    id.starts_with("sbx-") && id.len() <= 64 && id[4..].chars().all(|c| c.is_ascii_alphanumeric())
}

/// Write a file only its owner can read.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
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
    if state.templates.read().is_empty() {
        return Err((
            StatusCode::CONFLICT,
            "pausing needs sandboxes restored from a template, and this node boots them".into(),
        ));
    }
    let live = {
        let mut sandboxes = state.sandboxes.lock();
        match sandboxes.get(sandbox_id) {
            None => return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))),
            Some(live) if live.pending_registration.is_some() => {
                return Err((
                    StatusCode::CONFLICT,
                    "reconcile registration before pausing".into(),
                ));
            }
            // Chosen as idle, and a request arrived since: it is not idle,
            // and making room is not worth cutting that request off. Asked
            // for by name, a pause goes ahead regardless, as E2B's does.
            Some(live) if evicting && live.activity.busy() => {
                return Err((
                    StatusCode::CONFLICT,
                    format!("sandbox {sandbox_id} has a request in flight"),
                ));
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
    forwards::stop(state, sandbox_id);
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
        let mut sandboxes = state.sandboxes.lock();
        if let (Some(scopes), Some(network)) = (&state.secret_scopes, &live.network) {
            if network
                .gateway
                .set_secret_store(scopes.get(sandbox_id))
                .is_err()
            {
                tracing::warn!("secret policy gateway attachment failed after pause refusal");
            }
        }
        sandboxes.insert(sandbox_id.to_string(), live);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pausing {sandbox_id}: {e}"),
        ));
    }
    let _ = live.process_shutdown.send(());
    let tokens = live
        .network_request
        .as_ref()
        .map(|r| r.iam.clone())
        .unwrap_or_default();
    let network = live.network.map(|network| {
        network.bridge.abort();
        NetworkSpec {
            policy: network.gateway.policy(),
            proxy: network.gateway.egress_proxy(),
            tokens,
        }
    });
    let mut record = live.record;
    record.paused = true;
    // In a shared store, described there too -- after the snapshot, so a
    // node that finds the description finds a whole snapshot beside it.
    if let Some(store) = &state.store {
        record.portable = true;
        let meta = PausedMeta {
            descriptor: live.descriptor.clone(),
            record: record.clone(),
            lifecycle: live.lifecycle,
            network: live.network_request.clone(),
        };
        if let Err(e) = store.put(sandbox_id, &meta) {
            tracing::warn!("{e}; {sandbox_id} can resume only on this node");
            record.portable = false;
        }
    }
    state.paused.lock().insert(
        sandbox_id.to_string(),
        PausedSandbox {
            snapshot,
            descriptor: live.descriptor,
            record: record.clone(),
            lifecycle: live.lifecycle,
            network,
            network_request: live.network_request,
        },
    );
    state.metrics.pauses.inc();
    telemetry::log(state, sandbox_id, "info", "sandbox paused");
    state.metrics.pause_latency.observe(started.elapsed());
    record_event(state, &record, "sandbox-paused", state.running()).await;
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
    let known_here = state.paused.lock().contains_key(sandbox_id);
    let in_store = state
        .store
        .as_ref()
        .is_some_and(|store| store.meta(sandbox_id).exists());
    if !known_here && !in_store {
        return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}")));
    }
    let started = std::time::Instant::now();
    tracing::debug!("resume {sandbox_id}: waiting for a slot");
    let slot = reserve(state, park)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;

    // With a store, the store decides who resumes it: this node's own note
    // of a sandbox it paused may be stale, if another node has resumed it
    // since. Claimed first, then taken from the note or the description.
    let mut claim = None;
    let mut paused = match &state.store {
        Some(store) => {
            let Some((claimed, meta)) = store.claim(sandbox_id, &state.node_id) else {
                state.paused.lock().remove(sandbox_id);
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("paused {sandbox_id} was resumed or deleted elsewhere"),
                ));
            };
            claim = Some(claimed);
            match state.paused.lock().remove(sandbox_id) {
                Some(paused) => paused,
                None => PausedSandbox {
                    snapshot: store.snapshot(sandbox_id),
                    descriptor: meta.descriptor,
                    record: meta.record,
                    lifecycle: meta.lifecycle,
                    network: None,
                    network_request: meta.network,
                },
            }
        }
        None => match state.paused.lock().remove(sandbox_id) {
            Some(paused) => paused,
            None => return Err((StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))),
        },
    };
    let give_back = |paused: PausedSandbox| match (&state.store, &claim) {
        (Some(store), Some(claimed)) => store.release(sandbox_id, claimed),
        _ => {
            state.paused.lock().insert(sandbox_id.to_string(), paused);
        }
    };
    if let Some(node)=&state.node {
        let lookup_claim = match (&state.store, &claim) {
            (Some(store), Some(path)) => Some(SnapshotClaim {
                store, id: sandbox_id, path: path.clone(), released: false,
            }),
            _ => None,
        };
        let (returned_paused, ownership) = protected_paused_lookup(paused, |paused| {
            if state.store.is_none() {
                state.paused.lock().insert(sandbox_id.to_string(), paused);
            }
        }, protected_snapshot_lookup(lookup_claim, node.store().sandbox(sandbox_id))).await;
        paused = returned_paused;
        match ownership {
            Ok(Some(record))=>paused.record.owner_id=record.owner_id,
            Ok(None)=>{give_back(paused);return Err((StatusCode::NOT_FOUND,"resume ownership record missing".into()));},
            Err(_)=>{give_back(paused);return Err((StatusCode::SERVICE_UNAVAILABLE,"resume ownership unavailable".into()));},
        }
    }
    // Decided here when this node did not pause it: the description holds
    // the request, and this node decides it as it would a create's.
    let network = match (&paused.network, &paused.network_request) {
        (Some(spec), _) => Some(spec.clone()),
        (None, Some(request)) if state.opts.network => {
            let request = request.clone();
            let lookup_claim = match (&state.store, &claim) {
                (Some(store), Some(path)) => Some(SnapshotClaim {
                    store, id: sandbox_id, path: path.clone(), released: false,
                }),
                _ => None,
            };
            let (returned_paused, decision) = protected_paused_lookup(paused, |paused| {
                if state.store.is_none() {
                    state.paused.lock().insert(sandbox_id.to_string(), paused);
                }
            }, protected_snapshot_lookup(lookup_claim, request.decide(&state.opts))).await;
            paused = returned_paused;
            match decision {
                Ok(spec) => Some(spec),
                Err(e) => {
                    give_back(paused);
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, e));
                }
            }
        },
        (None, Some(_)) => {
            give_back(paused);
            return Err((
                StatusCode::CONFLICT,
                format!("{sandbox_id} has a network and this node gives sandboxes none"),
            ));
        }
        (None, None) => None,
    };
    let template_id = paused.record.template_id.clone();
    let running = match bring_up(
        state,
        sandbox_id,
        &template_id,
        Some(&paused.snapshot),
        network,
        &paused.record.volume_mounts,
        &BTreeMap::new(),
        &paused.descriptor.envd_access_token,
    )
    .await
    {
        Ok(running) => running,
        Err(e) => {
            give_back(paused);
            return Err(e);
        }
    };
    match (&state.store, &claim) {
        (Some(store), Some(claimed)) => store.finish(sandbox_id, claimed),
        _ => {
            let _ = std::fs::remove_file(&paused.snapshot);
        }
    }

    let mut descriptor = paused.descriptor;
    descriptor.process_port = running.process_addr.port();
    let mut record = paused.record;
    record.paused = false;
    record.portable = false;
    record.node_id.clone_from(&state.node_id);
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
        paused.network_request,
        RegistrationContext {
            event: Some("sandbox-resumed"),
            ..RegistrationContext::default()
        },
    )
    .await?;
    state.metrics.resumes.inc();
    state.metrics.resume_latency.observe(started.elapsed());
    Ok(Some(descriptor))
}

#[derive(Debug, Default, Deserialize)]
struct PauseRequest {
    memory: Option<bool>,
}

/// `POST /sandboxes/{id}/pause`.
/// Return shared paused metadata when a protected store operation is cancelled.
struct SnapshotClaim<'a> { store: &'a SnapshotStore, id: &'a str, path: std::path::PathBuf, released: bool }
impl SnapshotClaim<'_> {
    fn release(&mut self) -> std::io::Result<()> {
        std::fs::rename(&self.path,self.store.meta(self.id))?;
        self.released=true;Ok(())
    }
}
impl Drop for SnapshotClaim<'_> {
    fn drop(&mut self) { if !self.released { self.store.release(self.id,&self.path); } }
}

struct LookupRollback<P, F: FnMut(P)> { value: Option<P>, restore: F }
impl<P, F: FnMut(P)> Drop for LookupRollback<P, F> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() { (self.restore)(value); }
    }
}
async fn protected_paused_lookup<P, T>(paused: P, restore: impl FnMut(P), lookup: impl std::future::Future<Output = T>) -> (P, T) {
    let mut rollback = LookupRollback { value: Some(paused), restore };
    let result = lookup.await;
    (rollback.value.take().expect("paused state retained during lookup"), result)
}

// Protect pre-startup ownership/network decisions, before VM startup side effects.
// Completed lookups leave the claim with the caller's existing success/error path.
async fn protected_snapshot_lookup<T>(mut claim: Option<SnapshotClaim<'_>>, lookup: impl std::future::Future<Output = T>) -> T {
    let result = lookup.await;
    if let Some(claim) = claim.as_mut() { claim.released = true; }
    result
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AdoptOwnerRequest { principal_id: hv2_cluster::ownership::OwnerId }

async fn adopt_owner_route(State(state): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<AdoptOwnerRequest>) -> Response {
    use hv2_cluster::ownership::OwnerAdoption;
    if !valid_sandbox_id(&id) { return api_error(StatusCode::BAD_REQUEST,"invalid sandbox ID"); }
    let Some(node)=&state.node else { return api_error(StatusCode::SERVICE_UNAVAILABLE,"clustered owner adoption required"); };
    if state.opts.cluster_token.as_ref().is_none_or(|token|token.is_empty()) {
        return api_error(StatusCode::SERVICE_UNAVAILABLE,"authenticated cluster required");
    }
    let lock=transition_lock(&state,&id);let _held=lock.lock().await;
    let running={
        let live=state.sandboxes.lock();
        if live.get(&id).is_some_and(|live|live.pending_registration.is_some()) {
            return api_error(StatusCode::CONFLICT,"reconcile registration before adoption");
        }
        live.contains_key(&id)
    };
    let mut claim=if !running {
        match &state.store {
            Some(store)=>match store.claim(&id,&state.node_id) {
                Some((path,meta))=>Some((SnapshotClaim{store,id:&id,path,released:false},meta)),
                None=>return api_error(StatusCode::CONFLICT,"paused sandbox must be available for adoption"),
            },
            None=>{
                if !state.paused.lock().contains_key(&id) { return api_error(StatusCode::NOT_FOUND,"sandbox not present on node"); }
                None
            },
        }
    } else { None };
    let outcome=node.store().adopt_sandbox_owner(&id,&body.principal_id).await;
    let success=matches!(outcome,Ok(OwnerAdoption::Adopted|OwnerAdoption::AlreadyOwned));
    let mut metadata_error=false;
    if success {
        if let Some(live)=state.sandboxes.lock().get_mut(&id) { live.record.owner_id=Some(body.principal_id.clone()); }
        if let Some(paused)=state.paused.lock().get_mut(&id) { paused.record.owner_id=Some(body.principal_id.clone()); }
        if let Some((claim,meta))=&mut claim {
            let claimed=&claim.path;
            meta.record.owner_id=Some(body.principal_id.clone());
            let temporary=claimed.with_extension(format!("{}.tmp",uuid::Uuid::new_v4().simple()));
            let written=serde_json::to_vec(meta).map_err(|_|()).and_then(|bytes|write_private(&temporary,&bytes).map_err(|_|()))
                .and_then(|()|std::fs::rename(&temporary,claimed).map_err(|_|()));
            if written.is_err() { metadata_error=true;let _=std::fs::remove_file(&temporary); }
        }
    }
    if let Some((claim,_))=&mut claim { if claim.release().is_err() { metadata_error=true; } }
    if metadata_error { return api_error(StatusCode::SERVICE_UNAVAILABLE,"ownership committed; paused metadata update failed; retry same adoption"); }
    match outcome {
        Ok(OwnerAdoption::Adopted|OwnerAdoption::AlreadyOwned)=>StatusCode::NO_CONTENT.into_response(),
        Ok(OwnerAdoption::OwnerConflict)=>api_error(StatusCode::CONFLICT,"existing owner cannot be transferred"),
        Ok(OwnerAdoption::PortsPresent)=>api_error(StatusCode::CONFLICT,"remove legacy public-port reservations before adoption"),
        Ok(OwnerAdoption::SandboxMissing)=>api_error(StatusCode::NOT_FOUND,"sandbox record missing"),
        Err(_)=>api_error(StatusCode::SERVICE_UNAVAILABLE,"owner adoption store unavailable; outcome may be committed; retry same adoption"),
    }
}

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
    if state.templates.read().is_empty() {
        return api_error(
            StatusCode::CONFLICT,
            "forking needs sandboxes restored from a template, and this node boots them",
        );
    }

    let checkpoint = state.suspend_dir.join(format!(
        "{sandbox_id}-fork-{}.snap",
        uuid::Uuid::new_v4().simple()
    ));
    let (template_id, metadata, network, source_request, volume_mounts, owner_id) = {
        let lock = transition_lock(&state, &sandbox_id);
        let _held = lock.lock().await;
        let source = {
            let sandboxes = state.sandboxes.lock();
            if sandboxes
                .get(&sandbox_id)
                .is_some_and(|live| live.pending_registration.is_some())
            {
                return api_error(
                    StatusCode::CONFLICT,
                    "reconcile registration before forking",
                );
            }
            sandboxes.get(&sandbox_id).map(|live| {
                (
                    Arc::clone(&live.vm),
                    live.record.template_id.clone(),
                    live.record.metadata.clone(),
                    live.network.as_ref().map(|n| NetworkSpec {
                        policy: n.gateway.policy(),
                        proxy: n.gateway.egress_proxy(),
                        tokens: live
                            .network_request
                            .as_ref()
                            .map(|r| r.iam.clone())
                            .unwrap_or_default(),
                    }),
                    live.network_request.clone(),
                    live.record.volume_mounts.clone(),
                    live.record.owner_id.clone(),
                )
            })
        };
        let Some((vm, template_id, metadata, network, source_request, volume_mounts, owner_id)) = source
        else {
            return if state.paused.lock().contains_key(&sandbox_id) {
                api_error(
                    StatusCode::CONFLICT,
                    format!("sandbox {sandbox_id} is paused; resume it to fork it"),
                )
            } else {
                api_error(StatusCode::NOT_FOUND, format!("no sandbox {sandbox_id}"))
            };
        };
        let owner_id = if let Some(node)=&state.node {
            match node.store().sandbox(&sandbox_id).await {
                Ok(Some(record))=>record.owner_id,
                Ok(None)=>return api_error(StatusCode::NOT_FOUND,"fork source record missing"),
                Err(_)=>return api_error(StatusCode::SERVICE_UNAVAILABLE,"fork source ownership unavailable"),
            }
        } else { owner_id };
        let started = std::time::Instant::now();
        if let Err(e) = vm.checkpoint_to(&checkpoint).await {
            let _ = std::fs::remove_file(&checkpoint);
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("checkpointing {sandbox_id}: {e}"),
            );
        }
        state.metrics.checkpoint_latency.observe(started.elapsed());
        (
            template_id,
            metadata,
            network,
            source_request,
            volume_mounts,
            owner_id,
        )
    };

    // A name identifies its original sandbox. Copying it would make legacy
    // lookup ambiguous and conflict with the parent's reserved ownership.
    // Keep other caller metadata on the child, without inheriting its name.
    let mut metadata = metadata;
    metadata.remove("hm.name");

    // Concurrently: each fork is independent, and they are what a caller
    // fanning work out to N agents is waiting on.
    let forks: Vec<_> = (0..count)
        .map(|_| {
            let state = Arc::clone(&state);
            let checkpoint = checkpoint.clone();
            let template_id = template_id.clone();
            let metadata = metadata.clone();
            let network = network.clone();
            let network_request = source_request.clone();
            let volume_mounts = volume_mounts.clone();
            let owner_id = owner_id.clone();
            async move {
                let slot = reserve(&state, create_park(&state))
                    .await
                    .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
                let fork_id = new_sandbox_id();
                let access_token = new_access_token();
                let running = bring_up(
                    &state,
                    &fork_id,
                    &template_id,
                    Some(&checkpoint),
                    network,
                    &volume_mounts,
                    &BTreeMap::new(),
                    &access_token,
                )
                .await?;
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
                let sizes = sizes_of(&state, &template_id);
                let record = SandboxRecord {
                    owner_id,
                    sandbox_id: fork_id,
                    node_id: state
                        .node
                        .as_ref()
                        .map_or_else(|| "local".to_string(), |n| n.id().to_string()),
                    template_id,
                    started_at_ms,
                    end_at_ms: started_at_ms + lifetime_secs * 1000,
                    cpu_count: sizes.cpus,
                    memory_mb: sizes.memory_mb,
                    metadata,
                    envd_version: ENVD_VERSION.to_string(),
                    descriptor: serde_json::to_value(&descriptor).unwrap_or_default(),
                    paused: false,
                    portable: false,
                    volume_mounts: volume_mounts.clone(),
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
                    network_request,
                    RegistrationContext::default(),
                )
                .await?;
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
        let addr = match self.state.routes.resolve(sandbox, port).await {
            Some(addr) => addr,
            // Any port but envd's: the sandbox's own, carried into it.
            None if port != sandbox_proxy::ENVD_PORT => {
                forwards::listen(&self.state, sandbox, port).await?
            }
            None => return None,
        };
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
        let known = self
            .state
            .paused
            .lock()
            .get(sandbox)
            .map(|p| p.lifecycle.auto_resume);
        let wakes = known.unwrap_or_else(|| {
            self.state
                .store
                .as_ref()
                .and_then(|store| store.peek(sandbox))
                .is_some_and(|meta| meta.lifecycle.auto_resume)
        });
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
    sandbox_id: &str,
    vm: &Arc<AgentVM>,
    device: Arc<parking_lot::Mutex<hv2_core::devices::virtio_net_mmio::VirtioNetMmio>>,
    spec: NetworkSpec,
    configure_guest: bool,
) -> Result<LiveNetwork, String> {
    let mut builder = Gateway::builder(spec.policy).config(GatewayConfig::default());
    if let Some(authority) = &state.authority {
        builder = builder.intercept_with(Arc::clone(authority));
    }
    for root in &state.upstream_roots {
        builder = builder.upstream_root(root.clone());
    }
    let gateway = builder
        .build()
        .map_err(|e| format!("starting the gateway: {e}"))?;
    let handle = gateway.handle();
    if let Some(scopes) = &state.secret_scopes {
        handle
            .set_secret_store(scopes.get(sandbox_id))
            .map_err(|_| "configuring sandbox secret scope failed".to_string())?;
    }
    handle.set_egress_proxy(spec.proxy);
    // Workload tokens, minted here per request for the names this sandbox
    // registered: the guest's request carries a placeholder, and the token
    // exists only on the way out.
    if let (Some(identity), false) = (&state.identity, spec.tokens.is_empty()) {
        let identity = Arc::clone(identity);
        let tokens = spec.tokens;
        let sandbox = sandbox_id.to_string();
        handle.set_token_source(Some(Arc::new(move |name: &str| {
            let token = tokens.get(name)?;
            match identity.mint(&sandbox, &token.audience) {
                Ok(jwt) => Some(jwt),
                Err(e) => {
                    tracing::warn!("minting {name} for {sandbox}: {e}");
                    None
                }
            }
        })));
    }

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

/// How long a template's guest may take to boot and answer.
const TEMPLATE_BOOT_TIMEOUT: Duration = Duration::from_secs(120);

/// A booted, configured guest written to disk, that sandboxes are restored
/// from instead of booting.
struct Template {
    dir: std::path::PathBuf,
    snapshot: std::path::PathBuf,
    /// Deleted with this process. Not when it lives in a snapshot store,
    /// where other nodes -- and paused sandboxes -- depend on it.
    owned: bool,
    /// The guest pages a sandbox restored from this touches before it first
    /// answers, as guest-physical (address, length) ranges: prefaulted into
    /// every restore, so the guest does not take an exit for each. Empty
    /// unless `--prefault`, or when it could not be measured.
    working_set: Vec<(u64, u64)>,
}

impl Drop for Template {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// What a template is a function of, as a name: every input that changes
/// the guest it holds. Nodes that agree on it can share one template, and
/// only then can a snapshot layered over it resume on either.
fn template_key(opts: &Options, authority: Option<&Authority>) -> Result<String, String> {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    for path in [&opts.kernel, &opts.initrd] {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    let config = format!(
        "{}\0{}\0{}\0{}\0{}",
        guest_cmdline(opts.network),
        opts.memory_mb,
        opts.cpu_cores,
        opts.network,
        authority.map_or("", Authority::ca_pem)
    );
    hash.update(config.as_bytes());
    Ok(hash
        .finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The template in `store` for this configuration: found if some node made
/// it already, else made here and published -- into a scratch directory,
/// then renamed into place, so another node never sees half of one. If two
/// nodes race, the first rename wins and the other uses its template.
async fn shared_template(
    opts: &Options,
    authority: Option<&Authority>,
    store: &SnapshotStore,
) -> Result<Template, String> {
    let key = template_key(opts, authority)?;
    let templates = store.dir.join("templates");
    std::fs::create_dir_all(&templates).map_err(|e| format!("{}: {e}", templates.display()))?;
    let dir = templates.join(&key);
    let found = |dir: std::path::PathBuf| Template {
        snapshot: dir.join("template.snap"),
        dir,
        owned: false,
        working_set: Vec::new(),
    };
    if dir.join("template.snap").exists() {
        tracing::info!("using the shared template {key}");
        return Ok(found(dir));
    }
    let scratch = templates.join(format!(".{key}.{}", uuid::Uuid::new_v4().simple()));
    // A few attempts, looking between them for one another node published:
    // two nodes booting templates at once on one busy host have been seen
    // to leave one guest unanswering past the ready timeout.
    let mut attempt = 1;
    let mut built = loop {
        match build_template(opts, authority, scratch.clone()).await {
            Ok(built) => break built,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&scratch);
                if dir.join("template.snap").exists() {
                    tracing::info!("building template {key} failed ({e}); another node's is there");
                    return Ok(found(dir));
                }
                if attempt == 3 {
                    return Err(e);
                }
                tracing::warn!("building template {key}, attempt {attempt}: {e}; again");
                attempt += 1;
            }
        }
    };
    built.owned = false;
    match std::fs::rename(&scratch, &dir) {
        Ok(()) => {
            tracing::info!("published the shared template {key}");
            Ok(Template {
                snapshot: dir.join("template.snap"),
                dir,
                owned: false,
                working_set: std::mem::take(&mut built.working_set),
            })
        }
        Err(_) if dir.join("template.snap").exists() => {
            let _ = std::fs::remove_dir_all(&scratch);
            tracing::info!("another node published template {key} first; using theirs");
            Ok(found(dir))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            Err(format!("publishing template {key}: {e}"))
        }
    }
}

/// `GET /templates`: the templates this node offers, in E2B's `Template`
/// shape as far as it applies -- what each is, and whether its sandboxes are
/// restored from a snapshot or booted.
async fn list_templates(State(state): State<Arc<AppState>>) -> Response {
    let snapshots = state.templates.read();
    let mut listed: Vec<serde_json::Value> = state
        .initrds
        .read()
        .keys()
        .map(|name| {
            json!({
                "templateID": name,
                "buildID": name,
                "aliases": [name],
                "public": false,
                "cpuCount": sizes_of(&state, name).cpus,
                "memoryMB": sizes_of(&state, name).memory_mb,
                "envdVersion": ENVD_VERSION,
                "buildStatus": "ready",
                "snapshot": snapshots.contains_key(name),
            })
        })
        .collect();
    for (name, build) in state.builds.lock().iter() {
        let (status, detail) = match build {
            TemplateBuild::Building { image } => ("building", image.clone()),
            TemplateBuild::Failed { image, error } => ("error", format!("{image}: {error}")),
        };
        listed.push(json!({
            "templateID": name,
            "buildID": name,
            "aliases": [name],
            "public": false,
            "buildStatus": status,
            "detail": detail,
        }));
    }
    Json(listed).into_response()
}

/// A template build in progress, or one that failed.
enum TemplateBuild {
    Building { image: String },
    Failed { image: String, error: String },
}

#[derive(Debug, Deserialize)]
struct BuildTemplateRequest {
    #[serde(rename = "templateID")]
    template_id: String,
    /// An OCI image reference: `python:3.12-slim`, `ghcr.io/org/tool@sha256:...`.
    image: String,
    /// A private registry's login, used for this pull and not kept.
    username: Option<String>,
    password: Option<String>,
    /// Its sandboxes' size, E2B's fields; the node's when absent.
    #[serde(rename = "cpuCount")]
    cpu_count: Option<u32>,
    #[serde(rename = "memoryMB")]
    memory_mb: Option<u64>,
}

/// What a snapshot store keeps about a template a node built, so every node
/// sharing the store offers it too.
#[derive(Serialize, Deserialize)]
struct BuiltTemplate {
    image: String,
    digest: String,
    initramfs: String,
    /// Absent in records from before sizes: the node's own.
    #[serde(default)]
    sizes: Option<Sizes>,
}

/// `POST /templates`: build a template from an OCI image, pulled from its
/// registry by this node -- no Docker daemon. Answers 202 at once; the
/// build's progress is in `GET /templates`, and a create for the template
/// works from the moment it is `ready`.
async fn build_template_route(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BuildTemplateRequest>,
) -> Response {
    let name = req.template_id;
    if !valid_template_name(&name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!("template name {name:?}: letters, digits, - _ . only"),
        );
    }
    if state.opts.guest_kit.is_none() {
        return api_error(
            StatusCode::NOT_IMPLEMENTED,
            "this node builds no templates: start it with --guest-kit DIR",
        );
    }
    if state.initrds.read().contains_key(&name) {
        return api_error(StatusCode::CONFLICT, format!("template {name} exists"));
    }
    let sizes = match Sizes::requested(req.cpu_count, req.memory_mb, Sizes::of(&state.opts)) {
        Ok(sizes) => sizes,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    {
        let mut builds = state.builds.lock();
        if matches!(builds.get(&name), Some(TemplateBuild::Building { .. })) {
            return api_error(StatusCode::CONFLICT, format!("template {name} is building"));
        }
        builds.insert(
            name.clone(),
            TemplateBuild::Building {
                image: req.image.clone(),
            },
        );
    }
    let building = Arc::clone(&state);
    let (task_name, image) = (name.clone(), req.image.clone());
    let credentials = match (req.username, req.password) {
        (Some(username), Some(password)) => Some(oci::Credentials { username, password }),
        _ => None,
    };
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let outcome = build_from_image(&building, &task_name, &image, credentials, sizes).await;
        let mut builds = building.builds.lock();
        match outcome {
            Ok(()) => {
                builds.remove(&task_name);
                tracing::info!(
                    "template {task_name} from {image} built in {:?}",
                    started.elapsed()
                );
            }
            Err(error) => {
                tracing::warn!("building template {task_name} from {image}: {error}");
                builds.insert(task_name, TemplateBuild::Failed { image, error });
            }
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(json!({ "templateID": name, "buildStatus": "building" })),
    )
        .into_response()
}

/// A template's sandboxes' size: E2B's `cpuCount` and `memoryMB`. Fixed when
/// its image is made a template -- the template's snapshot is of a guest
/// that size, and every sandbox restored from it is that guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Sizes {
    #[serde(rename = "cpuCount")]
    cpus: u32,
    #[serde(rename = "memoryMB")]
    memory_mb: u64,
}

impl Sizes {
    /// The node's, from `--cpu-cores` and `--memory-mb`.
    fn of(opts: &Options) -> Self {
        Self {
            cpus: opts.cpu_cores,
            memory_mb: opts.memory_mb,
        }
    }

    /// What a request asks for, `default` where it asks for nothing; refused
    /// if it is no size a guest here can be.
    fn requested(cpus: Option<u32>, memory_mb: Option<u64>, default: Self) -> Result<Self, String> {
        let sizes = Self {
            cpus: cpus.unwrap_or(default.cpus),
            memory_mb: memory_mb.unwrap_or(default.memory_mb),
        };
        let host_cpus = std::thread::available_parallelism()
            .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
        if sizes.cpus == 0 || sizes.cpus > host_cpus.min(hv2_core::boot::mptable::MAX_CPUS) {
            return Err(format!(
                "cpuCount {}: between 1 and {}, this host's CPUs",
                sizes.cpus,
                host_cpus.min(hv2_core::boot::mptable::MAX_CPUS)
            ));
        }
        if !(256..=MAX_MEMORY_MB).contains(&sizes.memory_mb) || !sizes.memory_mb.is_multiple_of(2) {
            return Err(format!(
                "memoryMB {}: an even number from 256 to {MAX_MEMORY_MB}",
                sizes.memory_mb
            ));
        }
        Ok(sizes)
    }

    /// `opts`, as for a guest this size.
    fn applied(self, opts: &Options) -> Options {
        let mut sized = opts.clone();
        sized.cpu_cores = self.cpus;
        sized.memory_mb = self.memory_mb;
        sized
    }
}

/// The most memory one sandbox may have.
const MAX_MEMORY_MB: u64 = 64 * 1024;

/// The size of `template`'s sandboxes: its own, or the node's.
fn sizes_of(state: &AppState, template: &str) -> Sizes {
    state
        .sizes
        .read()
        .get(template)
        .copied()
        .unwrap_or_else(|| Sizes::of(&state.opts))
}

fn valid_template_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Pull `image`, write its initramfs, snapshot it, and offer it.
async fn build_from_image(
    state: &Arc<AppState>,
    name: &str,
    image: &str,
    credentials: Option<oci::Credentials>,
    sizes: Sizes,
) -> Result<(), String> {
    let kit_dir = state.opts.guest_kit.clone().ok_or("no --guest-kit")?;
    let kit = initramfs::GuestKit::check(&kit_dir)?;
    let pulled = oci::pull(&state.http, image, credentials).await?;
    let digest = pulled.digest.clone();
    let bytes = tokio::task::spawn_blocking(move || initramfs::build(&pulled, &kit))
        .await
        .map_err(|e| e.to_string())??;

    let images = match &state.store {
        Some(store) => store.dir.join("images"),
        None => std::env::temp_dir().join(format!("hv2-sandboxd-{}-images", std::process::id())),
    };
    std::fs::create_dir_all(&images).map_err(|e| format!("{}: {e}", images.display()))?;
    let short = digest.trim_start_matches("sha256:");
    let file = images.join(format!("{name}-{}.cpio.gz", &short[..short.len().min(16)]));
    let scratch = images.join(format!(".{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&scratch, &bytes).map_err(|e| format!("{}: {e}", scratch.display()))?;
    std::fs::rename(&scratch, &file).map_err(|e| format!("{}: {e}", file.display()))?;
    let initramfs = file.to_string_lossy().into_owned();

    offer(state, name, &initramfs, sizes).await?;
    if let Some(store) = &state.store {
        // Recorded for the other nodes, which adopt it from here.
        let built = BuiltTemplate {
            image: image.to_string(),
            digest,
            initramfs,
            sizes: Some(sizes),
        };
        let dir = store.dir.join("built");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let record = serde_json::to_vec(&built).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!(".{name}.{}", uuid::Uuid::new_v4().simple()));
        std::fs::write(&tmp, record).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join(format!("{name}.json"))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Snapshot the template in `initramfs`, its sandboxes `sizes`, and offer it
/// as `name`.
async fn offer(state: &AppState, name: &str, initramfs: &str, sizes: Sizes) -> Result<(), String> {
    let mut for_this = sizes.applied(&state.opts);
    for_this.initrd = initramfs.to_string();
    if !state.opts.no_template {
        let built = match &state.store {
            Some(store) => shared_template(&for_this, state.authority.as_deref(), store).await,
            None => {
                // A directory of its own: a rebuild replaces the template
                // under this name, and the one replaced deletes its own
                // directory when dropped -- which, shared, took the new
                // snapshot with it.
                let dir = std::env::temp_dir().join(format!(
                    "hv2-sandboxd-{}-{name}-{}",
                    std::process::id(),
                    &uuid::Uuid::new_v4().simple().to_string()[..8]
                ));
                build_template(&for_this, state.authority.as_deref(), dir).await
            }
        }?;
        state
            .templates
            .write()
            .insert(name.to_string(), Arc::new(built));
    }
    state.sizes.write().insert(name.to_string(), sizes);
    state
        .initrds
        .write()
        .insert(name.to_string(), initramfs.to_string());
    advertise_templates(state);
    Ok(())
}

/// Tell the cluster every template this node offers: those built, and the
/// snapshots, which a create names the same way.
fn advertise_templates(state: &AppState) {
    if let Some(node) = &state.node {
        let names: Vec<String> = state.initrds.read().keys().cloned().collect();
        let mut metadata: BTreeMap<_, _> = names
            .into_iter()
            .map(|name| {
                let sizes = sizes_of(state, &name);
                let info = hv2_cluster::model::TemplateInfo {
                    snapshot: state.templates.read().contains_key(&name),
                    cpu_count: sizes.cpus,
                    memory_mb: sizes.memory_mb,
                };
                (name, info)
            })
            .collect();
        let snapshots: Vec<_> = state
            .snapshots
            .read()
            .iter()
            .map(|(name, snapshot)| (name.clone(), snapshot.base.clone()))
            .collect();
        for (name, base) in snapshots {
            let sizes = sizes_of(state, &base);
            metadata.insert(
                name,
                hv2_cluster::model::TemplateInfo {
                    snapshot: state.templates.read().contains_key(&base),
                    cpu_count: sizes.cpus,
                    memory_mb: sizes.memory_mb,
                },
            );
        }
        node.set_template_metadata(metadata);
    }
}

/// Offer every template another node built into the shared store that this
/// one does not have yet -- at start, and every few seconds after.
async fn adopt_built(state: Arc<AppState>) {
    let Some(store) = &state.store else {
        return;
    };
    let dir = store.dir.join("built");
    loop {
        if let Ok(listing) = std::fs::read_dir(&dir) {
            for entry in listing.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Some(name) = file.strip_suffix(".json") else {
                    continue;
                };
                if name.starts_with('.') || state.initrds.read().contains_key(name) {
                    continue;
                }
                let Some(built) = std::fs::read(entry.path())
                    .ok()
                    .and_then(|b| serde_json::from_slice::<BuiltTemplate>(&b).ok())
                else {
                    continue;
                };
                let sizes = built.sizes.unwrap_or_else(|| Sizes::of(&state.opts));
                match offer(&state, name, &built.initramfs, sizes).await {
                    Ok(()) => tracing::info!(
                        "offering template {name} ({}), built by another node",
                        built.image
                    ),
                    Err(e) => tracing::warn!("adopting template {name}: {e}"),
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// `GET /.well-known/jwks.json`: this node's workload-token key.
async fn node_jwks(State(state): State<Arc<AppState>>) -> Response {
    let keys: Vec<serde_json::Value> = state.identity.iter().map(|i| i.jwk()).collect();
    Json(json!({ "keys": keys })).into_response()
}

/// `GET /.well-known/openid-configuration`, when an issuer is configured.
async fn node_openid_configuration(State(state): State<Arc<AppState>>) -> Response {
    match &state.opts.identity_issuer {
        Some(issuer) => Json(identity::Identity::discovery(issuer)).into_response(),
        None => api_error(
            StatusCode::NOT_FOUND,
            "no --identity-issuer: this node is not an OIDC issuer",
        ),
    }
}

/// The workload-token key every node sharing `store` signs with, published
/// the same way the egress CA is.
fn shared_identity(store: &SnapshotStore, opts: &Options) -> Result<identity::Identity, String> {
    let dir = store.dir.join("identity");
    let load = |dir: &std::path::Path| {
        let der =
            std::fs::read(dir.join("key.pk8")).map_err(|e| format!("{}: {e}", dir.display()))?;
        identity::Identity::from_pkcs8(
            &der,
            opts.identity_issuer.clone(),
            opts.trust_domain.clone(),
        )
    };
    if dir.join("key.pk8").exists() {
        return load(&dir);
    }
    let made =
        identity::Identity::generate(opts.identity_issuer.clone(), opts.trust_domain.clone())?;
    let scratch = store
        .dir
        .join(format!(".identity.{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    write_private(&scratch.join("key.pk8"), made.pkcs8())?;
    match std::fs::rename(&scratch, &dir) {
        Ok(()) => Ok(made),
        Err(_) => {
            let _ = std::fs::remove_dir_all(&scratch);
            load(&dir)
        }
    }
}

/// The egress CA every node sharing `store` signs with: loaded, or made
/// here and published the same way a template is.
fn shared_authority(store: &SnapshotStore) -> Result<Authority, String> {
    let dir = store.dir.join("egress-ca");
    let load = |dir: &std::path::Path| -> Result<Authority, String> {
        let cert = std::fs::read_to_string(dir.join("ca.pem"))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let key = std::fs::read_to_string(dir.join("ca.key"))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        Authority::from_pem(&cert, &key).map_err(|e| format!("the shared egress CA: {e}"))
    };
    if dir.join("ca.key").exists() {
        return load(&dir);
    }
    let authority = Authority::generate().map_err(|e| format!("generating the egress CA: {e}"))?;
    let scratch = store
        .dir
        .join(format!(".egress-ca.{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    write_private(&scratch.join("ca.key"), authority.key_pem().as_bytes())?;
    write_private(&scratch.join("ca.pem"), authority.ca_pem().as_bytes())?;
    match std::fs::rename(&scratch, &dir) {
        Ok(()) => Ok(authority),
        Err(_) => {
            let _ = std::fs::remove_dir_all(&scratch);
            load(&dir)
        }
    }
}

/// The kernel command line every sandbox guest boots with. `loglevel=3`: the
/// kernel's errors and panics reach the console, which a guest that never
/// answers is reported with; nothing below that, which a booting guest
/// would write a character at a time, an exit each.
fn guest_cmdline(network: bool) -> String {
    format!(
        "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=3 {}{}",
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
    initrd: &str,
    name: &str,
    cid: u64,
    mac: Option<[u8; 6]>,
) -> Result<(AgentVM, Option<NetDevice>), String> {
    let mut capabilities = CapabilitySet::default();
    capabilities.add(Capability::GuestExec);
    let vm = AgentVM::builder()
        .name(name.to_string())
        .cpu_cores(opts.cpu_cores)
        .memory_mb(opts.memory_mb)
        .capabilities(capabilities)
        .boot_linux(&opts.kernel, Some(initrd), guest_cmdline(mac.is_some()))
        .build()
        .await
        .map_err(|e| format!("building the VM: {e}"))?;
    vm.attach_guest_channel(cid)
        .await
        .map_err(|e| format!("attaching the guest channel: {e}"))?;
    // Attached before launch: virtio-mmio has no hotplug, and the kernel
    // learns where to probe from the command line `attach_net` extends.
    let nic = match mac {
        Some(mac) => {
            let nic = vm
                .vm()
                .attach_net(mac)
                .await
                .map_err(|e| format!("attaching the network device: {e}"))?;
            // The far end is always the gateway, a userspace stack that takes
            // a segment of any size: let the guest send 64 KiB at a time.
            nic.lock().set_host_offloads(!opts.no_net_offload);
            Some(nic)
        }
        None => None,
    };
    // A console, on every VM alike -- template, booted sandbox, restored
    // one -- so a guest that never answers says why: its kernel's panic, its
    // init's last words. On every VM, not only a template's, because a guest
    // restored from a template whose kernel found a UART at boot must find
    // one still there. Its buffer is capped (1 MiB), and a quiet kernel
    // (`loglevel=0`) writes little to it.
    {
        use hv2_core::{Device, SerialDevice};
        let mut console = SerialDevice::new("COM1".to_string(), 0x3F8);
        console
            .init()
            .await
            .map_err(|e| format!("attaching the console: {e}"))?;
        vm.vm()
            .devices()
            .register_device("COM1", Arc::new(tokio::sync::RwLock::new(console)))
            .await
            .map_err(|e| format!("attaching the console: {e}"))?;
        // Registered, a device is not yet reached: its ports route to it.
        vm.vm()
            .devices()
            .register_io_port_range("COM1".to_string(), 0x3F8, 0x3FF)
            .await
            .map_err(|e| format!("routing the console's ports: {e}"))?;
    }
    Ok((vm, nic))
}

/// The last of what `vm`'s guest wrote to its console, for an error that
/// says why a guest never answered.
async fn console_tail(vm: &AgentVM) -> String {
    let Some(output) = vm.console_output().await else {
        return "no console".into();
    };
    let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return "the guest wrote nothing to its console".into();
    }
    let tail = lines[lines.len().saturating_sub(15)..].join(" | ");
    format!("its console ended: {tail}")
}

/// What `vm`'s guest had to say, and whether its vCPUs were running at all:
/// each one's VM exits over half a second. A guest that wrote nothing could
/// be one that never ran (no exits), one spinning (exits but no progress),
/// or one halted and waiting (a few timer exits) -- and those are different
/// bugs.
async fn guest_report(vm: &AgentVM) -> String {
    let console = console_tail(vm).await;
    let machine = vm.vm();
    let stats = machine.all_vcpu_stats();
    let before: Vec<(u64, u64)> = stats
        .iter()
        .map(|s| {
            (
                s.exits(),
                s.io_exits.load(std::sync::atomic::Ordering::Relaxed),
            )
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let vcpus: Vec<String> = stats
        .iter()
        .zip(before)
        .enumerate()
        .map(|(i, (s, (exits, io)))| {
            format!(
                "vCPU {i}: {} exits ({} I/O) in 0.5 s, {} in all",
                s.exits() - exits,
                s.io_exits.load(std::sync::atomic::Ordering::Relaxed) - io,
                s.exits()
            )
        })
        .collect();
    // VM-level GET_IRQCHIP/GET_PIT2 sample the kernel controllers before a
    // diagnostic kick can wake the guest. No vCPU register ioctl is issued
    // here, and these independent reads are not a restoration snapshot.
    let interrupts = match machine.backend().save_machine().await {
        Ok(Some(state)) => boot_diagnostics::machine_sample(&state),
        Ok(None) => "pre-kick machine sample unavailable".into(),
        Err(error) => format!("pre-kick machine diagnostic unavailable: {error}"),
    };
    let architecture = match machine.diagnostic_vcpu_samples().await {
        Ok(states) => states
            .into_iter()
            .map(|state| boot_diagnostics::owner_diagnostic(&state))
            .collect::<Vec<_>>()
            .join(", "),
        Err(error) => format!("owner diagnostic unavailable: {error}"),
    };
    format!(
        "{console}; VM {:?}; {}; {interrupts}; {architecture}",
        vm.state(),
        vcpus.join(", ")
    )
}

/// Boot the template once, configure it as every sandbox needs, and write it
/// to disk with its memory as an image a restore can map.
async fn build_template(
    opts: &Options,
    authority: Option<&Authority>,
    dir: std::path::PathBuf,
) -> Result<Template, String> {
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut template = Template {
        snapshot: dir.join("template.snap"),
        dir,
        owned: true,
        working_set: Vec::new(),
    };

    let started = std::time::Instant::now();
    let (vm, _nic) = new_vm(
        opts,
        &opts.initrd,
        "template",
        TEMPLATE_CID,
        opts.network.then_some(TEMPLATE_MAC),
    )
    .await?;
    vm.launch().await.map_err(|e| format!("launching: {e}"))?;
    let result = async {
        // Booted once, at startup, so allowed longer than a sandbox is: a
        // template from an OCI image first unpacks its whole filesystem --
        // 130 MiB for python:3.12-slim -- and missed 15 s on a busy host.
        if let Err(e) = vm
            .ping_guest(opts.ready_timeout.max(TEMPLATE_BOOT_TIMEOUT))
            .await
        {
            return Err(format!(
                "the template's agent never answered: {e}; {}",
                guest_report(&vm).await
            ));
        }
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
        &opts.initrd,
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
    if let Some(live) = state.sandboxes.lock().get_mut(&sandbox_id) {
        // The update replaces the rules; the tokens were registered at
        // creation and are not part of it.
        let iam = live
            .network_request
            .take()
            .map(|r| r.iam)
            .unwrap_or_default();
        live.network_request = Some(NetworkRequest {
            allow_internet_access: None,
            network: Some(update),
            iam,
        });
    }
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
    let (vm, _active) = {
        let sandboxes = state.sandboxes.lock();
        match sandboxes.get(&sandbox_id) {
            // In use for as long as the command runs: an idle pause must not
            // freeze a quiet command halfway (see idle.rs).
            Some(s) => (Arc::clone(&s.vm), ActivityGuard::enter(&s.activity)),
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
        // A paused one has only its snapshot and its record to lose -- and
        // in a store, only once it is claimed, so a sandbox another node is
        // resuming is not deleted from under it.
        let paused = state.paused.lock().remove(sandbox_id);
        let claimed = match &state.store {
            Some(store) => match store.claim(sandbox_id, &state.node_id) {
                Some((claimed, _)) => {
                    store.finish(sandbox_id, &claimed);
                    true
                }
                None => false,
            },
            None => false,
        };
        drop(held);
        state.transitions.lock().remove(sandbox_id);
        if let Some(paused) = &paused {
            if state.store.is_none() {
                let _ = std::fs::remove_file(&paused.snapshot);
            }
        }
        // With a store, only a claim makes it this node's to delete: a note
        // of a sandbox this node paused, claimed since by another that has
        // resumed it, is not a sandbox here -- and its record, which now
        // names the other node, is not this node's to remove.
        let ours = if state.store.is_some() {
            claimed
        } else {
            paused.is_some()
        };
        if !ours {
            return false;
        }
        state.metrics.ended_deleted.inc();
        ended(
            state,
            sandbox_id,
            paused.as_ref().map(|p| &p.record),
            kind,
            running,
        )
        .await;
        return true;
    };
    drop(held);
    state.transitions.lock().remove(sandbox_id);
    telemetry::forget(state, sandbox_id);
    if kind == "sandbox-expired" {
        state.metrics.ended_expired.inc();
    } else {
        state.metrics.ended_deleted.inc();
    }
    // Stop resolving the name first: a request that arrives during teardown
    // should fail to route rather than be sent at a VM that is stopping.
    forwards::stop(state, sandbox_id);
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
    ended(state, sandbox_id, Some(&live.record), kind, running).await;
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
        idle::pause_idle(&state, now).await;
    }
}

/// Ctrl-C or SIGTERM.
async fn shutdown_signal() {
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

    let store = match &opts.snapshot_store {
        None => None,
        Some(dir) => match SnapshotStore::open(dir) {
            Ok(store) => Some(store),
            Err(e) => {
                eprintln!("hv2-sandboxd: {e}");
                return std::process::ExitCode::FAILURE;
            }
        },
    };
    let authority = if opts.network {
        let made = match &store {
            Some(store) => shared_authority(store),
            None => Authority::generate().map_err(|e| format!("generating the egress CA: {e}")),
        };
        match made {
            Ok(authority) => Some(Arc::new(authority)),
            Err(e) => {
                eprintln!("hv2-sandboxd: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let upstream_roots = match &opts.egress_upstream_ca {
        None => Vec::new(),
        Some(path) => {
            if !opts.network {
                eprintln!("hv2-sandboxd: --egress-upstream-ca requires --network");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = path;
                eprintln!("hv2-sandboxd: private upstream root loading requires Linux");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(target_os = "linux")]
            match hv2_net::gateway::mitm::upstream_roots_from_private_file(std::path::Path::new(
                path,
            )) {
                Ok(roots) => roots,
                Err(_) => {
                    eprintln!("hv2-sandboxd: private upstream root validation failed");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
    };
    let secret_scopes = match &opts.egress_secrets_file {
        None => None,
        Some(path) => {
            if !opts.network {
                eprintln!("hv2-sandboxd: --egress-secrets-file requires --network");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = path;
                eprintln!("hv2-sandboxd: private secret policy loading requires Linux");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(target_os = "linux")]
            match hv2_net::secret_substitution::ScopedStores::from_file(std::path::Path::new(path))
            {
                Ok(scopes) => Some(Arc::new(scopes)),
                Err(_) => {
                    eprintln!("hv2-sandboxd: private secret policy validation failed");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
    };
    // Workload identity, for sandboxes with a network: a key given, shared
    // through the snapshot store so every node signs alike, or made here.
    let identity = if opts.network {
        let made = match (&opts.identity_key, &store) {
            (Some(path), _) => std::fs::read(path)
                .map_err(|e| format!("{}: {e}", path.display()))
                .and_then(|der| {
                    identity::Identity::from_pkcs8(
                        &der,
                        opts.identity_issuer.clone(),
                        opts.trust_domain.clone(),
                    )
                }),
            (None, Some(store)) => shared_identity(store, &opts),
            (None, None) => identity::Identity::generate(
                opts.identity_issuer.clone(),
                opts.trust_domain.clone(),
            ),
        };
        match made {
            Ok(identity) => Some(Arc::new(identity)),
            Err(e) => {
                eprintln!("hv2-sandboxd: workload identity: {e}");
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
    // Normally a failed build falls back to cold boot. Operators requiring
    // snapshot latency can refuse that fallback before listening or joining.
    let mut initrds = BTreeMap::new();
    initrds.insert("base".to_string(), opts.initrd.clone());
    for (name, path) in &opts.templates {
        initrds.insert(name.clone(), path.clone());
    }
    let mut templates = BTreeMap::new();
    if !opts.no_template {
        for (name, initrd) in &initrds {
            // Each template is the guest its own initramfs boots: the same
            // build, with that initramfs.
            let mut for_this = opts.clone();
            for_this.initrd.clone_from(initrd);
            let built = match &store {
                Some(store) => shared_template(&for_this, authority.as_deref(), store).await,
                None => {
                    let dir = std::env::temp_dir()
                        .join(format!("hv2-sandboxd-{}-{name}", std::process::id()));
                    build_template(&for_this, authority.as_deref(), dir).await
                }
            };
            match built {
                Ok(template) => {
                    templates.insert(name.clone(), Arc::new(template));
                }
                Err(e) if opts.require_template => {
                    eprintln!("hv2-sandboxd: required template {name} failed: {e}");
                    return std::process::ExitCode::FAILURE;
                }
                Err(e) => tracing::warn!(
                    "no snapshot for template {name} ({e}); its sandboxes will boot instead"
                ),
            }
        }
    }
    let template_line = if templates.is_empty() {
        "sandboxes boot from the kernel (no template snapshot)".to_string()
    } else {
        format!(
            "sandboxes are restored from template snapshots: {}",
            templates.keys().cloned().collect::<Vec<_>>().join(", ")
        )
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
                    jwk: identity.as_ref().map(|i| i.jwk()),
                    templates: initrds.keys().cloned().collect(),
                },
            );
            agent.set_template_metadata(
                initrds
                    .keys()
                    .map(|name| {
                        (
                            name.clone(),
                            hv2_cluster::model::TemplateInfo {
                                snapshot: templates.contains_key(name),
                                cpu_count: opts.cpu_cores,
                                memory_mb: opts.memory_mb,
                            },
                        )
                    })
                    .collect(),
            );
            if let Err(e) = agent.join().await {
                eprintln!("hv2-sandboxd: joining the cluster: {e}");
                return std::process::ExitCode::FAILURE;
            }
            Some(agent)
        }
    };

    // Where paused sandboxes and fork checkpoints go: the store's, if there
    // is one; else beside the template, whose image every one of them is
    // layered over.
    let suspend_dir = match &store {
        Some(store) => store.paused_dir(),
        None => std::env::temp_dir().join(format!("hv2-sandboxd-{}-suspended", std::process::id())),
    };
    if let Err(e) = std::fs::create_dir_all(&suspend_dir) {
        eprintln!("hv2-sandboxd: {}: {e}", suspend_dir.display());
        return std::process::ExitCode::FAILURE;
    }

    let routes = Arc::new(PortMap::new());
    let opts_capacity = opts.capacity as usize;
    let cold_boot_slots = opts
        .cold_start_concurrency
        .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit)));
    let event_store: Arc<dyn hv2_cluster::store::ClusterStore> = match &node {
        Some(node) => Arc::clone(node.store()),
        None => Arc::new(hv2_cluster::store::MemoryStore::new()),
    };
    let events =
        hv2_cluster::events::Dispatcher::new(Arc::clone(&event_store), opts.allow_private_webhooks);
    let state = Arc::new(AppState {
        events,
        checkpoints: parking_lot::Mutex::new(HashMap::new()),
        authority,
        secret_scopes,
        upstream_roots,
        opts,
        sandboxes: Mutex::new(HashMap::new()),
        next_cid: Mutex::new(0),
        routes: Arc::clone(&routes),
        slots: Arc::new(tokio::sync::Semaphore::new(opts_capacity)),
        cold_boot_slots,
        templates: parking_lot::RwLock::new(templates),
        initrds: parking_lot::RwLock::new(initrds),
        builds: Mutex::new(BTreeMap::new()),
        sizes: parking_lot::RwLock::new(BTreeMap::new()),
        telemetry: parking_lot::Mutex::new(HashMap::new()),
        forwards: forwards::Forwards::default(),
        snapshots: parking_lot::RwLock::new(BTreeMap::new()),
        step_builds: parking_lot::Mutex::new(HashMap::new()),
        upload_tokens: Mutex::new(HashMap::new()),
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default(),
        metrics: NodeMetrics::default(),
        node: node.clone(),
        paused: Mutex::new(HashMap::new()),
        suspend_dir,
        store,
        identity,
        node_id: node
            .as_ref()
            .map_or_else(|| "local".to_string(), |n| n.id().to_string()),
        transitions: Mutex::new(HashMap::new()),
    });
    if let Some(node) = node.clone() {
        let beating = Arc::clone(&state);
        tokio::spawn(node.heartbeat(move || beating.running()));
    }
    #[cfg(target_os = "linux")]
    if let Some(scopes) = state.secret_scopes.clone() {
        let mut reload = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        {
            Ok(signal) => signal,
            Err(_) => {
                eprintln!("hv2-sandboxd: registering secret policy reload failed");
                return std::process::ExitCode::FAILURE;
            }
        };
        let selected = Arc::clone(&state);
        tokio::spawn(async move {
            while reload.recv().await.is_some() {
                let path = selected
                    .opts
                    .egress_secrets_file
                    .as_ref()
                    .expect("configured policy");
                if scopes.rotate_file(std::path::Path::new(path)).is_err() {
                    tracing::warn!("secret policy reload refused; active scopes retained");
                    continue;
                }
                for (id, sandbox) in selected.sandboxes.lock().iter() {
                    if let Some(network) = &sandbox.network {
                        if network.gateway.set_secret_store(scopes.get(id)).is_err() {
                            tracing::warn!("secret policy gateway attachment failed");
                        }
                    }
                }
                tracing::info!("secret policy reload completed");
            }
        });
    }
    tokio::spawn(expire(Arc::clone(&state)));
    tokio::spawn(adopt_built(Arc::clone(&state)));
    tokio::spawn(snapshots::follow_store(Arc::clone(&state)));
    tokio::spawn(telemetry::sample(Arc::clone(&state)));

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
    // Mutual TLS, when configured, for both of this node's ports: only a
    // peer whose certificate the cluster's CA signed -- a control plane --
    // gets a handshake at all. It replaces --tls-cert/--tls-key, which
    // authenticate this end only.
    let mtls = match (
        &state.opts.mtls_ca,
        &state.opts.mtls_cert,
        &state.opts.mtls_key,
    ) {
        (None, None, None) => None,
        (Some(ca), Some(cert), Some(key)) => {
            let loaded = hv2_cluster::mtls::Mtls::load(
                std::path::Path::new(ca),
                std::path::Path::new(cert),
                std::path::Path::new(key),
                hv2_cluster::mtls::DEFAULT_NODE_NAME,
            )
            .and_then(|mtls| mtls.server_config());
            match loaded {
                Ok(config) => Some(config),
                Err(e) => {
                    eprintln!("hv2-sandboxd: mTLS: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
        _ => {
            eprintln!("hv2-sandboxd: --mtls-ca, --mtls-cert and --mtls-key go together");
            return std::process::ExitCode::FAILURE;
        }
    };
    if mtls.is_some()
        && state
            .opts
            .advertise_api
            .as_deref()
            .is_some_and(|api| !api.starts_with("https://"))
    {
        eprintln!("hv2-sandboxd: with mTLS, --advertise-api must be an https:// URL");
        return std::process::ExitCode::FAILURE;
    }
    let tls = match (&tls_cert, &tls_key) {
        _ if mtls.is_some() => mtls.clone(),
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
        .route(
            "/sandboxes/{sandboxID}/registration/reconcile",
            post(reconcile_registration),
        )
        .route("/templates", get(list_templates).post(build_template_route))
        .route("/sandboxes/{sandboxID}/pause", post(pause_route))
        .route("/sandboxes/{sandboxID}/resume", post(resume_route))
        .route("/sandboxes/{sandboxID}/fork", post(fork_route))
        .route("/sandboxes/{sandboxID}/owner", post(adopt_owner_route))
        .route("/sandboxes/{sandboxID}/snapshots", post(snapshots::create))
        .route(
            "/sandboxes/{sandboxID}/checkpoints",
            post(checkpoints::create).get(checkpoints::list),
        )
        .route(
            "/sandboxes/{sandboxID}/checkpoints/{name}",
            axum::routing::delete(checkpoints::delete),
        )
        .route(
            "/sandboxes/{sandboxID}/checkpoints/{name}/restore",
            post(checkpoints::restore),
        )
        .route("/snapshots", get(snapshots::list))
        .route(
            "/templates/{templateID}",
            axum::routing::delete(snapshots::delete),
        )
        .route("/v3/templates", post(builds::request))
        .route(
            "/templates/{templateID}/files/{hash}",
            get(builds::file_link),
        )
        .route(
            "/v2/templates/{templateID}/builds/{buildID}",
            post(builds::start),
        )
        .route(
            "/templates/{templateID}/builds/{buildID}/status",
            get(builds::status),
        )
        .route("/templates/aliases/{alias}", get(builds::alias))
        .merge(hv2_cluster::events::router(event_store))
        .route("/volumes", get(volumes::list).post(volumes::create))
        .route(
            "/volumes/{volumeID}",
            get(volumes::get).delete(volumes::delete),
        )
        .route("/sandboxes/{sandboxID}/exec", post(exec))
        .route(
            "/sandboxes/{sandboxID}/ports/{port}/tcp",
            get(forwards::tcp_tunnel),
        )
        .route(
            "/sandboxes/{sandboxID}/ports/{port}/udp",
            get(forwards::udp_tunnel),
        )
        .route("/sandboxes/{sandboxID}/ports/{port}/udp6", get(forwards::udp_tunnel_ipv6))
        .route("/sandboxes/metrics", get(telemetry::latest))
        .route("/sandboxes/{sandboxID}/metrics", get(telemetry::metrics))
        .route("/sandboxes/{sandboxID}/logs", get(telemetry::logs_v1))
        .route("/v2/sandboxes/{sandboxID}/logs", get(telemetry::logs_v2))
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
        // No API key: the SDK sends none with an upload. The URL's token,
        // from the authenticated `GET` of the same path, stands in.
        .route("/templates/{templateID}/files/{hash}", put(builds::upload))
        // E2B's volume content API: each volume's own bearer token, which
        // the SDK's `Volume` sends in place of the API key.
        .route(
            "/volumecontent/{volumeID}/file",
            get(volumes::read_file).put(volumes::write_file),
        )
        .route(
            "/volumecontent/{volumeID}/dir",
            get(volumes::list_dir).post(volumes::make_dir),
        )
        .route(
            "/volumecontent/{volumeID}/path",
            get(volumes::stat)
                .patch(volumes::update)
                .delete(volumes::remove_path),
        )
        // Public: a verifier of a sandbox's token fetches these. In a
        // cluster the control planes serve the same, for every node.
        .route("/.well-known/jwks.json", get(node_jwks))
        .route(
            "/.well-known/openid-configuration",
            get(node_openid_configuration),
        )
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
    let served = match mtls {
        None => axum::serve(
            axum::serve::ListenerExt::tap_io(listener, hv2_api::tls::configure_api_socket),
            app,
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| e.to_string()),
        Some(config) => hv2_api::tls::serve_tls(
            listener,
            app,
            tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            shutdown_signal(),
        )
        .await
        .map_err(|e| e.to_string()),
    };
    // With a shared store, a node going away pauses what it runs rather
    // than ending it: another node resumes each on its next request, so
    // draining a node loses no sandbox. Without one, everything ends --
    // paused ones too, whose snapshots are layered over a template this
    // process is about to delete.
    if state.store.is_some() && !state.templates.read().is_empty() {
        let running: Vec<String> = state.sandboxes.lock().keys().cloned().collect();
        for id in running {
            match pause_sandbox(&state, &id, false).await {
                Ok(()) => tracing::info!("paused {id} into the store for another node"),
                Err((_, e)) => tracing::warn!("could not pause {id} on the way out: {e}"),
            }
        }
    }
    let ids: Vec<String> = if state.store.is_some() {
        state.sandboxes.lock().keys().cloned().collect()
    } else {
        state
            .sandboxes
            .lock()
            .keys()
            .chain(state.paused.lock().keys())
            .cloned()
            .collect()
    };
    for id in ids {
        end_sandbox(&state, &id, "sandbox-deleted").await;
    }
    // Without a store, paused sandboxes and fork checkpoints were this
    // process's alone, and ended with it above.
    if state.store.is_none() {
        let _ = std::fs::remove_dir_all(&state.suspend_dir);
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

    #[test]
    fn creator_context_requires_authenticated_cluster_and_one_valid_header() {
        use hv2_cluster::ownership::OWNER_HEADER;
        let mut headers = HeaderMap::new();
        assert!(creator_owner(&headers, false, false).unwrap().is_none());
        headers.insert(OWNER_HEADER, "principal-a".parse().unwrap());
        for (clustered, authenticated) in [(false, false), (false, true), (true, false)] {
            assert_eq!(creator_owner(&headers, clustered, authenticated).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        }
        assert_eq!(creator_owner(&headers, true, true).unwrap().unwrap().as_str(), "principal-a");
        headers.append(OWNER_HEADER, "principal-b".parse().unwrap());
        assert_eq!(creator_owner(&headers, true, true).unwrap_err().0, StatusCode::BAD_REQUEST);
        headers.insert(OWNER_HEADER, "bad/owner".parse().unwrap());
        assert_eq!(creator_owner(&headers, true, true).unwrap_err().0, StatusCode::BAD_REQUEST);
    }


    #[test]
    fn registration_reconciliation_requires_each_requests_cluster_credential() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            authorize_registration_reconciliation(&headers, false, Some("owned-token"))
                .unwrap_err()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            authorize_registration_reconciliation(&headers, true, None)
                .unwrap_err()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            authorize_registration_reconciliation(&headers, true, Some("owned-token"))
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
        headers.insert(CLUSTER_TOKEN_HEADER, "wrong-token".parse().unwrap());
        assert_eq!(
            authorize_registration_reconciliation(&headers, true, Some("owned-token"))
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
        headers.insert(CLUSTER_TOKEN_HEADER, "owned-token".parse().unwrap());
        assert!(authorize_registration_reconciliation(&headers, true, Some("owned-token")).is_ok());
    }
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
    #[test]
    fn named_cluster_creation_requires_valid_authenticated_operation_context() {
        let metadata = BTreeMap::from([("hm.name".into(), "guest".into())]);
        let mut headers = HeaderMap::new();
        assert_eq!(
            named_creation_operation(&headers, &metadata, true, true)
                .unwrap_err()
                .0,
            StatusCode::CONFLICT
        );
        headers.insert(
            NAME_OPERATION_HEADER,
            "invalid-private-token".parse().unwrap(),
        );
        let error = named_creation_operation(&headers, &metadata, true, true).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(!error.1.contains("invalid-private-token"));
        let owner = NameReservation::pending(SandboxName::parse("guest").unwrap());
        headers.insert(
            NAME_OPERATION_HEADER,
            owner.operation_token().parse().unwrap(),
        );
        assert_eq!(
            named_creation_operation(&headers, &metadata, true, false)
                .unwrap_err()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let operation = named_creation_operation(&headers, &metadata, true, true)
            .unwrap()
            .unwrap();
        assert!(owner.matches_pending_operation(&operation));
        assert_eq!(
            named_creation_operation(&headers, &BTreeMap::new(), true, true)
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
        let invalid = BTreeMap::from([("hm.name".into(), "bad name".into())]);
        assert_eq!(
            named_creation_operation(&headers, &invalid, true, true)
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn standalone_metadata_names_and_unnamed_cluster_requests_keep_their_protocol() {
        let metadata = BTreeMap::from([("hm.name".into(), "guest".into())]);
        let mut headers = HeaderMap::new();
        assert!(
            named_creation_operation(&headers, &metadata, false, false)
                .unwrap()
                .is_none()
        );
        assert!(
            named_creation_operation(&headers, &BTreeMap::new(), true, false)
                .unwrap()
                .is_none()
        );
        headers.insert(NAME_OPERATION_HEADER, "untrusted".parse().unwrap());
        assert_eq!(
            named_creation_operation(&headers, &metadata, false, false)
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
    }
}

#[cfg(test)]
mod backup_lock_tests {
    use super::SnapshotStore;

    #[test]
    fn nodes_share_store_lock_and_exclude_offline_backup_until_all_stop() {
        let dir = tempfile::tempdir().unwrap();
        let a = SnapshotStore::open(dir.path()).unwrap();
        let b = SnapshotStore::open(dir.path()).unwrap();
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.path().join(".backup.lock"))
            .unwrap();
        assert!(exclusive.try_lock().is_err());
        drop(a);
        assert!(exclusive.try_lock().is_err());
        drop(b);
        exclusive.try_lock().unwrap();
        assert!(SnapshotStore::open(dir.path()).is_err());
        exclusive.unlock().unwrap();
        assert!(SnapshotStore::open(dir.path()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn store_lock_refuses_symlink_without_modifying_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(".backup.lock")).unwrap();
        assert!(SnapshotStore::open(dir.path()).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
    }
}

#[cfg(test)]
mod adoption_claim_tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_lookup_restores_owned_paused_value() {
        let restored=Arc::new(Mutex::new(None));
        let output=restored.clone();
        let ready=Arc::new(tokio::sync::Notify::new());let signal=ready.clone();
        let task=tokio::spawn(async move {
            protected_paused_lookup(vec![0,255,42],move |value| { *output.lock()=Some(value); },async {
                signal.notify_one();std::future::pending::<()>().await;
            }).await
        });
        ready.notified().await;task.abort();assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(*restored.lock(),Some(vec![0,255,42]));
    }
    #[tokio::test]
    async fn completed_lookup_returns_paused_value_without_rollback() {
        let (value,result)=protected_paused_lookup(vec![0,255,42], |_| panic!("completion must not roll back"),async { Err::<(),_>("unavailable") }).await;
        assert_eq!(value,vec![0,255,42]);assert_eq!(result,Err("unavailable"));
    }

    #[tokio::test]
    async fn completed_lookup_keeps_claim_for_caller_cleanup() {
        let directory=tempfile::tempdir().unwrap();
        let store=SnapshotStore::open(directory.path()).unwrap();
        let path=store.paused_dir().join("sbx-lookup.json.claimed-node");
        std::fs::write(&path,b"preserved-description").unwrap();
        let claim=SnapshotClaim{store:&store,id:"sbx-lookup",path:path.clone(),released:false};
        let result=protected_snapshot_lookup(Some(claim),async { Err::<(),_>("lookup failed") }).await;
        assert_eq!(result,Err("lookup failed"));
        assert!(!store.meta("sbx-lookup").exists());
        assert_eq!(std::fs::read(&path).unwrap(),b"preserved-description");
        store.release("sbx-lookup",&path);
        assert_eq!(std::fs::read(store.meta("sbx-lookup")).unwrap(),b"preserved-description");
    }

    #[tokio::test]
    async fn cancelled_protected_lookup_returns_paused_description() {
        let directory=tempfile::tempdir().unwrap();
        let store=Arc::new(SnapshotStore::open(directory.path()).unwrap());
        let path=store.paused_dir().join("sbx-adoption.json.claimed-node");
        std::fs::write(&path,b"preserved-description").unwrap();
        let ready=Arc::new(tokio::sync::Notify::new());
        let task_store=store.clone();let task_ready=ready.clone();
        let task=tokio::spawn(async move {
            let claim=SnapshotClaim{store:&task_store,id:"sbx-adoption",path,released:false};
            protected_snapshot_lookup(Some(claim), async {
                task_ready.notify_one();std::future::pending::<()>().await;
            }).await;
        });
        ready.notified().await;task.abort();assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(std::fs::read(store.meta("sbx-adoption")).unwrap(),b"preserved-description");
    }
}
