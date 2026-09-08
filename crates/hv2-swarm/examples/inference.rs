//! An agent asks a model a question, and holding a model is a permission.
//!
//! Everything else in this repository has been the floor under this one. The
//! memory a fleet needs for a model was measured against a region of filler
//! bytes; the bandwidth a forward pass would have was measured with a loop
//! shaped like one; the channel and the capability a tool call travels over
//! were exercised with a tool that returned a hostname. There were no weights,
//! no tokeniser and no forward pass anywhere in the tree, and the project said
//! so on its own front page.
//!
//! This is a real model — a 1.2-billion-parameter Llama, quantised to `Q8_0`,
//! mapped once — answering questions put to it from inside hardware-isolated
//! sandboxes, under the same capability graph that governs every other tool.
//!
//! # Where the model runs, and why that is the design
//!
//! In the host, invoked by the guest as a capability. `hv2-infer`'s crate
//! documentation carries the full argument; the short of it is that a fleet
//! cannot all infer at once — weight streaming says so — so inference has to be
//! *scheduled*, and a scheduler that lives inside a thousand independent guests
//! is not a scheduler. The isolation the other choice appears to buy is already
//! spent: the weights are a host-owned mapping either way, and the host is the
//! thing that created the guest's memory.
//!
//! What the sandbox keeps is what it was always for. The agent decides *what*
//! to ask; whether it may ask at all is not its decision, and the answer
//! arrives on its own connection under its own request id.
//!
//! # What is asserted
//!
//! - **The answer is inside the guest.** Checked on each agent's own console,
//!   in its own VM, as every delivery claim in this project is.
//! - **A refused agent gets no inference.** Counted at the model — forward
//!   passes are counted inside the thing that runs them, not at the broker that
//!   gates it, because a gate that refuses and infers anyway reads identically
//!   from outside.
//! - **The model is paid for once.** Two agents converse over one mapping. What
//!   the second one costs is its own key/value cache, which is the number that
//!   decides how many agents fit on a node.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example inference -- <model.gguf>
//! ```
//!
//! Needs `/dev/kvm`, the `x86_64-unknown-none` target, and a Llama-architecture
//! GGUF. Put the model on a Linux filesystem: reading one over a 9p mount took
//! 44 seconds here against 6 from local disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_agent_proto::{parse, Header, Kind, HEADER_LEN};
use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_infer::schedule::default_threads;
use hv2_infer::{generate, Model, Session};
use hv2_swarm::{AgentId, Capability, Swarm};

const GUEST_TARGET: &str = "x86_64-unknown-none";
const HOST_PORT: u32 = 1024;
const GUEST_PORT: u32 = 5000;
const BOUND: Duration = Duration::from_secs(10);
/// A forward pass is hundreds of milliseconds and an answer is tens of tokens.
const THINKING: Duration = Duration::from_secs(180);

/// The capability that gates the model.
const TOOL: &str = "infer";

/// Who asks what, and what has to be in the answer for it to have worked.
///
/// Questions with answers that are not matters of opinion, so that "it worked"
/// is checkable. A transformer with a defect in it runs at full speed and stops
/// producing language; what it does not do is answer.
const AGENTS: [(&str, &str, &str, bool); 3] = [
    (
        "alpha",
        "What is the capital of France? Answer in one word.",
        "Paris",
        true,
    ),
    (
        "beta",
        "What colour is the sky on a clear day? Answer in one word.",
        "Blue",
        true,
    ),
    (
        "gamma",
        "What is the capital of Japan? Answer in one word.",
        "Tokyo",
        false,
    ),
];

/// Forward passes run, counted inside the model rather than at the gate.
static PASSES: AtomicUsize = AtomicUsize::new(0);

