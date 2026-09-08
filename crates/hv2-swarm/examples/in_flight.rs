//! An agent holding three requests at once, answered out of order.
//!
//! The protocol has carried a request id since it had a shape at all, and
//! nothing had ever used two of them. Every example sent one frame and waited
//! for its answer, which is precisely the arrangement the id was added to
//! escape: if only one request can be outstanding, the reply needs no id
//! because it can only be answering the one thing.
//!
//! So this is the id earning itself. One agent issues three tool calls from a
//! single task and goes straight back to reading; a peer reaches it while all
//! three are outstanding; and the host then answers them in the reverse of the
//! order they were asked, refusing the middle one.
//!
//! # What is asserted, and why each half is needed
//!
//! **That the requests were outstanding together.** Counted at the host: three
//! `ToolCall` frames are read before any answer is written. A guest that
//! blocked for each reply could not have produced the second frame, so this is
//! the claim that the agent is not a request/response loop wearing a header.
//!
//! **That a peer got through while it was busy.** The `Deliver` is admitted by
//! the graph and lands on the worker's console *before* any of its own answers
//! do. An agent that cannot be reached while it is waiting on a tool is an
//! agent that can be silenced by a slow tool.
//!
//! **That each answer settled its own request.** This is what the id is for
//! and it is the half that would otherwise pass by luck. The answers come back
//! last-asked-first, and each names the request it settles: had the guest
//! paired them by arrival order — which is what every previous version of this
//! guest effectively did — the console would attribute `gamma`'s result to
//! `alpha`, and the refusal to the wrong tool entirely.
//!
//! **That a refused call is refused at the tool.** `beta` is withheld, and the
//! count is read from inside the tool rather than from the broker that gates
//! it, as everywhere else in this repository.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example in_flight
//! ```
//!
//! Needs `/dev/kvm` and the `x86_64-unknown-none` target.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_agent_proto::{parse, Header, Kind, HEADER_LEN};
use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{AgentId, Capability, Message, Swarm, Transport};

const GUEST_TARGET: &str = "x86_64-unknown-none";
const HOST_PORT: u32 = 1024;
const GUEST_PORT: u32 = 5000;
const BOUND: Duration = Duration::from_secs(10);

/// The tools on offer, and what each returns.
///
/// Distinct answers, deliberately. A tool that returned the same thing as its
/// neighbour would let a guest mismatch every reply and still print something
/// that looked right.
const TOOLS: [(&str, &str); 3] = [("alpha", "A"), ("beta", "B"), ("gamma", "G")];

/// Which of them the worker is allowed to call. `beta` is withheld.
const GRANTED: [&str; 2] = ["alpha", "gamma"];

/// How many times each tool has actually run, counted inside the tool.
static RUNS: [AtomicUsize; 3] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

/// Call a tool, for real.
fn run_tool(index: usize) -> &'static str {
    RUNS[index].fetch_add(1, Ordering::SeqCst);
    TOOLS[index].1
}

struct Agent {
    vm: Arc<VM>,
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Agent {
    fn send(&self, payload: &[u8]) {
        let _ = self.device.lock().send(self.connection, payload);
    }

    /// Everything the guest has said and not yet been read, consumed.
    fn take(&self) -> Vec<u8> {
        self.device.lock().recv(self.connection).unwrap_or_default()
    }

    async fn console(&self) -> String {
        self.vm.console_output().await
    }
}

/// The host's side of the stream, which has the same problem the guest's does.
///
/// Three frames written back to back arrive as one run of bytes. Reading them
/// needs a buffer that outlives a single read, for exactly the reason the guest
/// needs one — a reader that takes what is there and parses the first frame
/// throws the rest away.
#[derive(Default)]
struct Inbox {
    bytes: Vec<u8>,
}

impl Inbox {
    /// Take whatever the guest has said, and add it to what is already held.
    fn fill(&mut self, agent: &Agent) {
        self.bytes.extend_from_slice(&agent.take());
    }

