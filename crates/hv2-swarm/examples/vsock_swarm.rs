//! A swarm whose messages reach agents over vsock, not a serial port.
//!
//! `guest_transport` carries a message across the host/guest boundary by
//! writing bytes into COM1 and watching the guest echo them. That is real
//! delivery, and it is a serial port: one byte at a time, no addressing, no
//! connection, no way for a second agent on the same guest to have its own
//! channel. vsock is the transport an agent should have, and until now it was
//! out of reach — it needs protected mode and virtqueues, and every guest here
//! was sixteen-bit hand-assembled code.
//!
//! Now there is a `no_std` Rust unikernel with a virtio-vsock driver, so this
//! runs the command graph over the transport it was always meant to have.
//!
//! # What it checks, and where
//!
//! Every claim is checked at the *recipient*, because a transport that
//! reported its own success would be asserting the thing under test:
//!
//! 1. A command from the root reaches a worker — proven by the worker's guest
//!    echoing the payload back over its own vsock connection.
//! 2. A sideways message between workers is refused *and does not arrive* —
//!    proven by the second worker's guest having echoed nothing and printed
//!    nothing.
//! 3. Granting the sideways edge makes the same message arrive, so the refusal
//!    was the graph's decision and not a broken connection.
//!
//! Check 2 is the one that matters. A rule that returns "denied" while the
//! message still arrives passes a verdict test and fails the swarm.
//!
//! # Cost
//!
//! An idle agent halts, so it costs a parked thread and no CPU. That was not
//! true while the guest polled its virtqueue, and it is why this example used
//! to run three agents and `unikernel_swarm` a thousand — the thousand were
//! guests that halted immediately and never spoke, which measures the
//! hypervisor and not the swarm.
//!
//! `--agents N` runs the same three checks and then holds the whole swarm idle
//! to measure what it costs. Every agent is a full VM with its own vsock device
//! and its own connection.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example vsock_swarm -- --agents 1000
//! ```
//!
//! Needs `/dev/kvm` and the `i686-unknown-linux-musl` target.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_agent_proto::{Header, Kind, HEADER_LEN};
use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{AgentId, Denied, Message, Swarm, Transport};

/// The 32-bit x86 target stable Rust ships a prebuilt `core` for.
const GUEST_TARGET: &str = "i686-unknown-linux-musl";

/// The host port every agent's connection uses. Each agent has its own device,
/// so the same number on different guests is a different socket.
const HOST_PORT: u32 = 1024;
/// The guest port. Nothing in the guest binds it; the driver answers whatever
/// it is asked on.
const GUEST_PORT: u32 = 5000;

/// One agent: its VM, its vsock device, and the connection into it.
struct Agent {
    vm: Arc<VM>,
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Agent {
    /// What this agent's guest has echoed back so far.
    fn echoed(&self) -> Vec<u8> {
        self.device.lock().peek(self.connection).unwrap_or_default()
    }
}

/// Delivery into a guest, over that guest's own vsock connection.
///
/// Holds the outbound side only. Whether a guest answered is read from its
/// device afterwards, never from here.
struct VsockTransport {
    agents: Arc<BTreeMap<AgentId, Agent>>,
    /// Ids for the frames this transport sends. Monotonic, so a console line
    /// can be matched to the delivery that produced it.
    next_id: u32,
    /// Deliveries that could not reach a guest, kept rather than discarded so
    /// a run can report them instead of appearing to have worked.
    undeliverable: Vec<(AgentId, String)>,
}

impl Transport for VsockTransport {
    fn deliver(&mut self, message: Message) {
        let Some(agent) = self.agents.get(&message.to) else {
            self.undeliverable
                .push((message.to, "no VM for this agent".to_string()));
            return;
        };
        // Synchronous, and so is `VsockDevice::send`: the packet is queued and
        // the guest woken before this returns. A delivery that completed later
        // would make "the message arrived" and "the message was accepted" two
        // different moments, and every assertion below a race.
        // Framed, and as a `Deliver`: the guest is being told another agent
        // reached it, which is a different thing from being given a task.
        let payload = frame(self.next_id, Kind::Deliver, &message.payload);
        self.next_id += 1;
        if let Err(e) = agent.device.lock().send(agent.connection, &payload) {
            self.undeliverable.push((message.to, e.to_string()));
        }
    }
}

/// Build one frame.
fn frame(id: u32, kind: Kind, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; HEADER_LEN];
    Header::new(id, kind, payload.len() as u32)
        .encode((&mut out[..HEADER_LEN]).try_into().expect("header sized"));
    out.extend_from_slice(payload);
    out
}

/// Whether an agent's guest has printed `payload` on its own console.
///
/// Checked at the recipient, in the recipient's VM, which is the only place a
/// claim about delivery is worth anything. The negative case has always been
/// checked this way; both positive ones are now, so all three agree about what
/// counts as arriving.
async fn saw(agent: &Agent, payload: &[u8]) -> bool {
    agent
        .vm
        .console_output()
        .await
        .contains(&String::from_utf8_lossy(payload).to_string())
}

