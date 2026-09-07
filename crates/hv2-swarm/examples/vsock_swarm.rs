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
//! Each agent is a VM whose guest polls its virtqueue in a spin loop, so each
//! costs a host core while it runs. Three agents, deliberately: this
//! demonstrates the transport, and `unikernel_swarm` is where the graph is run
//! at a thousand agents. A polling guest is also only stoppable because
//! `stop()` can now interrupt a vCPU that never leaves the guest on its own.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example vsock_swarm
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
        if let Err(e) = agent.device.lock().send(agent.connection, &message.payload) {
            self.undeliverable.push((message.to, e.to_string()));
        }
    }
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

    // The graph first. It is cheap, it is the thing being demonstrated, and a
    // topology mistake found before any VM exists costs nothing.
    let names = ["root", "w-a", "w-b"];
    let mut agents = BTreeMap::new();
    let started = Instant::now();
    for (index, name) in names.iter().enumerate() {
        // CIDs 0, 1 and 2 are reserved, so guests start at 3.
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
        "agents        : {} unikernels booted and connected in {:.1} ms",
        agents.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );

    let agents = Arc::new(agents);
    let mut swarm = Swarm::new(VsockTransport {
        agents: Arc::clone(&agents),
        undeliverable: Vec::new(),
    });
    swarm.add_root("root").expect("root");
    swarm.add_agent("w-a", "root").expect("w-a");
    swarm.add_agent("w-b", "root").expect("w-b");

    let mut ok = true;

    // ── 1. A command down the hierarchy arrives ──────────────────────────
    let command = b"run: summarise the changelog".to_vec();
    match swarm.send("root", "w-a", command.clone()) {
        Ok(relation) => {
            let agent = &agents[&AgentId::new("w-a")];
            let arrived = wait_for(Duration::from_secs(5), || async {
                agent.echoed() == command
            })
            .await;
            match arrived {
                Some(()) => {
                    println!("down          : ok — w-a echoed the command back ({relation})");
                }
                None => {
                    println!(
                        "down          : FAILED — the graph admitted it but w-a echoed {:?}",
                        String::from_utf8_lossy(&agent.echoed())
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
                agent.echoed() == sideways
            })
            .await;
            match arrived {
                Some(()) => println!(
                    "granted       : ok — the same message arrived once the edge existed \
                     ({relation})"
                ),
                None => {
                    println!("granted       : FAILED — granted, but w-b echoed nothing");
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

    println!();
    for name in names {
        let agent = &agents[&AgentId::new(name)];
        for line in agent.vm.console_output().await.lines() {
            println!("{name:<6} says  : {line}");
        }
    }
    println!();

    // Stopping is not a formality here. Every guest is spinning on its
    // virtqueue, so every one of them is a vCPU that never leaves the guest of
    // its own accord.
    let stopping = Instant::now();
    for name in names {
        if let Err(e) = agents[&AgentId::new(name)].vm.stop().await {
            println!("stop          : {name} — {e}");
            ok = false;
        }
    }
    println!(
        "stop          : {} spinning guests in {:.1} ms",
        names.len(),
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