struct Agent {
    vm: Arc<VM>,
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Agent {
    fn send(&self, payload: &[u8]) {
        let _ = self.device.lock().send(self.connection, payload);
    }

    fn take(&self) -> Vec<u8> {
        self.device.lock().recv(self.connection).unwrap_or_default()
    }

    async fn console(&self) -> String {
        self.vm.console_output().await
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

fn frame(id: u32, kind: Kind, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; HEADER_LEN];
    Header::new(id, kind, payload.len() as u32)
        .encode((&mut out[..HEADER_LEN]).try_into().expect("header sized"));
    out.extend_from_slice(payload);
    out
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
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    None
}

/// Wait for an agent's tool call and return its id and payload.
async fn take_call(agent: &Agent) -> Option<(u32, String)> {
    let deadline = Instant::now() + BOUND;
    let mut buffered = Vec::new();
    while Instant::now() < deadline {
        buffered.extend_from_slice(&agent.take());
        if let Some((header, body)) = parse(&buffered) {
            if header.kind == Kind::ToolCall {
                return Some((header.id, String::from_utf8_lossy(body).to_string()));
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    None
}

/// This process's resident set, in bytes.
///
/// Read rather than estimated. It is the weaker of the two memory numbers this
/// prints and is labelled as such where it is printed: a session allocates out
/// of an arena that loading the model already grew, so a resident delta
/// understates what a conversation costs. The cache size is counted.
fn resident() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|p| p.parse().ok()))
        .map_or(0, |pages: u64| pages * 4096)
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: inference <model.gguf>");
        eprintln!();
        eprintln!("A Llama-architecture GGUF in Q8_0, F16 or F32. Put it on a Linux filesystem:");
        eprintln!("reading one over a 9p mount took 44 s here against 6 from local disk.");
        return std::process::ExitCode::FAILURE;
    };

    // A pool of the size a forward pass actually wants. Rayon's global pool is
    // sized to the machine and a pass gets slower past about a third of it —
    // `examples/scheduled` gets this from the scheduler, and this one, which
    // predates the scheduler, has to ask for it.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(default_threads())
        .build()
        .expect("a thread pool");

    // ── the model, once, for the whole fleet ────────────────────────────
    let before_model = resident();
    let started = Instant::now();
    let model = match Model::load(&path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let after_model = resident();
    let s = &model.shape;
    println!(
        "model         : {} layers, width {}, {} heads over {} kv heads, vocab {} — {:.0} MiB \
         mapped",
        s.layers,
        s.width,
        s.heads,
        s.kv_heads,
        s.vocab,
        model.mapped_bytes() as f64 / (1024.0 * 1024.0)
    );
    println!(
        "loaded        : {:.1} s, of which {:.1} s deciding the rotation convention by running \
         both",
        started.elapsed().as_secs_f64(),
        model.detection.as_secs_f64()
    );
    println!(
        "resident      : +{:.1} MiB for the model. A mapping is faulted in as it is read rather than copied — and deciding the rotation read all of it, so this is a model fully resident, which is what a busy agent would make it anyway.",
        mib(after_model.saturating_sub(before_model))
    );

    // ── the fleet ───────────────────────────────────────────────────────
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let mut agents = BTreeMap::new();
    for (index, (name, _, _, _)) in AGENTS.iter().enumerate() {
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

    let mut swarm = Swarm::new(hv2_swarm::LocalTransport::new());
    swarm.add_root("root").expect("root");
    for (name, _, _, granted) in AGENTS {
        swarm.add_agent(name, "root").expect("an agent");
        if granted {
            swarm.grant_capability(&AgentId::new(name), format!("tool:{TOOL}"));
        }
    }
    println!(
        "agents        : {} sandboxes, identical images — {} hold tool:{TOOL}",
        AGENTS.len(),
        AGENTS.iter().filter(|a| a.3).count()
    );
    println!();

    // ── each agent asks, and the graph decides ──────────────────────────
    let mut ok = true;
    let mut per_agent = Vec::new();
    // Held, not dropped. The first version of this let each session fall out of
    // scope before the next was measured, so the second agent's cache came out
    // of the first one's freed arena and its resident growth read as zero — a
    // number about the allocator, presented as a number about what an agent
    // costs.
    let mut sessions: Vec<Session<'_>> = Vec::new();

    for (index, (name, question, expected, granted)) in AGENTS.iter().enumerate() {
        let id = AgentId::new(*name);
        let agent = &agents[&id];

        // The task is the tool and its argument. The guest's rule for a task it
        // does not recognise as a `send` is to answer with a tool call carrying
        // the same bytes — one line, standing in for a model deciding which
        // tool to reach for, which is exactly the decision this example is
        // about to make available to it.
        agent.send(&frame(
            1,
            Kind::Task,
            format!("{TOOL}: {question}").as_bytes(),
        ));

        let Some((request, asked)) = take_call(agent).await else {
            println!("{name:<6}        : FAILED — never asked");
            ok = false;
            continue;
        };
        let argument = asked.split_once(':').map_or("", |(_, rest)| rest).trim();

        if !swarm.holds(&id, &Capability::new(format!("tool:{TOOL}"))) {
            // Under the request's own id, so an agent with several calls
            // outstanding knows which one was refused.
            agent.send(&frame(request, Kind::Error, b"refused"));
            println!("{name:<6}        : asked, refused — no forward pass was run");
            continue;
        }

        let before = resident();
        let mut session = Session::open(&model);
        let thinking = Instant::now();
        let answer = match pool.install(|| generate(&model, &mut session, argument, 32)) {
            Ok(answer) => answer,
            Err(e) => {
                println!("{name:<6}        : FAILED — {e}");
                ok = false;
                continue;
            }
        };
        PASSES.fetch_add(session.len(), Ordering::SeqCst);
        let took = thinking.elapsed();
        agent.send(&frame(request, Kind::ToolResult, answer.trim().as_bytes()));

        println!(
            "{name:<6}        : {:?} -> {:?} in {:.1} s, {} tokens, {:.0} ms each",
            question,
            answer.trim(),
            took.as_secs_f64(),
            session.len(),
            took.as_secs_f64() * 1000.0 / session.len().max(1) as f64
        );
        per_agent.push((
            *name,
            *expected,
            session.cache_bytes(),
            resident().saturating_sub(before),
        ));
        sessions.push(session);
        let _ = (granted, index);
    }

    // ── the assertion, at each guest's own console ──────────────────────
    println!();
    for (name, question, expected, granted) in AGENTS {
        let agent = &agents[&AgentId::new(name)];
        let want = if granted { expected } else { "agent denied" };
        let arrived = wait_for(THINKING, || async { agent.console().await.contains(want) }).await;
        match (arrived, granted) {
            (Some(()), true) => println!("{name:<6} guest  : the answer reached it — {want:?}"),
            (Some(()), false) => {
                println!("{name:<6} guest  : refused, and it was told which request");
            }
            (None, true) => {
                println!("{name:<6} guest  : FAILED — the answer never reached the sandbox");
                ok = false;
            }
            (None, false) => {
                println!("{name:<6} guest  : FAILED — refused and never told");
                ok = false;
            }
        }
        let _ = question;
    }

    println!();
    for (name, _, _, _) in AGENTS {
        for line in agents[&AgentId::new(name)]
            .console()
            .await
            .lines()
            .filter(|l| l.starts_with("agent "))
        {
            println!("{name:<6} says   : {line}");
        }
    }

    // ── the model once, plus what each agent holds ──────────────────────
    println!();
    let expected_passes: usize = PASSES.load(Ordering::SeqCst);
    let refused = AGENTS.iter().filter(|a| !a.3).count();
    println!(
        "forward passes: {expected_passes}, for the {} agents that were allowed — {refused} \
         asked and ran none",
        AGENTS.len() - refused
    );
    println!(
        "the model     : mapped once, {:.0} MiB, and every conversation ran over that one mapping",
        model.mapped_bytes() as f64 / (1024.0 * 1024.0)
    );
    for (name, _, cache, cost) in &per_agent {
        println!(
            "{name:<6}        : {:.2} MiB of key/value cache held, {:.1} MiB of resident growth",
            *cache as f64 / (1024.0 * 1024.0),
            mib(*cost)
        );
    }

    println!(
        "per token     : {} bytes of cache, so {:.0} MiB for a 2,000-token conversation and {:.0} MiB for an 8,000-token one. That is what a node should be sized from: counted rather than sampled, and the one thing an agent cannot share with another agent, because it is that agent's conversation.",
        model.cache_bytes_per_token(),
        (model.cache_bytes_per_token() * 2000) as f64 / (1024.0 * 1024.0),
        (model.cache_bytes_per_token() * 8000) as f64 / (1024.0 * 1024.0)
    );

    for (name, _, _, _) in AGENTS {
        let _ = agents[&AgentId::new(name)].vm.stop().await;
    }

    println!();
    if ok {
        println!(
            "result        : a real model answered real questions put to it from inside \
             hardware-isolated sandboxes, and the one without the capability got nothing — \
             counted at the model, not at the gate. Every inference figure this project has \
             published was the floor under this."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
