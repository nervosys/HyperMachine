//! Deliver a swarm message into a running guest, and watch the guest act on it.
//!
//! Until now `Transport::deliver` put messages in a host-side queue. The
//! permission graph was real and the VMs were real, but nothing crossed
//! between them: an agent inside a guest could not receive anything. This is
//! the crossing.
//!
//! # Why the serial port and not vsock
//!
//! vsock is the right transport and is not reachable from a guest this small.
//! It needs protected mode, a virtio transport, and virtqueue handling — a
//! kernel, or something close enough to one that the unikernel argument stops
//! applying. The serial port is a device the host already emulates in both
//! directions, and a guest can drive it in fifteen bytes.
//!
//! So this is not the final transport. It is a real one: the bytes leave the
//! host, enter the guest through a device model, are read by guest code, and
//! come back out. Everything the permission graph decides is unchanged by
//! which device carries the result.
//!
//! # The guest
//!
//! An echo loop. Poll the line status register until a byte is ready, read it,
//! write it back, repeat.
//!
//! ```text
//!   0: BA FD 03   mov dx, 0x3FD    line status register
//!   3: EC         in  al, dx
//!   4: A8 01      test al, 1       data ready?
//!   6: 74 FB      jz  -5 -> 3      no: poll again
//!   8: BA F8 03   mov dx, 0x3F8    data register
//!  11: EC         in  al, dx       take the byte the host sent
//!  12: EE         out dx, al       send it back
//!  13: EB F1      jmp -15 -> 0     wait for the next one
//! ```
//!
//! Echoing is the smallest thing that proves receipt. A guest that took the
//! byte and stayed silent would be indistinguishable from one that never ran,
//! which is the failure this whole example exists to rule out.
//!
//! # Running it
//!
//! ```text
//! cargo run --release -p hv2-swarm --example guest_transport
//! ```
//!
//! Needs `/dev/kvm`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hv2_core::{BootSource, VMConfig, VM};
use hv2_swarm::{AgentId, Denied, Message, Relation, Swarm, Transport};

/// COM1, which `Machine::legacy_pc` maps to a 16550.
const COM1: u16 = 0x3F8;

/// Poll COM1 for a byte, echo it, repeat. Fifteen bytes.
fn echo_guest() -> Vec<u8> {
    vec![
        0xBA, 0xFD, 0x03, // mov dx, 0x3FD
        0xEC, // in al, dx
        0xA8, 0x01, // test al, 1
        0x74, 0xFB, // jz -5
        0xBA, 0xF8, 0x03, // mov dx, 0x3F8
        0xEC, // in al, dx
        0xEE, // out dx, al
        0xEB, 0xF1, // jmp -15
    ]
}

/// Where a delivered message goes: into the recipient's guest.
///
/// Holds the outbound side only. Whether the guest answered is read from the
/// VM afterwards, because a transport that reported its own success would be
/// asserting the thing under test.
struct GuestTransport {
    vms: BTreeMap<AgentId, Arc<VM>>,
    /// Deliveries that could not reach a guest, kept rather than discarded so
    /// the run can report them instead of appearing to have worked.
    undeliverable: Arc<Mutex<Vec<(AgentId, String)>>>,
    handle: tokio::runtime::Handle,
}

impl Transport for GuestTransport {
    fn deliver(&mut self, message: Message) {
        let Some(vm) = self.vms.get(&message.to).cloned() else {
            self.undeliverable
                .lock()
                .expect("undeliverable list poisoned")
                .push((message.to.clone(), "no VM for this agent".to_string()));
            return;
        };

        let payload = message.payload.clone();
        let to = message.to.clone();
        let undeliverable = Arc::clone(&self.undeliverable);
        // `Transport::deliver` is synchronous and the device manager is async,
        // and `send` is called from inside the runtime. `Handle::block_on`
        // alone panics there -- "cannot start a runtime from within a runtime"
        // -- so the worker is handed back to the scheduler for the duration
        // with `block_in_place`, and the delivery is driven on it.
        //
        // Blocking rather than spawning, deliberately: a delivery that
        // completes after `send` returns would make "the message arrived"
        // and "the message was accepted" two different moments, and every
        // assertion about what a guest received would become a race.
        let handle = self.handle.clone();
        tokio::task::block_in_place(move || {
            handle.block_on(async move {
                let Some(com1) = vm.devices().find_io_device(COM1).await else {
                    undeliverable
                        .lock()
                        .expect("undeliverable list poisoned")
                        .push((to, "no COM1 on this VM".to_string()));
                    return;
                };
                if let Err(e) = com1.console_input(&payload).await {
                    undeliverable
                        .lock()
                        .expect("undeliverable list poisoned")
                        .push((to, format!("COM1 refused the input: {e}")));
                }
            })
        });
    }
}