/// Build the guest crate and stage its ELF on local storage.
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

    // Boot from a copy on local storage: `provision` reads the image, and over
    // a 9p mount that read costs more than the whole boot.
    let dir = std::env::temp_dir().join("hv2-unikernel");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let local = dir.join("hv2-unikernel.elf");
    std::fs::copy(&elf, &local).map_err(|e| format!("could not stage the guest image: {e}"))?;
    Ok(local)
}

/// Boot one agent and open its connection.
///
/// `cid` must be unique per guest: it is the address the host reaches it at.
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

    // Before launch: a guest that probes an empty window reports "no virtio
    // device" and carries on, which is a different failure from one that never
    // ran, and the difference would be invisible here.
    let device = vm
        .attach_vsock(cid)
        .await
        .map_err(|e| format!("{name}: attach_vsock — {e}"))?;

    vm.launch()
        .await
        .map_err(|e| format!("{name}: launch — {e}"))?;

    // Wait for the guest's driver to come up, then connect.
    wait_for(Duration::from_secs(10), || async {
        vm.console_output().await.contains("vsock cid")
    })
    .await
    .ok_or_else(|| format!("{name}: the guest never reported a vsock device"))?;

    let connection = device
        .lock()
        .connect(HOST_PORT, GUEST_PORT)
        .map_err(|e| format!("{name}: connect — {e}"))?;

    wait_for(Duration::from_secs(10), || async {
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

/// Poll until `check` is true, or give up.
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
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    None
}

/// This process's CPU time so far, in seconds.
///
/// Read from `/proc/self/stat` rather than measured with a timer, because the
/// claim is about CPU consumed and not about time passing — those are the same
/// number for a spinning guest and very different for a halted one, which is
/// the entire point.
fn cpu_seconds() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // The comm field can contain spaces and parentheses, so fields are counted
    // from the closing parenthesis rather than from the start.
    let after_comm = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // utime and stime are fields 14 and 15 of the whole line; two are consumed
    // before the comm, so they are 11 and 12 of what is left.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let hz = 100.0; // USER_HZ, fixed at 100 on every Linux this runs on
    Some((utime + stime) as f64 / hz)
}

/// Resident set size in bytes, for the per-agent memory figure.
fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// How long the swarm is held idle while its cost is measured.
const IDLE_WINDOW: Duration = Duration::from_secs(3);

