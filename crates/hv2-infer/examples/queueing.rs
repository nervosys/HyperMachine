//! What a request actually waits for when the lanes are full.
//!
//! A lane runs to the end of its answer, so the standing question has been
//! whether the queue needs a priority — a way for an urgent request to go
//! before one that merely arrived earlier. That is a product question, but it
//! has a measurable part, and the measurable part had never been measured:
//! *how long is the wait?*
//!
//! The product question has since been answered yes, so this now measures the
//! other half as well: **does saying an agent is urgent actually change what it
//! waits?** Two of the eight agents here are urgent, and the run is a failure
//! if their waits are not shorter than everyone else's — a priority that does
//! not show up in the numbers is not a priority.
//!
//! # Why this counts steps and not seconds
//!
//! A wait in milliseconds is a fact about the host. The same configuration on
//! this machine has measured 40.8, 54.3 and 58.9 ms per token in three
//! consecutive runs because the Windows side was busy, and a queueing result
//! that moves by 40% with someone else's build is not a result. A *step* is one
//! pass over the weights. Counting those makes the answer a property of the
//! scheduler, and it comes out the same on a loaded machine and an idle one.
//!
//! # What it does
//!
//! More agents ask at once than there are lanes, each from its own thread, so
//! the queue is genuinely over-subscribed rather than nominally so. Every
//! request reports the steps that ran between it joining the queue and it
//! getting a lane, and the report splits those by urgency.
//!
//! The urgency is passed to [`Scheduler::ask_at`] rather than decided by the
//! scheduler. Here it comes from a constant because this example has no swarm;
//! in `hv2-swarm/examples/scheduled` it comes from the capability graph, which
//! is where it belongs.
//!
//! ```text
//! cargo run --release -p hv2-infer --example queueing -- <model.gguf>
//! ```

use std::sync::Arc;
use std::time::Instant;

use hv2_infer::{Limits, Model, Scheduler};

/// Lanes. Deliberately fewer than the askers below, because a queue that never
/// fills answers nothing about queueing.
const LANES: usize = 2;

/// How many agents ask at once.
const ASKERS: usize = 8;

/// Short, so the run is bounded and every answer costs about the same number of
/// steps — which is what makes the waits comparable to each other.
const ANSWER_TOKENS: usize = 24;

/// Which of the agents are urgent. Two of eight, so there is something for
/// them to go ahead of.
const URGENT: [usize; 2] = [5, 6];

