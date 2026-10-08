//! Machines: long-lived VMs, as a vSphere host runs them.
//!
//! A sandbox is ephemeral: a lifetime cap, a root filesystem in RAM, gone
//! when its node restarts. A machine is the other kind. It boots from its own
//! persistent root disk, has no lifetime, keeps what it writes, comes back
//! when its guest reboots or crashes, and comes back when this daemon
//! restarts if it was running.
//!
//! - `POST /machines` `{"name", "templateID", "cpuCount", "memoryMB",
//!   "diskGiB", "autostart", "restartPolicy", "start"}` creates one. Its root
//!   disk is the template's file tree on a fresh ext4 image (sparse,
//!   `diskGiB` large), which the guest boots from as `/dev/vda`.
//! - `GET /machines`, `GET /machines/{name}`, `DELETE /machines/{name}`
//!   (stopped machines only).
//! - `POST /machines/{name}/start`, `/stop`, `/restart`.
//! - `POST /machines/{name}/exec` `{"cmd", "timeout_secs"}` runs a shell
//!   command through the guest agent; `GET /machines/{name}/console` is the
//!   end of its serial console.
//!
//! Each machine is a directory under `--machine-dir`: `machine.json` (what
//! it is and whether it should be running) and `root.img`. That file, not
//! this process, is the truth about what should be running.
//!
//! Stopping syncs the guest's filesystems through its agent first, then
//! stops the VM. There is no ACPI power button yet, so a guest's own
//! `poweroff` halts it without the VM exiting.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use hv2_agent::AgentVM;

use super::{
    api_error, new_vm, now_ms, reserve, start_network, AppState, Arc, Deserialize, IntoResponse,
    Json, LiveNetwork, NetworkRequest, NetworkSpec, Path, Response, SandboxNetworkConfig,
    Serialize, Sizes, Slot, State, StatusCode,
};

/// How long a booting machine's guest agent has to answer.
const BOOT_TIMEOUT: Duration = Duration::from_secs(60);
/// The most restarts after a guest stopped by itself within [`RESTART_WINDOW`].
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(300);

/// Serialises every lifecycle change on this node: start, stop, restart,
/// delete, and the watcher's restarts. A machine is never half-started.
static OPS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A running machine.
struct Live {
    vm: Arc<AgentVM>,
    /// Its share of capacity.
    _slot: Slot,
    started_ms: u64,
    /// Its gateway and the loop carrying its frames, when it has a NIC.
    network: Option<LiveNetwork>,
}

impl Drop for Live {
    fn drop(&mut self) {
        // However it stopped, by the API or by itself: its gateway goes too.
        if let Some(network) = &self.network {
            network.bridge.abort();
        }
    }
}

static LIVE: StdMutex<Option<HashMap<String, Live>>> = StdMutex::new(None);
static RESTARTS: StdMutex<Option<HashMap<String, VecDeque<Instant>>>> = StdMutex::new(None);

fn live<T>(f: impl FnOnce(&mut HashMap<String, Live>) -> T) -> T {
    let mut guard = LIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RestartPolicy {
    /// Boot it again when its guest stops by itself (reboot, panic).
    #[default]
    Always,
    /// Leave it stopped.
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Desired {
    Running,
    Stopped,
}

/// What `machine.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Stored {
    #[serde(rename = "machineID")]
    id: String,
    name: String,
    #[serde(rename = "templateID")]
    template: String,
    #[serde(rename = "cpuCount")]
    cpus: u32,
    #[serde(rename = "memoryMB")]
    memory_mb: u64,
    #[serde(rename = "diskGiB")]
    disk_gib: u64,
    autostart: bool,
    #[serde(rename = "restartPolicy")]
    restart: RestartPolicy,
    desired: Desired,
    #[serde(rename = "createdAt")]
    created_ms: u64,
    /// Its network, as asked for. `None` is no NIC at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network: Option<MachineNetwork>,
}

/// A machine's network: one NIC behind this node's egress gateway, which
/// decides every connection the guest makes. The same rules as a sandbox's.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct MachineNetwork {
    /// Whether it may reach the Internet. The node's default when absent.
    #[serde(
        rename = "allowInternetAccess",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    allow_internet_access: Option<bool>,
    /// Hosts, addresses and CIDRs it may reach whatever the default.
    #[serde(rename = "allowOut", default)]
    allow_out: Vec<String>,
    /// And those it may not.
    #[serde(rename = "denyOut", default)]
    deny_out: Vec<String>,
}