/// Wait for `vm` to echo `expected`, or give up.
async fn echoed(vm: &Arc<VM>, expected: &str, within: Duration) -> Option<String> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        let out = vm.console_output().await;
        if out.contains(expected) {
            return Some(out);
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    None
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let dir = std::env::temp_dir().join("hv2-swarm");
    let image = dir.join("echo.bin");
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&image, echo_guest()).is_err() {
        eprintln!("could not write the guest image to {}", image.display());
        return std::process::ExitCode::FAILURE;
    }

    let names = ["root", "worker-a", "worker-b"];
    let mut vms = BTreeMap::new();
    for name in names {
        let config = VMConfig {
            name: name.to_string(),
            vcpu_count: 1,
            memory_size: 16 * 1024 * 1024,
            boot: Some(BootSource::raw(&image)),
            ..Default::default()
        };
        let vm = match VM::new(config) {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                eprintln!("agent '{name}': no hypervisor backend — {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        if vm.provision().await.is_err() || vm.launch().await.is_err() {
            eprintln!("agent '{name}': could not be started");
            return std::process::ExitCode::FAILURE;
        }
        vms.insert(AgentId::new(name), vm);
    }
    println!("agents        : {} guests running an echo loop", vms.len());

    let undeliverable = Arc::new(Mutex::new(Vec::new()));
    let transport = GuestTransport {
        vms: vms.clone(),
        undeliverable: Arc::clone(&undeliverable),
        handle: tokio::runtime::Handle::current(),
    };

    let mut swarm = Swarm::new(transport);
    swarm.add_root("root").expect("root");
    swarm.add_agent("worker-a", "root").expect("worker-a");
    swarm.add_agent("worker-b", "root").expect("worker-b");

    let mut failures = Vec::new();

    // Allowed: the root commands a worker, and the bytes reach the guest.
    match swarm.send("root", "worker-a", b"PING-A".to_vec()) {
        Ok(Relation::Descendant) => match echoed(
            &vms[&AgentId::new("worker-a")],
            "PING-A",
            Duration::from_secs(5),
        )
        .await
        {
            Some(out) => println!("delivered     : root -> worker-a, guest echoed {out:?}"),
            None => failures.push("worker-a was sent PING-A and never echoed it".to_string()),
        },
        other => failures.push(format!(
            "root -> worker-a should be a command, got {other:?}"
        )),
    }

    // Refused: siblings, with no grant. The bytes must not reach the guest,
    // which is checked at the guest rather than at the return value.
    match swarm.send("worker-a", "worker-b", b"SECRET".to_vec()) {
        Err(Denied::NoGrant { .. }) => {
            // Give it as long as the successful delivery took, so "nothing
            // arrived" is a result and not just impatience.
            let leaked = echoed(
                &vms[&AgentId::new("worker-b")],
                "SECRET",
                Duration::from_millis(500),
            )
            .await;
            match leaked {
                None => println!("refused       : worker-a -> worker-b, nothing reached the guest"),
                Some(out) => failures.push(format!(
                    "worker-a -> worker-b was refused but the guest received it: {out:?}"
                )),
            }
        }
        other => failures.push(format!(
            "worker-a -> worker-b should be refused, got {other:?}"
        )),
    }

    // Granted, then delivered for real.
    swarm.grant("worker-a", "worker-b");
    match swarm.send("worker-a", "worker-b", b"NOW-OK".to_vec()) {
        Ok(Relation::Granted) => {
            match echoed(
                &vms[&AgentId::new("worker-b")],
                "NOW-OK",
                Duration::from_secs(5),
            )
            .await
            {
                Some(_) => println!("granted       : worker-a -> worker-b, guest echoed it"),
                None => failures.push("worker-b was granted the edge and never echoed".to_string()),
            }
        }
        other => failures.push(format!(
            "granted send should have been allowed, got {other:?}"
        )),
    }

    for vm in vms.values() {
        let _ = vm.stop().await;
    }

    let stuck = undeliverable.lock().expect("undeliverable list poisoned");
    if !stuck.is_empty() {
        eprintln!("\nundeliverable:");
        for (agent, why) in stuck.iter() {
            eprintln!("  {agent}: {why}");
        }
        return std::process::ExitCode::FAILURE;
    }
    if !failures.is_empty() {
        eprintln!("\nFAILED:");
        for failure in &failures {
            eprintln!("  {failure}");
        }
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "\nresult        : the graph decided, and the bytes crossed into the guests it allowed \
         and into none it refused"
    );
    std::process::ExitCode::SUCCESS
}