/// The two ranks this example uses, matching `hv2_swarm::Urgency`: smaller is
/// served sooner. Deliberately the *late* askers are the urgent ones — agents
/// 5 and 6 of 8 — because an urgent agent that also happened to arrive first
/// would prove nothing about ordering.
const URGENT_RANK: u8 = 0;
const ROUTINE_RANK: u8 = 1;

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: queueing <model.gguf>");
        return std::process::ExitCode::FAILURE;
    };

    let model = match Model::load(&path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let limits = Limits {
        batch: LANES,
        answer_tokens: ANSWER_TOKENS,
        ..Limits::default()
    };
    let scheduler = Arc::new(Scheduler::new(&model, limits));
    println!(
        "node          : {LANES} lanes, {ASKERS} agents asking at once, {ANSWER_TOKENS} answer tokens each"
    );
    println!("threads       : {}", scheduler.threads());
    println!();

    let started = Instant::now();
    let served = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..ASKERS)
            .map(|i| {
                let scheduler = Arc::clone(&scheduler);
                scope.spawn(move || {
                    let urgent = URGENT.contains(&i);
                    let agent = format!("agent-{i}");
                    let rank = if urgent { URGENT_RANK } else { ROUTINE_RANK };
                    let answer = scheduler.ask_at(&agent, "Name one colour. One word.", rank);
                    (agent, urgent, answer)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("the asking thread"))
            .collect::<Vec<_>>()
    });
    let elapsed = started.elapsed();

    let mut waits: Vec<usize> = Vec::new();
    let mut urgent_waits: Vec<usize> = Vec::new();
    let mut routine_waits: Vec<usize> = Vec::new();
    for (agent, urgent, outcome) in &served {
        let mark = if *urgent { "urgent " } else { "routine" };
        match outcome {
            Ok(Ok(answer)) => {
                println!(
                    "{agent:<9} {mark}: waited {:>3} steps ({:>5.1} s), shared a pass with {} others, said {:?}",
                    answer.waited_steps,
                    answer.waited.as_secs_f64(),
                    answer.batch.saturating_sub(1),
                    answer.text.trim()
                );
                waits.push(answer.waited_steps);
                if *urgent {
                    urgent_waits.push(answer.waited_steps);
                } else {
                    routine_waits.push(answer.waited_steps);
                }
            }
            Ok(Err(refused)) => println!("{agent:<9} {mark}: refused — {refused}"),
            Err(e) => println!("{agent:<9} {mark}: FAILED — {e}"),
        }
    }

    let stats = scheduler.stats();
    println!();
    println!(
        "queue         : {} admitted, {} served, peak {} waiting at once",
        stats.admitted, stats.served, stats.peak_waiting
    );
    println!(
        "work          : {} steps for {} answers, the largest pass carrying {}",
        stats.steps, stats.served, stats.largest_batch
    );
    println!("elapsed       : {:.1} s", elapsed.as_secs_f64());
    println!();

    waits.sort_unstable();
    let served_all = waits.len() == ASKERS;
    let queued = stats.peak_waiting > 1 && waits.last().is_some_and(|&w| w > 0);
    let longest = waits.last().copied().unwrap_or(0);
    // How long one answer holds a lane, which is what a waiting request is
    // waiting for. Not steps divided by answers: with two lanes a step serves
    // two answers at once, so that figure is the batching-amortised cost, and
    // dividing a wait by it overstates the wait by the batch width. The lane
    // occupancy is steps divided by the number of *rounds* of lanes.
    let rounds = stats.served.div_ceil(LANES).max(1);
    let per_answer = stats.steps / rounds;

    println!("everyone served: {}", yes_no(served_all));
    println!(
        "the queue filled: {}  (peak {} waiting; a run where nothing queued would answer nothing)",
        yes_no(queued),
        stats.peak_waiting
    );
    println!(
        "waits, in steps: {waits:?}  — longest {longest}, against {per_answer} steps that one answer holds a lane"
    );
    println!();

    if !served_all || !queued {
        println!(
            "result        : the queue did not fill, so this run says nothing about queueing. \
             Raise ASKERS or lower LANES."
        );
        return std::process::ExitCode::FAILURE;
    }

    let mean = |v: &[usize]| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<usize>() as f64 / v.len() as f64
        }
    };
    let urgent_mean = mean(&urgent_waits);
    let routine_mean = mean(&routine_waits);
    urgent_waits.sort_unstable();
    routine_waits.sort_unstable();
    println!("urgent waits  : {urgent_waits:?}  mean {urgent_mean:.1} steps");
    println!("routine waits : {routine_waits:?}  mean {routine_mean:.1} steps");
    println!();

    // The point of the whole change has to show up here. A priority that does
    // not move the number is not a priority, and a run that cannot tell the
    // difference should fail rather than reassure.
    if urgent_waits.is_empty() || routine_waits.is_empty() {
        println!("result        : FAILED — one of the two groups was never served.");
        return std::process::ExitCode::FAILURE;
    }
    if urgent_mean >= routine_mean {
        println!(
            "result        : FAILED — urgent agents waited {urgent_mean:.1} steps against \
             {routine_mean:.1} for routine ones, so the urgency never reached the queue."
        );
        return std::process::ExitCode::FAILURE;
    }

    println!(
        "result        : with {LANES} lanes and {ASKERS} askers, the longest wait was {longest} \
         steps, against {per_answer} steps that one answer holds a lane — {:.1} answers' worth, \
         where askers/lanes - 1 predicts {}. That is what a request waits for: a lane, freed when \
         an answer finishes. The two urgent agents asked fifth and sixth of eight and waited \
         {urgent_mean:.1} steps against {routine_mean:.1} for the rest, so urgency reordered the \
         queue rather than merely being recorded in it. The wait it saved did not vanish — it \
         moved onto the routine agents, which is the trade, and the capability graph is where the \
         decision to make it lives. Nothing starves for making it: a waiting request gains a rank \
         every {} steps, so it cannot be overtaken by newly-arriving urgent work for longer than \
         its rank times that.",
        longest as f64 / per_answer.max(1) as f64,
        ASKERS / LANES - 1,
        limits.patience
    );
    std::process::ExitCode::SUCCESS
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}