impl MachineNetwork {
    /// The network this asks for, decided now.
    async fn decide(&self, state: &AppState) -> Result<NetworkSpec, String> {
        NetworkRequest {
            allow_internet_access: self.allow_internet_access,
            network: Some(SandboxNetworkConfig {
                allow_out: self.allow_out.clone(),
                deny_out: self.deny_out.clone(),
                ..SandboxNetworkConfig::default()
            }),
            iam: BTreeMap::new(),
        }
        .decide(&state.opts)
        .await
    }
}

/// The MAC of a machine's one NIC. Only its own gateway ever sees it.
const MACHINE_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x00, 0x00, 0x02];

fn root(state: &AppState) -> PathBuf {
    match &state.opts.machine_dir {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir().join("hv2-sandboxd-machines"),
    }
}

fn dir_of(state: &AppState, id: &str) -> PathBuf {
    root(state).join(id)
}

fn machine_id(name: &str) -> String {
    let volume = hv2_cluster::model::volume_id(name);
    format!("vm-{}", volume.trim_start_matches("vol-"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn load(state: &AppState, id: &str) -> Option<Stored> {
    serde_json::from_slice(&std::fs::read(dir_of(state, id).join("machine.json")).ok()?).ok()
}

fn all(state: &AppState) -> Vec<Stored> {
    let Ok(listing) = std::fs::read_dir(root(state)) else {
        return Vec::new();
    };
    let mut machines: Vec<Stored> = listing
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("machine.json")).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    machines.sort_by(|a, b| a.name.cmp(&b.name));
    machines
}

/// Written beside, synced, then renamed over.
fn save(state: &AppState, machine: &Stored) -> Result<(), String> {
    let dir = dir_of(state, &machine.id);
    let temporary = dir.join(".machine.json");
    let bytes = serde_json::to_vec_pretty(machine).map_err(|e| e.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::File::open(&temporary)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, dir.join("machine.json")).map_err(|e| e.to_string())
}

fn describe(machine: &Stored) -> Value {
    let (running, started) = live(|l| {
        l.get(&machine.id)
            .map(|m| (true, Some(m.started_ms)))
            .unwrap_or((false, None))
    });
    json!({
        "machineID": machine.id,
        "name": machine.name,
        "templateID": machine.template,
        "cpuCount": machine.cpus,
        "memoryMB": machine.memory_mb,
        "diskGiB": machine.disk_gib,
        "autostart": machine.autostart,
        "restartPolicy": machine.restart,
        "desiredState": machine.desired,
        "state": if running { "running" } else { "stopped" },
        "startedAt": started,
        "createdAt": machine.created_ms,
        "network": machine.network,
    })
}

fn by_name(state: &AppState, name: &str) -> Option<Stored> {
    if !valid_name(name) {
        return None;
    }
    load(state, &machine_id(name))
}

// ── The root disk ───────────────────────────────────────────────────────────

/// One entry of a `newc` cpio archive.
enum CpioEntry {
    Dir(u32),
    File(u32, Vec<u8>),
    Symlink(String),
}

/// The entries of a gzipped `newc` cpio, by path. Device nodes and other
/// special files are skipped: the guest's init mounts devtmpfs over /dev.
fn read_cpio(gz: &[u8]) -> Result<BTreeMap<String, CpioEntry>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("the template's initramfs is not gzip: {e}"))?;
    let field = |header: &[u8], index: usize| -> Result<u32, String> {
        let text = std::str::from_utf8(&header[6 + index * 8..14 + index * 8])
            .map_err(|_| "a cpio header is not ASCII".to_string())?;
        u32::from_str_radix(text, 16).map_err(|_| "a cpio header field is not hex".to_string())
    };
    let pad = |n: usize| (4 - n % 4) % 4;
    let mut entries = BTreeMap::new();
    let mut at = 0usize;
    loop {
        let header = bytes
            .get(at..at + 110)
            .ok_or("the cpio archive ends without a trailer")?;
        if &header[..6] != b"070701" {
            return Err("not a newc cpio archive".into());
        }
        let mode = field(header, 1)?;
        let size = field(header, 6)? as usize;
        let name_size = field(header, 11)? as usize;
        let name_at = at + 110;
        let name = bytes
            .get(name_at..name_at + name_size.saturating_sub(1))
            .ok_or("a cpio name runs past the end")?;
        let name = String::from_utf8_lossy(name).into_owned();
        let data_at = name_at + name_size + pad(110 + name_size);
        let data = bytes
            .get(data_at..data_at + size)
            .ok_or("a cpio file runs past the end")?
            .to_vec();
        at = data_at + size + pad(size);
        if name == "TRAILER!!!" {
            break;
        }
        let path = name.trim_start_matches("./").trim_start_matches('/');
        if path.is_empty() || path == "." || path.split('/').any(|part| part == "..") {
            continue;
        }
        let permissions = mode & 0o7777;
        let entry = match mode & 0o170_000 {
            0o040_000 => CpioEntry::Dir(permissions),
            0o100_000 => CpioEntry::File(permissions, data),
            0o120_000 => CpioEntry::Symlink(String::from_utf8_lossy(&data).into_owned()),
            _ => continue,
        };
        entries.insert(path.to_string(), entry);
    }
    Ok(entries)
}

