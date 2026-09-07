//! One agent addresses another, and the graph decides whether it arrives.
//!
//! Every message in this project has so far travelled *into* a guest. The host
//! decided to send it, the graph was consulted on the host's behalf, and the
//! guest could only answer on its own connection. That is enough to prove a
//! transport and not enough to be a swarm: an agent that cannot address another
//! agent is not interacting with anything, it is being talked to.
//!
//! This is the other direction. A guest names a recipient; the graph is
//! consulted; the message arrives or it does not.
//!
//! # What is asserted, and where
//!
//! At the recipient, as always. A refusal that the sender is told about proves
//! the sender was told. What matters is that the recipient's guest never saw
//! the message — checked on its own console, inside its own VM, which is the
//! only place the claim is worth anything.
//!
//! The same message is then sent again after the edge is granted, so a silent
//! recipient cannot be mistaken for a broken transport. Without that second
//! half, a swarm whose delivery was simply broken would score exactly the same
//! as one that was enforcing.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example agent_messages
//! ```
//!
//! Needs `/dev/kvm` and the `i686-unknown-linux-musl` target.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{AgentId, Denied, Message, Swarm, Transport};

const GUEST_TARGET: &str = "i686-unknown-linux-musl";
const HOST_PORT: u32 = 1024;
const GUEST_PORT: u32 = 5000;
const BOUND: Duration = Duration::from_secs(10);

/// How long to wait before concluding a message did *not* arrive.
const ABSENCE_WINDOW: Duration = Duration::from_millis(500);