/// How long to wait before concluding a message did *not* arrive.
///
/// A negative claim needs a bound, and the bound has to be generous relative to
/// the positive case or it proves only that the host was slow. Deliveries that
/// do arrive are seen in single-digit milliseconds.
const ABSENCE_WINDOW: Duration = Duration::from_millis(500);

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("guest         : {}", elf.display());

    // Three named agents carry the checks; anything beyond them is there to be
    // counted. Three is the minimum the checks need -- a parent and two
    // siblings -- so the small case is the same swarm it always was.
    let mut total = 3usize;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agents" if i + 1 < args.len() => {
                total = args[i + 1].parse().unwrap_or(total).max(3);
                i += 1;
            }
            other => {
                eprintln!("unrecognised argument {other}");
                return std::process::ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let baseline_rss = rss_bytes();

    // The graph first. It is cheap, it is the thing being demonstrated, and a
    // topology mistake found before any VM exists costs nothing.
    let names = ["root", "w-a", "w-b"];
    let mut agents = BTreeMap::new();
    let started = Instant::now();
    for index in 0..total {
        let name = match index {
            0 => "root".to_string(),
            1 => "w-a".to_string(),
            2 => "w-b".to_string(),
            n => format!("w-{n}"),
        };
        // CIDs 0, 1 and 2 are reserved, so guests start at 3.
        match start_agent(&name, &elf, 3 + index as u64).await {
            Ok(agent) => {
                agents.insert(AgentId::new(name), agent);
            }
            Err(e) => {
                eprintln!("agent         : FAILED — {e}");
                eprintln!("This needs /dev/kvm.");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let booted = started.elapsed();
    println!(
        "agents        : {} unikernels booted and connected in {:.1} ms ({:.2} ms each)",
        agents.len(),
        booted.as_secs_f64() * 1000.0,
        booted.as_secs_f64() * 1000.0 / agents.len() as f64
    );
    if let (Some(base), Some(now)) = (baseline_rss, rss_bytes()) {
        let growth = now.saturating_sub(base);
        println!(
            "memory        : {:.2} MiB resident for the swarm, {:.3} MiB per agent",
            growth as f64 / (1024.0 * 1024.0),
            growth as f64 / (1024.0 * 1024.0) / agents.len() as f64
        );
    }

    let agents = Arc::new(agents);
    let mut swarm = Swarm::new(VsockTransport {
        agents: Arc::clone(&agents),
        next_id: 1,
        undeliverable: Vec::new(),
    });
    swarm.add_root("root").expect("root");
    swarm.add_agent("w-a", "root").expect("w-a");
    swarm.add_agent("w-b", "root").expect("w-b");
    for index in 3..total {
        swarm
            .add_agent(format!("w-{index}"), "root")
            .expect("worker");
    }

    let mut ok = true;

    // ── 1. A command down the hierarchy arrives ──────────────────────────
    let command = b"run: summarise the changelog".to_vec();
    match swarm.send("root", "w-a", command.clone()) {
        Ok(relation) => {
            let agent = &agents[&AgentId::new("w-a")];
            let arrived = wait_for(Duration::from_secs(5), || async {
                saw(agent, &command).await
            })
            .await;
            match arrived {
                Some(()) => {
                    println!("down          : ok — w-a's guest received the command ({relation})");
                }
                None => {
                    println!(
                        "down          : FAILED — the graph admitted it and w-a's guest never                          saw it"
                    );
                    ok = false;
                }
            }
        }
        Err(e) => {
            println!("down          : FAILED — the graph refused a command to a child: {e}");
            ok = false;
        }
    }

    // ── 2. A sideways message is refused, and does not arrive ────────────
    // The same payload is used again after the grant, so what changed between
    // the two attempts is the graph and nothing else. Its text is neutral for
    // that reason: after the grant this message is perfectly authorised.
    let sideways = b"run: check the disk usage".to_vec();
    match swarm.send("w-a", "w-b", sideways.clone()) {
        Err(Denied::NoGrant { .. }) => {
            // The verdict is the easy half. The claim is that nothing arrived,
            // so wait long enough for it to have, then look at the recipient.
            tokio::time::sleep(ABSENCE_WINDOW).await;
            let agent = &agents[&AgentId::new("w-b")];
            let echoed = agent.echoed();
            let console = agent.vm.console_output().await;
            if echoed.is_empty() && !console.contains("agent recv") {
                println!("sideways      : ok — refused, and w-b's guest received nothing");
            } else {
                println!(
                    "sideways      : FAILED — refused, but w-b received {:?}",
                    String::from_utf8_lossy(&echoed)
                );
                ok = false;
            }
        }
        Err(e) => {
            println!("sideways      : FAILED — refused for the wrong reason: {e}");
            ok = false;
        }
        Ok(_) => {
            println!("sideways      : FAILED — the graph admitted a message between siblings");
            ok = false;
        }
    }

    // ── 3. Granting the edge makes the same message arrive ───────────────
    swarm.grant("w-a", "w-b");
    match swarm.send("w-a", "w-b", sideways.clone()) {
        Ok(relation) => {
            let agent = &agents[&AgentId::new("w-b")];
            let arrived = wait_for(Duration::from_secs(5), || async {
                saw(agent, &sideways).await
            })
            .await;
            match arrived {
                Some(()) => println!(
                    "granted       : ok — the same message arrived once the edge existed \
                     ({relation})"
                ),
                None => {
                    println!("granted       : FAILED — granted, but w-b's guest never saw it");
                    ok = false;
                }
            }
        }
        Err(e) => {
            println!("granted       : FAILED — refused after the grant: {e}");
            ok = false;
        }
    }

    let undeliverable = std::mem::take(&mut swarm.transport_mut().undeliverable);
    for (agent, why) in &undeliverable {
        println!("undeliverable : {} — {why}", agent.as_str());
        ok = false;
    }

    // What the swarm costs while it is doing nothing, which is the state an
    // agent fleet spends almost all of its time in. Measured as CPU consumed
    // over a window of wall time: for a halted guest those two numbers are
    // unrelated, and for a spinning one they are the same.
    let cpu_before = cpu_seconds();
    let idle_started = Instant::now();
    tokio::time::sleep(IDLE_WINDOW).await;
    let idle_elapsed = idle_started.elapsed();
    if let (Some(before), Some(after)) = (cpu_before, cpu_seconds()) {
        let used = after - before;
        println!(
            "idle          : {:.2} s of CPU across {} agents over {:.1} s — {:.1}% of one core",
            used,
            agents.len(),
            idle_elapsed.as_secs_f64(),
            100.0 * used / idle_elapsed.as_secs_f64()
        );
    }

    println!();
    // Only the three named agents have anything to say; the rest booted,
    // connected and went to sleep, which is the point being measured.
    for name in names {
        let agent = &agents[&AgentId::new(name)];
        for line in agent.vm.console_output().await.lines() {
            println!("{name:<6} says  : {line}");
        }
    }
    println!();

    // Stopping is not a formality. Every guest is halted inside `KVM_RUN`
    // waiting for an interrupt that is not coming, which is exactly the state
    // that used to make `stop()` never return.
    let stopping = Instant::now();
    for (name, agent) in agents.iter() {
        if let Err(e) = agent.vm.stop().await {
            println!("stop          : {} — {e}", name.as_str());
            ok = false;
        }
    }
    println!(
        "stop          : {} halted guests in {:.1} ms",
        agents.len(),
        stopping.elapsed().as_secs_f64() * 1000.0
    );

    if ok {
        println!();
        println!(
            "result        : the command graph now decides who may talk to whom, and the \
             messages it admits arrive inside a hardware-isolated guest over vsock."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