/// Make `image`: `size_gib` sparse, ext4, holding the template's file tree.
fn build_root(initrd: &FsPath, image: &FsPath, size_gib: u64) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let gz = std::fs::read(initrd).map_err(|e| format!("{}: {e}", initrd.display()))?;
    let entries = read_cpio(&gz)?;
    let tree = image.with_extension("tree");
    let _ = std::fs::remove_dir_all(&tree);
    std::fs::create_dir_all(&tree).map_err(|e| e.to_string())?;
    let result = (|| {
        for (path, entry) in &entries {
            let target = tree.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            match entry {
                CpioEntry::Dir(mode) => {
                    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(*mode))
                        .map_err(|e| e.to_string())?;
                }
                CpioEntry::File(mode, data) => {
                    std::fs::write(&target, data).map_err(|e| e.to_string())?;
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(*mode))
                        .map_err(|e| e.to_string())?;
                }
                CpioEntry::Symlink(to) => {
                    let _ = std::fs::remove_file(&target);
                    std::os::unix::fs::symlink(to, &target).map_err(|e| e.to_string())?;
                }
            }
        }
        std::fs::File::create_new(image)
            .and_then(|f| f.set_len(size_gib * 1024 * 1024 * 1024))
            .map_err(|e| format!("{}: {e}", image.display()))?;
        let out = std::process::Command::new("mkfs.ext4")
            .args(["-F", "-q", "-L", "root", "-d"])
            .arg(&tree)
            .arg(image)
            .output()
            .map_err(|e| format!("running mkfs.ext4 (is e2fsprogs installed?): {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "mkfs.ext4 failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    })();
    let _ = std::fs::remove_dir_all(&tree);
    result
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