struct Agent {
    vm: Arc<VM>,
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Agent {
    fn send(&self, payload: &[u8]) {
        let _ = self.device.lock().send(self.connection, payload);
    }

    /// What the guest has said and not yet been read.
    fn said(&self) -> Vec<u8> {
        self.device.lock().peek(self.connection).unwrap_or_default()
    }

    /// The same, consumed.
    ///
    /// `peek` leaves the bytes in the connection's buffer, so a second request
    /// read with `peek` arrives concatenated to the first — which is how the
    /// first version of this example appeared to deliver a message the graph
    /// had refused. It had not: the refused text was still sitting in the
    /// buffer and rode along inside the *next* payload, which was permitted.
    ///
    /// A demonstration that has to be right about a refusal cannot read its
    /// evidence with a function that keeps the evidence around.
    fn take(&self) -> Vec<u8> {
        self.device.lock().recv(self.connection).unwrap_or_default()
    }
}

/// Delivery into a guest, over that guest's own vsock connection.
struct VsockTransport {
    agents: Arc<BTreeMap<AgentId, Agent>>,
}

impl Transport for VsockTransport {
    fn deliver(&mut self, message: Message) {
        if let Some(agent) = self.agents.get(&message.to) {
            agent.send(&message.payload);
        }
    }
}

fn build_guest() -> Result<PathBuf, String> {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("hv2-unikernel");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "building the guest failed:\n{}\n\nThe target may be missing:  rustup target add \
             {GUEST_TARGET}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let elf = crate_dir
        .join("target")
        .join(GUEST_TARGET)
        .join("release")
        .join("hv2-unikernel");
    let dir = std::env::temp_dir().join("hv2-unikernel");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

async fn start_agent(name: &str, elf: &Path, cid: u64) -> Result<Agent, String> {
    let config = VMConfig {
        name: name.to_string(),
        vcpu_count: 1,
        memory_size: 64 * 1024 * 1024,
        boot: Some(BootSource::multiboot(elf)),
        ..Default::default()
    };
    let vm = Arc::new(VM::new(config).map_err(|e| format!("{name}: no backend — {e}"))?);
    vm.provision()
        .await
        .map_err(|e| format!("{name}: provision — {e}"))?;
    let device = vm
        .attach_vsock(cid)
        .await
        .map_err(|e| format!("{name}: attach_vsock — {e}"))?;
    vm.launch()
        .await
        .map_err(|e| format!("{name}: launch — {e}"))?;

    wait_for(BOUND, || async {
        vm.console_output().await.contains("vsock cid")
    })
    .await
    .ok_or_else(|| format!("{name}: the guest never reported a vsock device"))?;

    let connection = device
        .lock()
        .connect(HOST_PORT, GUEST_PORT)
        .map_err(|e| format!("{name}: connect — {e}"))?;
    wait_for(BOUND, || async {
        device.lock().state(connection) == Some(VsockConnectionState::Established)
    })
    .await
    .ok_or_else(|| format!("{name}: the guest never answered the connection request"))?;

    Ok(Agent {
        vm,
        device,
        connection,
    })
}

async fn wait_for<F, Fut>(bound: Duration, mut check: F) -> Option<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + bound;
    while Instant::now() < deadline {
        if check().await {
            return Some(());
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    None
}

/// Wait for an agent to ask to send something, and return what it asked.
///
/// The guest answers a `send:` task with `to:<recipient>:<text>`. Splitting it
/// here rather than in the guest keeps the guest's half to one rule.
async fn take_request(agent: &Agent) -> Option<(String, Vec<u8>)> {
    wait_for(BOUND, || async { agent.said().starts_with(b"to:") }).await?;
    // Consumed, not peeked: see `Agent::take`.
    let said = agent.take();
    let rest = &said[b"to:".len()..];
    let colon = rest.iter().position(|b| *b == b':')?;
    let to = String::from_utf8_lossy(&rest[..colon]).to_string();
    Some((to, rest[colon + 1..].to_vec()))
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let names = ["root", "a", "b"];
    let mut agents = BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        match start_agent(name, &elf, 3 + index as u64).await {
            Ok(agent) => {
                agents.insert(AgentId::new(*name), agent);
            }
            Err(e) => {
                eprintln!("agent         : FAILED — {e}");
                eprintln!("This needs /dev/kvm.");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let agents = Arc::new(agents);
    println!(
        "agents        : {} unikernels — root, and siblings a and b",
        agents.len()
    );

    let mut swarm = Swarm::new(VsockTransport {
        agents: Arc::clone(&agents),
    });
    swarm.add_root("root").expect("root");
    swarm.add_agent("a", "root").expect("a");
    swarm.add_agent("b", "root").expect("b");

    let a = &agents[&AgentId::new("a")];
    let b = &agents[&AgentId::new("b")];
    let mut ok = true;

    // ── 1. a addresses b, with no edge between them ─────────────────────
    println!();
    a.send(b"send:b:the first message");
    let Some((to, payload)) = take_request(a).await else {
        println!("refused       : FAILED — a never asked to send anything");
        return std::process::ExitCode::FAILURE;
    };
    println!("a asks        : send to {to:?}, {} bytes", payload.len());

    match swarm.send("a", to.as_str(), payload.clone()) {
        Err(Denied::NoGrant { .. }) => {
            tokio::time::sleep(ABSENCE_WINDOW).await;
            let console = b.vm.console_output().await;
            if console.contains("the first message") {
                println!("refused       : FAILED — the graph refused and b received it anyway");
                ok = false;
            } else {
                println!("refused       : ok — siblings, no grant, and b's guest never saw it");
            }
        }
        Err(e) => {
            println!("refused       : FAILED — refused for the wrong reason: {e}");
            ok = false;
        }
        Ok(_) => {
            println!("refused       : FAILED — the graph admitted a message between siblings");
            ok = false;
        }
    }

    // ── 2. the same message, once the edge exists ───────────────────────
    swarm.grant("a", "b");
    a.send(b"send:b:the second message");
    let Some((to, payload)) = take_request(a).await else {
        println!("granted       : FAILED — a never asked again");
        return std::process::ExitCode::FAILURE;
    };

    match swarm.send("a", to.as_str(), payload) {
        Ok(relation) => {
            let arrived = wait_for(BOUND, || async {
                b.vm.console_output().await.contains("the second message")
            })
            .await;
            match arrived {
                Some(()) => println!("granted       : ok — b's guest received it ({relation})"),
                None => {
                    println!("granted       : FAILED — granted, and b's guest never saw it");
                    ok = false;
                }
            }
        }
        Err(e) => {
            println!("granted       : FAILED — refused after the grant: {e}");
            ok = false;
        }
    }

    println!();
    for name in names {
        for line in agents[&AgentId::new(name)]
            .vm
            .console_output()
            .await
            .lines()
        {
            println!("{name:<5} says   : {line}");
        }
    }

    for name in names {
        let _ = agents[&AgentId::new(name)].vm.stop().await;
    }

    println!();
    if ok {
        println!(
            "result        : an agent addressed another agent from inside its own sandbox, and \
             the graph decided. The refusal was checked where it counts — the recipient's \
             console, in the recipient's VM."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