    /// The next whole frame, if one is here.
    fn next(&mut self) -> Option<(Header, Vec<u8>)> {
        let (header, body) = parse(&self.bytes).map(|(h, b)| (h, b.to_vec()))?;
        self.bytes.drain(..HEADER_LEN + header.len as usize);
        Some((header, body))
    }
}

/// Delivery into a guest, over that guest's own vsock connection.
struct VsockTransport {
    agents: Arc<BTreeMap<AgentId, Agent>>,
    next_id: u32,
}

impl Transport for VsockTransport {
    fn deliver(&mut self, message: Message) {
        if let Some(agent) = self.agents.get(&message.to) {
            agent.send(&frame(self.next_id, Kind::Deliver, &message.payload));
            self.next_id += 1;
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

/// Build one frame.
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
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    None
}

/// Collect `want` tool calls from an agent without answering any of them.
///
/// Nothing is written back inside this loop, which is the point: if the guest
/// needed an answer to proceed, the second call would never arrive and this
/// would time out rather than quietly measure something weaker.
async fn gather_calls(agent: &Agent, inbox: &mut Inbox, want: usize) -> Vec<(u32, String)> {
    let mut calls = Vec::new();
    let deadline = Instant::now() + BOUND;
    while calls.len() < want && Instant::now() < deadline {
        inbox.fill(agent);
        while let Some((header, body)) = inbox.next() {
            if header.kind == Kind::ToolCall {
                calls.push((header.id, String::from_utf8_lossy(&body).trim().to_string()));
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    calls
}

/// Wait for an agent to ask to send something, and return what it asked.
async fn take_send_request(agent: &Agent, inbox: &mut Inbox) -> Option<(String, Vec<u8>)> {
    let deadline = Instant::now() + BOUND;
    while Instant::now() < deadline {
        inbox.fill(agent);
        while let Some((header, body)) = inbox.next() {
            if header.kind == Kind::Send {
                let colon = body.iter().position(|b| *b == b':')?;
                let to = String::from_utf8_lossy(&body[..colon]).to_string();
                return Some((to, body[colon + 1..].to_vec()));
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    None
}

/// Where a line beginning with `needle` first appears in the console.
fn line_at(console: &str, needle: &str) -> Option<usize> {
    console
        .lines()
        .position(|line| line.trim_start().starts_with(needle))
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

    let names = ["worker", "peer"];
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
    println!("agents        : worker and peer, siblings under root");

    let mut swarm = Swarm::new(VsockTransport {
        agents: Arc::clone(&agents),
        next_id: 1,
    });
    swarm.add_root("root").expect("root");
    swarm.add_agent("worker", "root").expect("worker");
    swarm.add_agent("peer", "root").expect("peer");
    for tool in GRANTED {
        swarm.grant_capability(&AgentId::new("worker"), format!("tool:{tool}"));
    }
    println!(
        "capabilities  : worker holds tool:{}, and not tool:{}",
        GRANTED.join(", tool:"),
        TOOLS[1].0
    );
    println!();

    let worker = &agents[&AgentId::new("worker")];
    let peer = &agents[&AgentId::new("peer")];
    let mut worker_inbox = Inbox::default();
    let mut peer_inbox = Inbox::default();
    let mut ok = true;

    // ── 1. one task, three requests, none of them answered ──────────────
    let task: String = format!("tools {} {} {}", TOOLS[0].0, TOOLS[1].0, TOOLS[2].0);
    let started = Instant::now();
    worker.send(&frame(1, Kind::Task, task.as_bytes()));

    let calls = gather_calls(worker, &mut worker_inbox, TOOLS.len()).await;
    if calls.len() != TOOLS.len() {
        println!(
            "in flight     : FAILED — {} of {} tool calls arrived before any was answered",
            calls.len(),
            TOOLS.len()
        );
        for name in names {
            for line in agents[&AgentId::new(name)].console().await.lines() {
                println!("{name:<6} says  : {line}");
            }
        }
        for name in names {
            let _ = agents[&AgentId::new(name)].vm.stop().await;
        }
        return std::process::ExitCode::FAILURE;
    }
    println!(
        "in flight     : {} tool calls outstanding at once — {}",
        calls.len(),
        calls
            .iter()
            .map(|(id, name)| format!("#{id} {name}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let holds = worker
        .console()
        .await
        .contains(&format!("agent holds 0x{:08X}", TOOLS.len()));
    if holds {
        println!("the guest says: it is holding {} of them", TOOLS.len());
    } else {
        println!("the guest says: FAILED — it did not report holding all three");
        ok = false;
    }

    // ── 2. a peer reaches the worker while all three are outstanding ────
    peer.send(&frame(
        1,
        Kind::Task,
        b"send worker:a message while you are busy",
    ));
    let Some((to, payload)) = take_send_request(peer, &mut peer_inbox).await else {
        println!("interleaved   : FAILED — peer never asked to send anything");
        return std::process::ExitCode::FAILURE;
    };
    swarm.grant("peer", "worker");
    match swarm.send("peer", to.as_str(), payload) {
        Ok(_) => {
            let arrived = wait_for(BOUND, || async {
                worker.console().await.contains("agent recv")
            })
            .await;
            match arrived {
                Some(()) => println!(
                    "interleaved   : peer's message reached the worker with {} calls still \
                     outstanding",
                    calls.len()
                ),
                None => {
                    println!("interleaved   : FAILED — granted, and the worker never saw it");
                    ok = false;
                }
            }
        }
        Err(e) => {
            println!("interleaved   : FAILED — refused after the grant: {e}");
            ok = false;
        }
    }

    // ── 3. answer them backwards, refusing the middle one ───────────────
    //
    // Reverse order on purpose. Answering in the order asked is the one order
    // a guest that ignores ids also gets right.
    let mut answered = Vec::new();
    for (id, name) in calls.iter().rev() {
        let index = TOOLS
            .iter()
            .position(|(tool, _)| tool == name)
            .expect("the guest asked for a tool this example offers");
        let capability = Capability::new(format!("tool:{name}"));
        if swarm.holds(&AgentId::new("worker"), &capability) {
            let answer = run_tool(index);
            worker.send(&frame(*id, Kind::ToolResult, answer.as_bytes()));
            answered.push(format!("agent done #0x{id:08X} \"{name}\" = \"{answer}\""));
        } else {
            // Under the request's own id, which is the only thing that makes a
            // refusal usable to an agent with several calls in the air.
            worker.send(&frame(*id, Kind::Error, b"refused"));
            answered.push(format!(
                "agent denied #0x{id:08X} \"refused\" was \"{name}\""
            ));
        }
    }
    println!(
        "answered      : {} — the reverse of the order they were asked",
        calls
            .iter()
            .rev()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", then ")
    );

    let settled = wait_for(BOUND, || async {
        let console = worker.console().await;
        answered.iter().all(|line| console.contains(line.as_str()))
    })
    .await;
    let elapsed = started.elapsed();

    let console = worker.console().await;
    println!();
    for name in names {
        for line in agents[&AgentId::new(name)].console().await.lines() {
            println!("{name:<6} says  : {line}");
        }
    }
    println!();

    // ── 4. what the console has to show ─────────────────────────────────
    if settled.is_none() {
        println!("correlation   : FAILED — an answer never reached the request it was for:");
        for line in &answered {
            if !console.contains(line.as_str()) {
                println!("                missing  {line}");
            }
        }
        ok = false;
    } else {
        println!(
            "correlation   : every answer settled its own request. Matched by arrival order \
             instead, the first line would have read {:?}.",
            format!("#0x{:08X} \"{}\"", calls[TOOLS.len() - 1].0, calls[0].1)
        );
    }

    // The answers must appear in the order they were sent, not the order they
    // were asked — otherwise nothing here has been shown out of order at all.
    let positions: Vec<Option<usize>> = answered.iter().map(|l| line_at(&console, l)).collect();
    let in_answer_order = positions.windows(2).all(|w| match (w[0], w[1]) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    });
    if in_answer_order {
        println!("order         : the console records them last-asked-first");
    } else {
        println!("order         : FAILED — the answers are not in the order they were sent");
        ok = false;
    }

    // And the peer's message must land before any of them.
    match (line_at(&console, "agent recv"), positions[0]) {
        (Some(recv), Some(first)) if recv < first => {
            println!("busy          : the peer's message landed before any answer did");
        }
        _ => {
            println!(
                "busy          : FAILED — the peer's message did not arrive while the worker \
                 was waiting"
            );
            ok = false;
        }
    }

    if console.contains("agent stray") {
        println!("stray         : FAILED — the guest was given an id it had not asked under");
        ok = false;
    }

    // ── 5. counted at the tool ──────────────────────────────────────────
    println!();
    for (index, (name, _)) in TOOLS.iter().enumerate() {
        let runs = RUNS[index].load(Ordering::SeqCst);
        let expected = usize::from(GRANTED.contains(name));
        println!(
            "tool {name:<6}   : ran {runs} time(s), asked for once, {}",
            if GRANTED.contains(name) {
                "granted"
            } else {
                "withheld"
            }
        );
        if runs != expected {
            println!("                FAILED — it should have run {expected} time(s)");
            ok = false;
        }
    }
    println!(
        "elapsed       : {:.1} ms from the task to the last answer",
        elapsed.as_secs_f64() * 1000.0
    );

    for name in names {
        let _ = agents[&AgentId::new(name)].vm.stop().await;
    }

    println!();
    if ok {
        println!(
            "result        : one agent, three requests in the air at once, a peer's message \
             delivered between them, and every answer matched to the request that asked for it \
             — including the one that was refused."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