/// Boot `machine` from its root disk. The caller holds [`OPS`].
async fn boot(state: &Arc<AppState>, machine: &Stored) -> Result<(), String> {
    if live(|l| l.contains_key(&machine.id)) {
        return Ok(());
    }
    let slot = reserve(state, None).await?;
    let cid = {
        let mut next = state.next_cid.lock();
        let cid = *next;
        *next += 1;
        super::GUEST_CID_BASE + cid
    };
    let sized = Sizes {
        cpus: machine.cpus,
        memory_mb: machine.memory_mb,
    }
    .applied(&state.opts);
    let image = dir_of(state, &machine.id).join("root.img");
    // Decided at every boot, as a sandbox's is on resume: the node's default
    // and reserved ranges may have changed since.
    let spec = match &machine.network {
        Some(network) => Some(network.decide(state).await?),
        None => None,
    };
    let (vm, nic) = new_vm(
        &sized,
        None,
        &machine.id,
        cid,
        spec.is_some().then_some(MACHINE_MAC),
        Some((image.as_path(), machine.id.as_str())),
    )
    .await?;
    let vm = Arc::new(vm);
    if let Err(e) = vm.launch().await {
        let _ = vm.stop().await;
        return Err(format!("launching: {e}"));
    }
    if let Err(e) = vm.ping_guest(BOOT_TIMEOUT).await {
        let report = super::guest_report(&vm).await;
        let _ = vm.stop().await;
        return Err(format!("the guest never answered: {e}; {report}"));
    }
    let network = match (spec, nic) {
        (Some(spec), Some(device)) => {
            // The guest is configured at every boot, by a script that is safe
            // to run again on a disk that kept the last boot's changes.
            match start_network(state, &machine.id, &vm, device, spec, false, false).await {
                Ok(network) => {
                    if let Err(e) =
                        configure_network(&vm, network.gateway.ca_pem().as_deref()).await
                    {
                        network.bridge.abort();
                        let _ = vm.stop().await;
                        return Err(e);
                    }
                    Some(network)
                }
                Err(e) => {
                    let _ = vm.stop().await;
                    return Err(e);
                }
            }
        }
        _ => None,
    };
    live(|l| {
        l.insert(
            machine.id.clone(),
            Live {
                vm,
                _slot: slot,
                started_ms: now_ms(),
                network,
            },
        )
    });
    tracing::info!("machine {} is running", machine.name);
    Ok(())
}

