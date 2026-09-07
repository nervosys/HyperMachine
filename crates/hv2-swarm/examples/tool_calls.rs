//! An agent asks for a tool. Whether it gets one is not its decision.
//!
//! A sandbox that can only talk to other sandboxes is not much of an agent.
//! What makes it one is tools — reading a file, calling an API, spending money
//! — and what makes it *safe* is that holding a tool is a permission rather
//! than a capability of the code. An agent that can decide which tool to call
//! can also decide to call one it should not have, and the whole point of
//! putting it in a hardware-isolated sandbox is that deciding is not the same
//! as doing.
//!
//! # What this runs
//!
//! Two agents, identical in every respect except one grant. Each is given the
//! same task over vsock; each answers with the same tool call; one holds the
//! capability and one does not.
//!
//! The assertion is not that the second is refused. It is that **the tool does
//! not run** — counted at the tool itself, not at the broker that gates it.
//! A gate that refuses and then calls anyway is indistinguishable from a
//! working one at the agent, and distinguishable everywhere it matters.
//!
//! # What is honest about the agent here
//!
//! Nothing in the guest decides anything. A task beginning `do:` is answered
//! with `tool:` and the rest, by one rule, and that stands in for a model
//! choosing a tool. What is under test is the path a decision travels and the
//! permission that governs it, and neither cares how the decision was reached
//! — which is exactly why this can be measured before there is a model to
//! decide.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example tool_calls
//! ```
//!
//! Needs `/dev/kvm` and the `i686-unknown-linux-musl` target.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{AgentId, Capability, Swarm};

const GUEST_TARGET: &str = "i686-unknown-linux-musl";
const HOST_PORT: u32 = 1024;
const GUEST_PORT: u32 = 5000;

/// The tool on offer, and the capability that gates it.
const TOOL: &str = "hostname";

/// How long to wait before concluding a tool call did not arrive.
const BOUND: Duration = Duration::from_secs(10);

/// The tool itself, and a count of how many times it has actually run.
///
/// The count is the whole assertion. Everything else in this example could be
/// working perfectly while the gate leaks, and the only place that shows is
/// here.
static TOOL_RUNS: AtomicUsize = AtomicUsize::new(0);

fn run_tool() -> String {
    TOOL_RUNS.fetch_add(1, Ordering::SeqCst);
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// One agent: its VM, its vsock device, and the connection into it.
struct Agent {
    vm: Arc<VM>,
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Agent {
    fn send(&self, payload: &[u8]) {
        let _ = self.device.lock().send(self.connection, payload);
    }

    fn said(&self) -> Vec<u8> {
        self.device.lock().peek(self.connection).unwrap_or_default()
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

/// What a tool call amounted to.
enum Outcome {
    /// The agent asked, held the capability, and the tool ran.
    Ran(String),
    /// The agent asked and did not hold the capability. The tool did not run.
    Refused,
    /// The agent never asked.
    Silent,
}

/// Take one tool call from an agent and decide it.
///
/// The capability is checked here, once, at the only point a request crosses
/// from a guest into the host — the same shape as `Swarm::send`, and for the
/// same reason: a check a caller can forget is advice.
async fn broker(swarm: &Swarm<hv2_swarm::LocalTransport>, id: &AgentId, agent: &Agent) -> Outcome {
    let asked = wait_for(BOUND, || async { agent.said().starts_with(b"tool:") }).await;
    if asked.is_none() {
        return Outcome::Silent;
    }

    let request = agent.said();
    let name = String::from_utf8_lossy(&request[b"tool:".len()..])
        .trim()
        .to_string();

    let capability = Capability::new(format!("tool:{name}"));
    if !swarm.holds(id, &capability) {
        agent.send(b"refused");
        return Outcome::Refused;
    }

    let answer = run_tool();
    agent.send(answer.as_bytes());
    Outcome::Ran(answer)
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

    let names = ["trusted", "untrusted"];
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
    println!(
        "agents        : {} unikernels, identical images",
        agents.len()
    );

    // The graph, and the single grant that separates the two.
    let mut swarm = Swarm::new(hv2_swarm::LocalTransport::new());
    swarm.add_root("root").expect("root");
    swarm.add_agent("trusted", "root").expect("trusted");
    swarm.add_agent("untrusted", "root").expect("untrusted");
    swarm.grant_capability(&AgentId::new("trusted"), format!("tool:{TOOL}"));
    println!("capability    : 'tool:{TOOL}' granted to trusted, withheld from untrusted");
    println!();

    let mut ok = true;
    let mut outcomes = Vec::new();
    for name in names {
        let id = AgentId::new(name);
        let agent = &agents[&id];
        agent.send(format!("do:{TOOL}").as_bytes());
        let outcome = broker(&swarm, &id, agent).await;
        match &outcome {
            Outcome::Ran(answer) => {
                println!("{name:<10}    : asked, allowed, tool returned {answer:?}");
            }
            Outcome::Refused => println!("{name:<10}    : asked, refused, tool not called"),
            Outcome::Silent => {
                println!("{name:<10}    : never asked");
                ok = false;
            }
        }
        outcomes.push(outcome);
    }

    // The assertion that matters, counted at the tool rather than at the gate.
    let runs = TOOL_RUNS.load(Ordering::SeqCst);
    println!();
    println!(
        "tool ran      : {runs} time(s), for {} agents that asked",
        names.len()
    );

    let trusted_ran = matches!(outcomes.first(), Some(Outcome::Ran(_)));
    let untrusted_refused = matches!(outcomes.get(1), Some(Outcome::Refused));

    if !trusted_ran {
        println!("result        : the agent holding the capability did not get its tool.");
        ok = false;
    }
    if !untrusted_refused {
        println!("result        : the agent without the capability was not refused.");
        ok = false;
    }
    if runs != 1 {
        println!(
            "result        : the tool ran {runs} times and should have run once — a gate that \
             refuses and calls anyway reads exactly like one that works."
        );
        ok = false;
    }

    println!();
    for name in names {
        for line in agents[&AgentId::new(name)]
            .vm
            .console_output()
            .await
            .lines()
        {
            println!("{name:<10} says: {line}");
        }
    }

    for name in names {
        let _ = agents[&AgentId::new(name)].vm.stop().await;
    }

    if ok {
        println!();
        println!(
            "result        : two identical sandboxes, one grant between them. Both asked for \
             the same tool; it ran once. Deciding to call a tool and being able to are \
             different things, which is what the sandbox is for."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
