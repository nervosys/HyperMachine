//! A fleet sharing one model: a queue, a conversation each, and a bound.
//!
//! `inference` proved a model answers an agent that holds the capability. It
//! did it one agent at a time, in a loop, with a fresh conversation for each
//! request — which is fine as a proof that the parts connect and is not how a
//! node with a hundred agents on it behaves.
//!
//! This is the shape that follows from the measurement. A forward pass is
//! bounded by memory bandwidth, so a node runs a small fixed number at once and
//! everyone else waits; an agent's conversation is the one thing it cannot
//! share, so each agent holds its own and is bounded in how much of it it may
//! hold.
//!
//! # What is asserted
//!
//! - **An agent remembers its own conversation.** Two agents are each told a
//!   number and then asked, in a second turn, what it was. The answer comes
//!   from the earlier turn, which is still in that agent's cache.
//! - **And cannot read anyone else's.** A third agent, granted the same
//!   capability and given only the *second* question, does not know the number.
//!   Without this, an agent answering "7" would prove nothing — the model might
//!   simply favour it.
//! - **The queue is real.** Every agent asks at once, from its own thread, and
//!   the scheduler serves them one at a time. Peak queue depth is reported: if
//!   it never exceeds one, nothing was scheduled and this example is measuring
//!   an empty claim.
//! - **The bound refuses, and refuses cheaply.** An agent over its context cap
//!   is turned away with the numbers in the refusal, and *before* it takes a
//!   place in the queue — counted at the scheduler.
//! - **A refused agent never reaches the model.** The capability is checked
//!   before the request is submitted, and the forward-pass count is read from
//!   the scheduler rather than from the gate.
//!
//! ```text
//! cargo run --release -p hv2-swarm --example scheduled -- <model.gguf>
//! ```
//!
//! Needs `/dev/kvm`, the `x86_64-unknown-none` target, and a Llama-architecture
//! GGUF on a Linux filesystem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hv2_agent_proto::{parse, Header, Kind, HEADER_LEN};
use hv2_core::devices::virtio_vsock::{VsockConnectionId, VsockConnectionState, VsockDevice};
use hv2_core::{BootSource, VMConfig, VM};
use hv2_infer::{Limits, Model, Refused, Scheduler};
use hv2_swarm::{AgentId, Capability, Swarm, Urgency};

const GUEST_TARGET: &str = "x86_64-unknown-none";
const HOST_PORT: u32 = 1024;
const GUEST_PORT: u32 = 5000;
const BOUND: Duration = Duration::from_secs(10);

/// The capability that gates the model.
const TOOL: &str = "infer";

/// The number an agent is asked to hold on to, and the question that asks for
/// it back.
const REMEMBER: &str = "Remember the number 7. Reply with just OK.";
const RECALL: &str = "What number did I ask you to remember? Reply with just the number.";

/// Who asks what.
///
/// `alpha` and `beta` hold a conversation. `gamma` is the control: same
/// capability, same second question, no first turn — so if it answers 7 the
/// other two prove nothing. `delta` holds no capability at all.
struct Plan {
    name: &'static str,
    questions: &'static [&'static str],
    granted: bool,
    /// Whether the last answer should contain the number.
    expects_recall: bool,
    /// How urgent this agent is, as a fact recorded in the swarm rather than
    /// passed to the scheduler by hand. `beta` is urgent here to show the path
    /// works end to end; with four agents and eight lanes nothing queues, so
    /// this demonstrates the wiring and `hv2-infer/examples/queueing` is where
    /// the effect on waits is measured.
    urgency: Urgency,
}

const PLANS: [Plan; 4] = [
    Plan {
        name: "alpha",
        urgency: Urgency::Routine,
        questions: &[REMEMBER, RECALL],
        granted: true,
        expects_recall: true,
    },
    Plan {
        name: "beta",
        urgency: Urgency::Urgent,
        questions: &[REMEMBER, RECALL],
        granted: true,
        expects_recall: true,
    },
    Plan {
        name: "gamma",
        urgency: Urgency::Routine,
        questions: &[RECALL],
        granted: true,
        expects_recall: false,
    },
    Plan {
        name: "delta",
        urgency: Urgency::Routine,
        questions: &[RECALL],
        granted: false,
        expects_recall: false,
    },
];