/// Point a machine's guest at its gateway's resolver and make it trust the
/// egress CA, when there is one.
///
/// Unlike a sandbox's, this runs at every boot on a disk that persists, so it
/// changes nothing it already did: the CA is appended to the trust bundle only
/// when the bundle does not hold it.
async fn configure_network(vm: &AgentVM, ca: Option<&str>) -> Result<(), String> {
    let mut script = String::from("mkdir -p /etc && ln -sf /proc/net/pnp /etc/resolv.conf");
    // A line of the certificate's base64 body: enough to recognise it by.
    let marker = ca.and_then(|ca| ca.lines().nth(1));
    if let (Some(ca), Some(marker)) = (ca, marker) {
        script.push_str(&format!(
            " && mkdir -p /etc/ssl/certs && {{ grep -qF '{marker}' /etc/ssl/certs/ca-certificates.crt 2>/dev/null || printf '%s' '{ca}' >> /etc/ssl/certs/ca-certificates.crt; }}"
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

/// Stop `id`'s VM, its filesystems synced first. The caller holds [`OPS`].
async fn halt(id: &str) {
    let Some(running) = live(|l| l.remove(id)) else {
        return;
    };
    let _ = running
        .vm
        .exec_in_guest(
            "/bin/busybox",
            &["sync".to_string()],
            Duration::from_secs(15),
        )
        .await;
    if let Err(e) = running.vm.stop().await {
        tracing::warn!("stopping machine {id}: {e}");
    }
}

/// Bring machines back after this daemon starts, then watch for guests that
/// stop by themselves, restarting them by their policy.
pub(crate) async fn supervise(state: Arc<AppState>) {
    {
        let _ops = OPS.lock().await;
        for mut machine in all(&state) {
            if machine.desired != Desired::Running {
                continue;
            }
            if !machine.autostart {
                machine.desired = Desired::Stopped;
                let _ = save(&state, &machine);
                continue;
            }
            match boot(&state, &machine).await {
                Ok(()) => tracing::info!("machine {} started with the daemon", machine.name),
                Err(e) => tracing::warn!("machine {} did not start: {e}", machine.name),
            }
        }
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let stopped: Vec<String> = live(|l| {
            l.iter()
                .filter(|(_, m)| m.vm.state() == hv2_core::VMState::Stopped)
                .map(|(id, _)| id.clone())
                .collect()
        });
        for id in stopped {
            let _ops = OPS.lock().await;
            // Rechecked under the lock: a stop or restart may have run.
            let still = live(|l| {
                l.get(&id)
                    .is_some_and(|m| m.vm.state() == hv2_core::VMState::Stopped)
            });
            if !still {
                continue;
            }
            live(|l| l.remove(&id));
            let Some(mut machine) = load(&state, &id) else {
                continue;
            };
            if machine.desired != Desired::Running {
                continue;
            }
            let allowed = machine.restart == RestartPolicy::Always && admit_restart(&id);
            if !allowed {
                tracing::warn!(
                    "machine {}: its guest stopped; leaving it stopped",
                    machine.name
                );
                machine.desired = Desired::Stopped;
                let _ = save(&state, &machine);
                continue;
            }
            tracing::info!(
                "machine {}: its guest stopped by itself; restarting it",
                machine.name
            );
            if let Err(e) = boot(&state, &machine).await {
                tracing::warn!("machine {}: restart failed: {e}", machine.name);
            }
        }
    }
}

fn admit_restart(id: &str) -> bool {
    let mut guard = RESTARTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let recent = guard
        .get_or_insert_with(HashMap::new)
        .entry(id.to_string())
        .or_default();
    let now = Instant::now();
    while recent
        .front()
        .is_some_and(|t| now.duration_since(*t) > RESTART_WINDOW)
    {
        recent.pop_front();
    }
    if recent.len() >= MAX_RESTARTS {
        return false;
    }
    recent.push_back(now);
    true
}

// ── The API ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct NewMachine {
    name: String,
    #[serde(rename = "templateID")]
    template: Option<String>,
    #[serde(rename = "cpuCount")]
    cpus: Option<u32>,
    #[serde(rename = "memoryMB")]
    memory_mb: Option<u64>,
    #[serde(rename = "diskGiB")]
    disk_gib: Option<u64>,
    autostart: Option<bool>,
    #[serde(rename = "restartPolicy")]
    restart: Option<RestartPolicy>,
    start: Option<bool>,
    network: Option<MachineNetwork>,
}

/// `POST /machines`.
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NewMachine>,
) -> Response {
    if !valid_name(&req.name) {
        return api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "machine name {:?}: letters, digits, - and _, up to 63",
                req.name
            ),
        );
    }
    let template = req.template.unwrap_or_else(|| "base".into());
    let Some(initrd) = state.initrds.read().get(&template).cloned() else {
        return api_error(StatusCode::NOT_FOUND, format!("no template {template}"));
    };
    let disk_gib = req.disk_gib.unwrap_or(8);
    if !(1..=2048).contains(&disk_gib) {
        return api_error(StatusCode::BAD_REQUEST, "diskGiB is 1 to 2048");
    }
    if let Some(network) = &req.network {
        // Refused now, not at its first boot.
        if let Err(e) = network.decide(&state).await {
            return api_error(StatusCode::BAD_REQUEST, format!("network: {e}"));
        }
    }
    let node = Sizes::of(&state.opts);
    let machine = Stored {
        id: machine_id(&req.name),
        name: req.name,
        template,
        cpus: req.cpus.unwrap_or(node.cpus).clamp(1, 64),
        memory_mb: req
            .memory_mb
            .unwrap_or(node.memory_mb)
            .clamp(128, 1024 * 1024),
        disk_gib,
        autostart: req.autostart.unwrap_or(true),
        restart: req.restart.unwrap_or_default(),
        desired: Desired::Stopped,
        created_ms: now_ms(),
        network: req.network,
    };
    let dir = dir_of(&state, &machine.id);
    if let Err(e) = std::fs::create_dir_all(root(&state)) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return api_error(
                StatusCode::CONFLICT,
                format!("machine {} exists", machine.name),
            );
        }
        Err(e) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
    let image = dir.join("root.img");
    let initrd = PathBuf::from(initrd);
    let built = tokio::task::spawn_blocking(move || build_root(&initrd, &image, disk_gib))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r)
        .and_then(|()| save(&state, &machine));
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&dir);
        return api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("making the root disk: {e}"),
        );
    }
    if req.start.unwrap_or(true) {
        if let Err(response) = set_running(&state, &machine.id, true).await {
            return response;
        }
    }
    let machine = load(&state, &machine.id).unwrap_or(machine);
    (StatusCode::CREATED, Json(describe(&machine))).into_response()
}

/// Start or stop `id` and record it as what should be. Holds [`OPS`].
// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
async fn set_running(state: &Arc<AppState>, id: &str, running: bool) -> Result<(), Response> {
    let _ops = OPS.lock().await;
    let Some(mut machine) = load(state, id) else {
        return Err(api_error(StatusCode::NOT_FOUND, format!("no machine {id}")));
    };
    if running {
        boot(state, &machine)
            .await
            .map_err(|e| api_error(StatusCode::SERVICE_UNAVAILABLE, format!("starting: {e}")))?;
        machine.desired = Desired::Running;
    } else {
        halt(id).await;
        machine.desired = Desired::Stopped;
    }
    save(state, &machine).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// `GET /machines`.
pub(crate) async fn list(State(state): State<Arc<AppState>>) -> Response {
    Json(all(&state).iter().map(describe).collect::<Vec<_>>()).into_response()
}

/// `GET /machines/{name}`.
pub(crate) async fn get(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    match by_name(&state, &name) {
        Some(machine) => Json(describe(&machine)).into_response(),
        None => api_error(StatusCode::NOT_FOUND, format!("no machine {name}")),
    }
}

/// `POST /machines/{name}/{start,stop,restart}`.
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    Path((name, action)): Path<(String, String)>,
) -> Response {
    let Some(machine) = by_name(&state, &name) else {
        return api_error(StatusCode::NOT_FOUND, format!("no machine {name}"));
    };
    let result = match action.as_str() {
        "start" => set_running(&state, &machine.id, true).await,
        "stop" => set_running(&state, &machine.id, false).await,
        "restart" => match set_running(&state, &machine.id, false).await {
            Ok(()) => set_running(&state, &machine.id, true).await,
            Err(e) => Err(e),
        },
        other => {
            return api_error(
                StatusCode::NOT_FOUND,
                format!("no action {other}: start, stop or restart"),
            )
        }
    };
    match result {
        Ok(()) => {
            let machine = load(&state, &machine.id).unwrap_or(machine);
            Json(describe(&machine)).into_response()
        }
        Err(response) => response,
    }
}

/// `DELETE /machines/{name}`: a stopped machine and its disk.
pub(crate) async fn delete(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let _ops = OPS.lock().await;
    let Some(machine) = by_name(&state, &name) else {
        return api_error(StatusCode::NOT_FOUND, format!("no machine {name}"));
    };
    if live(|l| l.contains_key(&machine.id)) {
        return api_error(
            StatusCode::CONFLICT,
            format!("machine {name} is running; stop it first"),
        );
    }
    match std::fs::remove_dir_all(dir_of(&state, &machine.id)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExecRequest {
    cmd: String,
    timeout_secs: Option<u64>,
}

// The error is the reply, returned by the handler at once.
#[allow(clippy::result_large_err)]
fn running_vm(state: &AppState, name: &str) -> Result<Arc<AgentVM>, Response> {
    let machine = by_name(state, name)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("no machine {name}")))?;
    live(|l| l.get(&machine.id).map(|m| Arc::clone(&m.vm))).ok_or_else(|| {
        api_error(
            StatusCode::CONFLICT,
            format!("machine {name} is not running"),
        )
    })
}

/// `POST /machines/{name}/exec`.
pub(crate) async fn exec(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Response {
    let vm = match running_vm(&state, &name) {
        Ok(vm) => vm,
        Err(response) => return response,
    };
    let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(30).clamp(1, 3600));
    match vm
        .exec_in_guest("/bin/sh", &["-c".to_string(), req.cmd], timeout)
        .await
    {
        Ok(out) => Json(json!({
            "exit_code": out.exit_code,
            "signal": out.signal,
            "stdout": out.stdout,
            "stderr": out.stderr,
        }))
        .into_response(),
        Err(e) => api_error(StatusCode::BAD_GATEWAY, format!("exec: {e}")),
    }
}