/// The sync half of an agent: enough to talk to its guest from a plain thread.
///
/// The VM's console is behind an async call and the device is not, which is
/// what lets the conversations below run on scoped threads while the scheduler
/// blocks them.
#[derive(Clone)]
struct Wire {
    device: Arc<parking_lot::Mutex<VsockDevice>>,
    connection: VsockConnectionId,
}

impl Wire {
    fn send(&self, payload: &[u8]) {
        let _ = self.device.lock().send(self.connection, payload);
    }

    fn take(&self) -> Vec<u8> {
        self.device.lock().recv(self.connection).unwrap_or_default()
    }

    /// Wait for the guest to ask for a tool, and return the request id and what
    /// it asked for.
    fn take_call(&self) -> Option<(u32, String)> {
        let deadline = Instant::now() + BOUND;
        let mut buffered = Vec::new();
        while Instant::now() < deadline {
            buffered.extend_from_slice(&self.take());
            if let Some((header, body)) = parse(&buffered) {
                if header.kind == Kind::ToolCall {
                    return Some((header.id, String::from_utf8_lossy(body).to_string()));
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }
}

struct Agent {
    vm: Arc<VM>,
    wire: Wire,
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
        wire: Wire { device, connection },
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

/// What one agent's whole conversation amounted to.
struct Transcript {
    name: &'static str,
    turns: Vec<Turn>,
}

struct Turn {
    outcome: String,
    batch: usize,
    waited: Duration,
    /// The same wait counted in token steps, which does not move when the host
    /// is busy and is therefore the number the priority question is about.
    waited_steps: usize,
    ran: Duration,
    context: usize,
    /// The answer text, empty if it was refused.
    answer: String,
}

/// Run one agent's side of the conversation, on its own thread.
///
/// Everything here is synchronous on purpose: the scheduler blocks the caller
/// while it waits for a turn, and that blocking is the demonstration. A future
/// that yielded instead would show the runtime interleaving rather than the
/// scheduler ordering.
fn converse(
    plan: &'static Plan,
    wire: &Wire,
    swarm: &Swarm<hv2_swarm::LocalTransport>,
    scheduler: &Scheduler<'_>,
) -> Transcript {
    let id = AgentId::new(plan.name);
    let capability = Capability::new(format!("tool:{TOOL}"));
    let mut turns = Vec::new();

    for (index, question) in plan.questions.iter().enumerate() {
        wire.send(&frame(
            index as u32 + 1,
            Kind::Task,
            format!("{TOOL}: {question}").as_bytes(),
        ));
        let Some((request, asked)) = wire.take_call() else {
            turns.push(Turn {
                outcome: "the guest never asked".to_string(),
                waited: Duration::ZERO,
                waited_steps: 0,
                ran: Duration::ZERO,
                context: 0,
                batch: 0,
                answer: String::new(),
            });
            continue;
        };
        let argument = asked.split_once(':').map_or("", |(_, rest)| rest).trim();

        // The capability, before the queue. An agent that may not ask does not
        // get a place in it.
        if !swarm.holds(&id, &capability) {
            wire.send(&frame(request, Kind::Error, b"refused"));
            turns.push(Turn {
                outcome: "refused: no capability, never queued".to_string(),
                waited: Duration::ZERO,
                waited_steps: 0,
                ran: Duration::ZERO,
                context: 0,
                batch: 0,
                answer: String::new(),
            });
            continue;
        }

        // Read off the graph, not decided here and not decided by the
        // scheduler: the same object that said this agent may ask also says how
        // soon.
        let urgency = swarm.urgency_of(&id).rank();
        match scheduler.ask_at(plan.name, argument, urgency) {
            Ok(Ok(served)) => {
                wire.send(&frame(
                    request,
                    Kind::ToolResult,
                    served.text.trim().as_bytes(),
                ));
                turns.push(Turn {
                    outcome: "served".to_string(),
                    waited: served.waited,
                    waited_steps: served.waited_steps,
                    ran: served.ran,
                    context: served.context,
                    batch: served.batch,
                    answer: served.text.trim().to_string(),
                });
            }
            Ok(Err(refused @ Refused::ContextFull { .. })) => {
                wire.send(&frame(request, Kind::Error, refused.to_string().as_bytes()));
                turns.push(Turn {
                    outcome: format!("refused: {refused}"),
                    waited: Duration::ZERO,
                    waited_steps: 0,
                    ran: Duration::ZERO,
                    context: 0,
                    batch: 0,
                    answer: String::new(),
                });
            }
            Err(e) => {
                wire.send(&frame(request, Kind::Error, b"the model failed"));
                turns.push(Turn {
                    outcome: format!("FAILED — {e}"),
                    waited: Duration::ZERO,
                    waited_steps: 0,
                    ran: Duration::ZERO,
                    context: 0,
                    batch: 0,
                    answer: String::new(),
                });
            }
        }
    }

    Transcript {
        name: plan.name,
        turns,
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: scheduled <model.gguf>");
        eprintln!();
        eprintln!("A Llama-architecture GGUF in Q8_0, F16 or F32, on a Linux filesystem.");
        return std::process::ExitCode::FAILURE;
    };

    let started = Instant::now();
    let model = match Model::load(&path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let s = &model.shape;
    println!(
        "model         : {} layers, width {}, vocab {} — {:.0} MiB mapped, loaded in {:.1} s",
        s.layers,
        s.width,
        s.vocab,
        model.mapped_bytes() as f64 / (1024.0 * 1024.0),
        started.elapsed().as_secs_f64()
    );

    // The cap is deliberately small enough that a third turn would not fit, so
    // the bound is exercised rather than described. At 64 KiB of key/value
    // cache per token it is also a real memory figure: 96 tokens is 6 MiB.
    let limits = Limits {
        // Four lanes for four agents: one pass over the weights advances every
        // conversation that is waiting.
        batch: 4,
        context_tokens: 96,
        answer_tokens: 24,
        // No node-wide bound for the run above: the point of it is the queue
        // and the conversations, and an eviction in the middle would be a
        // second thing happening at once. The bound gets its own section at the
        // end, where it is the only thing being shown.
        cache_bytes: 0,
        // The default. Nothing queues in this run — four lanes for four agents
        // — so ageing has nothing to act on here; it is the queueing example
        // that exercises it.
        patience: Limits::default().patience,
        // Zero: let the scheduler choose, which is a third of the machine
        // rather than all of it. On this host that is worth 2.2x against the
        // global pool, and the reason is in `Limits::threads`.
        threads: 0,
    };
    let scheduler = Scheduler::new(&model, limits);
    println!(
        "scheduler     : batches of {} on {} of {} cores, {} tokens of context per agent ({:.1} MiB of cache), {} tokens per answer",
        limits.batch,
        scheduler.threads(),
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        limits.context_tokens,
        (limits.context_tokens * model.cache_bytes_per_token()) as f64 / (1024.0 * 1024.0),
        limits.answer_tokens
    );

    let elf = match build_guest() {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("guest build   : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let mut agents = BTreeMap::new();
    for (index, plan) in PLANS.iter().enumerate() {
        match start_agent(plan.name, &elf, 3 + index as u64).await {
            Ok(agent) => {
                agents.insert(plan.name, agent);
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
    for plan in &PLANS {
        swarm.add_agent(plan.name, "root").expect("an agent");
        if plan.granted {
            swarm.grant_capability(&AgentId::new(plan.name), format!("tool:{TOOL}"));
        }
        // Urgency lives beside the capability, in the graph, for the same
        // reason: both are statements about the agent rather than about the
        // request it happens to be making.
        swarm.set_urgency(&AgentId::new(plan.name), plan.urgency);
        {}
    }
    println!(
        "agents        : {} sandboxes — {} hold tool:{TOOL}",
        PLANS.len(),
        PLANS.iter().filter(|p| p.granted).count()
    );
    println!();

    // Everyone at once, each on its own thread, so the queue has something to
    // do. Scoped threads because the scheduler borrows the model.
    let began = Instant::now();
    let transcripts = std::thread::scope(|scope| {
        let handles: Vec<_> = PLANS
            .iter()
            .map(|plan| {
                let wire = agents[plan.name].wire.clone();
                let swarm = &swarm;
                let scheduler = &scheduler;
                scope.spawn(move || converse(plan, &wire, swarm, scheduler))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a conversation thread"))
            .collect::<Vec<_>>()
    });
    let wall = began.elapsed();

    let mut ok = true;
    for transcript in &transcripts {
        for (turn, record) in transcript.turns.iter().enumerate() {
            if record.answer.is_empty() {
                println!(
                    "{:<6} turn {}: {}",
                    transcript.name,
                    turn + 1,
                    record.outcome
                );
            } else {
                println!(
                    "{:<6} turn {}: {:?}  [waited {:.1} s = {} steps, batch of {} ran {:.1} s, context {} tokens]",
                    transcript.name,
                    turn + 1,
                    record.answer,
                    record.waited.as_secs_f64(),
                    record.waited_steps,
                    record.batch,
                    record.ran.as_secs_f64(),
                    record.context
                );
            }
        }
    }

    // ── the queue was a queue ───────────────────────────────────────────
    let stats = scheduler.stats();
    // The ceiling: only agents holding the capability ever reach the queue, and
    // a pass carries at most as many lanes as the scheduler was given. Saying
    // what the ceiling is stops "3" reading as a shortfall when it is the whole
    // fleet.
    let possible = PLANS.iter().filter(|p| p.granted).count().min(limits.batch);
    // Per-request service time cannot be summed: a batch's duration belongs to
    // every request in it, so adding them up counts one pass several times and
    // reports more service than there was wall clock. What a request actually
    // cost is its batch divided by how many shared it.
    let service: Duration = transcripts
        .iter()
        .flat_map(|t| t.turns.iter())
        .filter(|r| r.batch > 0)
        .map(|r| r.ran / r.batch as u32)
        .sum();
    println!();
    println!(
        "queue         : {} admitted, {} served, {} refused before queueing — peak {} waiting at \
         once",
        stats.admitted, stats.served, stats.refused, stats.peak_waiting
    );
    println!(
        "steps         : {} passes over the weights for {} answers — the most conversations one pass carried was {} of a possible {}",
        stats.steps,
        stats.served,
        stats.largest_batch,
        possible
    );
    if stats.largest_batch < 2 {
        println!(
            "               FAILED — every pass read the whole model to produce one token, so nothing was batched"
        );
        ok = false;
    } else if stats.largest_batch < possible {
        // Not a failure: the agents are real sandboxes and do not arrive in
        // lockstep. Worth saying, because the gap between this and `possible`
        // is exactly what continuous batching is for.
        println!(
            "               (they did not all overlap — {} of {possible} is what the timing              allowed)",
            stats.largest_batch
        );
    }
    println!(
        "time          : {:.1} s of wall clock, {:.1} s of it someone's share of a pass — {:.0}% busy",
        wall.as_secs_f64(),
        service.as_secs_f64(),
        service.as_secs_f64() / wall.as_secs_f64() * 100.0
    );
    if stats.peak_waiting < 2 {
        println!(
            "               FAILED — nothing ever queued, so nothing here was scheduled. The \
             agents did not overlap."
        );
        ok = false;
    }

    // ── each agent remembered its own conversation, and only its own ────
    println!();
    for (plan, transcript) in PLANS.iter().zip(&transcripts) {
        let Some(last) = transcript.turns.last() else {
            continue;
        };
        if !plan.granted {
            continue;
        }
        let knew = last.answer.contains('7');
        match (knew, plan.expects_recall) {
            (true, true) => println!(
                "{:<6}        : recalled the number from its own first turn",
                plan.name
            ),
            (false, false) => println!(
                "{:<6}        : was never told, and did not know — {:?}",
                plan.name,
                last.answer.chars().take(40).collect::<String>()
            ),
            (false, true) => {
                println!(
                    "{:<6}        : FAILED — it was told, and did not recall: {:?}",
                    plan.name, last.answer
                );
                ok = false;
            }
            (true, false) => {
                println!(
                    "{:<6}        : FAILED — it was never told and answered anyway, so the other \
                     agents' recall proves nothing",
                    plan.name
                );
                ok = false;
            }
        }
    }

    // ── the bound refused a third turn, cheaply ─────────────────────────
    println!();
    let over = scheduler.ask("alpha", RECALL);
    match over {
        Ok(Err(refused)) => {
            println!("bound         : alpha asked again and was refused — {refused}");
        }
        Ok(Ok(_)) => {
            println!(
                "bound         : FAILED — alpha is holding {} tokens and a further turn was \
                 admitted under a cap of {}",
                scheduler.context_of("alpha"),
                limits.context_tokens
            );
            ok = false;
        }
        Err(e) => {
            println!("bound         : FAILED — {e}");
            ok = false;
        }
    }

    // ── and the agent without the capability never reached the model ────
    let delta_turns: usize = transcripts
        .iter()
        .find(|t| t.name == "delta")
        .map_or(0, |t| {
            t.turns.iter().filter(|r| !r.answer.is_empty()).count()
        });
    println!(
        "capability    : delta asked and was answered {delta_turns} time(s) — it holds no \
         tool:{TOOL}, so the request never reached the queue"
    );
    if delta_turns != 0 {
        ok = false;
    }

    println!();
    for plan in &PLANS {
        for line in agents[plan.name]
            .vm
            .console_output()
            .await
            .lines()
            .filter(|l| l.starts_with("agent done") || l.starts_with("agent denied"))
        {
            println!("{:<6} says  : {line}", plan.name);
        }
    }

    // ── the node's own bound, and who pays for it ───────────────────────
    //
    // `context_tokens` bounds one agent. This bounds the node, and they are
    // different problems: an agent that behaves is still a problem if a
    // thousand of them each hold a small conversation and nothing ever lets one
    // go. A second scheduler, with room for about one conversation, over the
    // same model — the weights are mapped once and neither scheduler copies
    // them.
    println!();
    let tight = Limits {
        batch: 2,
        context_tokens: 96,
        answer_tokens: 4,
        // About one short conversation. The second fits; the third has to
        // displace something.
        cache_bytes: 24 * model.cache_bytes_per_token(),
        patience: limits.patience,
        threads: limits.threads,
    };
    let small = Scheduler::new(&model, tight);
    println!(
        "cache bound   : {:.1} MiB across the whole node — about {} tokens of context. A conversation in a lane cannot be dropped, so the total can sit above this while somebody is being served.",
        tight.cache_bytes as f64 / (1024.0 * 1024.0),
        tight.cache_bytes / model.cache_bytes_per_token()
    );

    // Five asks by three agents, arranged so the signal has to be right in
    // both directions. `two` asks twice in a row, so its second answer must
    // report `continued: true` — otherwise a flag that is always false would
    // pass the eviction check below and mean nothing. `one` goes first and is
    // therefore the least recently used when room is needed, so it is what gets
    // dropped, and its last ask is where it finds out.
    let mut kept_told = false;
    let mut evicted_told = false;
    for (round, who) in ["one", "two", "two", "three", "one"].iter().enumerate() {
        match small.ask(who, "Say OK.") {
            Ok(Ok(served)) => {
                println!(
                    "  ask {}         : {who} -> {:?}, continued {}, node now holding {:.1} MiB",
                    round + 1,
                    served.text.trim(),
                    served.continued,
                    small.cache_bytes() as f64 / (1024.0 * 1024.0)
                );
                // `two` asking twice in a row must continue; `one` asking at
                // the end must not, because it was dropped to make room.
                if round == 2 && served.continued {
                    kept_told = true;
                }
                if round == 4 && !served.continued {
                    evicted_told = true;
                }
            }
            Ok(Err(refused)) => println!("  ask {}         : refused — {refused}", round + 1),
            Err(e) => {
                println!("  ask {}         : FAILED — {e}", round + 1);
                ok = false;
            }
        }
    }
    let evictions = small.stats().evicted;
    println!("evicted       : {evictions} conversation(s) dropped to stay inside the bound",);
    if evictions == 0 {
        println!("               FAILED — the bound was never enforced");
        ok = false;
    }
    if !kept_told {
        println!(
            "               FAILED — an agent asked twice in a row and was told its conversation was new, so the flag is not reporting anything"
        );
        ok = false;
    }
    if !evicted_told {
        println!(
            "               FAILED — a conversation was dropped and the agent it belonged to was not told, which is the failure this signal exists to prevent"
        );
        ok = false;
    } else {
        println!(
            "told          : `two` asking twice running got `continued: true`, and the agent whose context was dropped got `continued: false` — the flag distinguishes, which is what makes it worth reading"
        );
    }

    for plan in &PLANS {
        let _ = agents[plan.name].vm.stop().await;
    }

    println!();
    if ok {
        println!(
            "result        : four sandboxes over one model, served from a first-in-first-out queue whose lanes fill and empty between token steps. Each kept its own conversation and could not see anyone else's; the one over its own context bound was refused without taking a place in the queue; the one without the capability never reached it; and when the node's cache bound was reached, the least recently used conversation was dropped and the agent it belonged to was told."
        );
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