/// `GET /machines/{name}/network/decisions`: what its gateway allowed and
/// refused, since this boot.
pub(crate) async fn network_decisions(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let Some(machine) = by_name(&state, &name) else {
        return api_error(StatusCode::NOT_FOUND, format!("no machine {name}"));
    };
    let gateway = live(|l| {
        l.get(&machine.id)
            .map(|m| m.network.as_ref().map(|n| n.gateway.clone()))
    });
    match gateway {
        None => api_error(
            StatusCode::CONFLICT,
            format!("machine {name} is not running"),
        ),
        Some(None) => api_error(StatusCode::BAD_REQUEST, "this machine has no network"),
        Some(Some(gateway)) => Json(super::gateway_decisions(&gateway)).into_response(),
    }
}

/// `GET /machines/{name}/console`: the end of its serial console.
pub(crate) async fn console(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    match running_vm(&state, &name) {
        Ok(vm) => super::guest_report(&vm).await.into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A newc archive built the way `initramfs.rs` and `cpio -H newc` build
    /// them, read back: directories, files with their modes, symlinks; device
    /// nodes and escapes skipped.
    #[test]
    fn a_newc_initramfs_reads_back_as_its_tree() {
        use std::io::Write;
        fn entry(out: &mut Vec<u8>, name: &str, mode: u32, data: &[u8]) {
            let name_size = name.len() + 1;
            let header = format!(
                "070701{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
                1, mode, 0, 0, 1, 0, data.len(), 0, 0, 0, 0, name_size, 0
            );
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.resize(out.len() + (4 - (110 + name_size) % 4) % 4, 0);
            out.extend_from_slice(data);
            out.resize(out.len() + (4 - data.len() % 4) % 4, 0);
        }
        let mut archive = Vec::new();
        entry(&mut archive, "bin", 0o040_755, b"");
        entry(&mut archive, "init", 0o100_755, b"#!/bin/sh\n");
        entry(&mut archive, "bin/sh", 0o120_777, b"busybox");
        entry(&mut archive, "dev/console", 0o020_600, b"");
        entry(&mut archive, "../escape", 0o100_644, b"x");
        entry(&mut archive, "TRAILER!!!", 0, b"");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&archive).unwrap();
        let entries = read_cpio(&gz.finish().unwrap()).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries["bin"], CpioEntry::Dir(0o755)));
        match &entries["init"] {
            CpioEntry::File(mode, data) => {
                assert_eq!(*mode, 0o755);
                assert_eq!(data, b"#!/bin/sh\n");
            }
            _ => panic!("init is a file"),
        }
        assert!(matches!(&entries["bin/sh"], CpioEntry::Symlink(t) if t == "busybox"));
        assert!(read_cpio(b"not gzip").is_err());
    }

    /// A machine written before networks existed still loads, with none.
    #[test]
    fn a_machine_without_a_network_still_loads() {
        let old = r#"{"machineID":"vm-a","name":"a","templateID":"base","cpuCount":1,
            "memoryMB":512,"diskGiB":1,"autostart":true,"restartPolicy":"always",
            "desired":"running","createdAt":1}"#;
        let machine: Stored = serde_json::from_str(old).unwrap();
        assert!(machine.network.is_none());
        assert!(!serde_json::to_string(&machine).unwrap().contains("network"));

        let new = old.replace(
            r#""createdAt":1"#,
            r#""createdAt":1,"network":{"allowOut":["10.0.0.0/8"]}"#,
        );
        let machine: Stored = serde_json::from_str(&new).unwrap();
        let network = machine.network.unwrap();
        assert_eq!(network.allow_out, ["10.0.0.0/8"]);
        assert!(network.deny_out.is_empty() && network.allow_internet_access.is_none());
    }

    #[test]
    fn names_and_ids() {
        assert!(valid_name("web-01"));
        assert!(!valid_name("../x"));
        assert!(!valid_name(""));
        assert_eq!(machine_id("web-01"), machine_id("web-01"));
        assert!(machine_id("web-01").starts_with("vm-"));
    }

    #[test]
    fn restarts_are_limited_within_the_window() {
        let id = "vm-crashloop";
        for _ in 0..MAX_RESTARTS {
            assert!(admit_restart(id));
        }
        assert!(!admit_restart(id));
    }
}
