//! Four conversations in one pass over the weights, and the same answers.
//!
//! Batching is the largest number available to a fleet: a forward pass reads
//! 1.25&nbsp;GiB of weights whether it produces one token or eight, so eight
//! agents waiting should cost one read rather than eight. The throughput is
//! easy to measure and easy to fake — a batch that quietly mixed two
//! conversations together would run at exactly the same speed.
//!
//! So this measures nothing. It asks four different questions twice: once each
//! on their own, and once all four in a single batch, and checks the answers
//! are the same. That is the only property that makes the speed worth having.
//!
//! # What would break, and how this would see it
//!
//! A batched matrix product indexes weights by row and activations by lane. Get
//! the two confused and lane 1 attends over lane 0's context, which does not
//! crash and does not slow down — it answers the wrong question, fluently. Four
//! questions with four different one-word answers is what makes that visible:
//! if the lanes are crossed, the answers are permuted.
//!
//! ```text
//! cargo run --release -p hv2-infer --example batched -- <model.gguf>
//! ```

use std::time::Instant;

use hv2_infer::schedule::default_threads;
use hv2_infer::{ask, ask_many, Model, Runner, Session};

/// Questions with answers that are not matters of opinion, and are all
/// different from each other — so that a batch which crossed its lanes would
/// produce visibly the wrong ones rather than plausibly the right ones.
const QUESTIONS: [(&str, &str); 4] = [
    (
        "What is the capital of France? Answer in one word.",
        "Paris",
    ),
    (
        "What colour is the sky on a clear day? Answer in one word.",
        "Blue",
    ),
    ("What is the capital of Japan? Answer in one word.", "Tokyo"),
    (
        "How many legs does a spider have? Answer with a number.",
        "8",
    ),
];

/// Tokens per answer. Short, because four questions are asked twice.
const LIMIT: usize = 16;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: batched <model.gguf>");
        return std::process::ExitCode::FAILURE;
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(default_threads(1))
        .build()
        .expect("a thread pool");

    let model = match Model::load(path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let lanes = QUESTIONS.len();
    println!(
        "model         : {:.0} MiB of weights, {} threads, {lanes} lanes",
        model.weight_bytes() as f64 / (1024.0 * 1024.0),
        pool.current_num_threads()
    );

    // ── one at a time ───────────────────────────────────────────────────
    let mut alone = Vec::with_capacity(lanes);
    let started = Instant::now();
    for (question, _) in QUESTIONS {
        let mut runner = Runner::single(&model);
        let mut session = Session::open(&model);
        match pool.install(|| ask(&mut runner, &mut session, question, LIMIT)) {
            Ok(answer) => alone.push(answer.trim().to_string()),
            Err(e) => {
                eprintln!("alone         : FAILED — {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let one_at_a_time = started.elapsed();
    println!(
        "one at a time : {:.1} s for {lanes} answers",
        one_at_a_time.as_secs_f64()
    );

    // ── all at once ─────────────────────────────────────────────────────
    let mut runner = Runner::new(&model, lanes);
    let mut owned: Vec<Session> = (0..lanes).map(|_| Session::open(&model)).collect();
    let mut sessions: Vec<&mut Session> = owned.iter_mut().collect();
    let questions: Vec<&str> = QUESTIONS.iter().map(|(q, _)| *q).collect();

    let started = Instant::now();
    let together = match pool.install(|| ask_many(&mut runner, &mut sessions, &questions, LIMIT)) {
        Ok(answers) => answers,
        Err(e) => {
            eprintln!("batched       : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let batched = started.elapsed();
    println!(
        "all at once   : {:.1} s for the same {lanes} answers — {:.2}x",
        batched.as_secs_f64(),
        one_at_a_time.as_secs_f64() / batched.as_secs_f64()
    );
    println!(
        "scratch       : {:.1} MiB in the runner, held once rather than per agent",
        runner.scratch_bytes() as f64 / (1024.0 * 1024.0)
    );
    println!();

    // ── the answers have to be the same ─────────────────────────────────
    let mut ok = true;
    for (lane, (question, expected)) in QUESTIONS.iter().enumerate() {
        let solo = alone[lane].trim();
        let batch = together[lane].trim();
        let right = batch.contains(expected);
        let same = solo == batch;
        println!(
            "lane {lane}        : {:?} -> alone {solo:?}, batched {batch:?}",
            question.split('?').next().unwrap_or(question)
        );
        if !right {
            println!("                FAILED — the batched answer is not {expected:?}");
            ok = false;
        }
        if !same {
            println!("                FAILED — batching changed the answer");
            ok = false;
        }
    }

    // Lanes crossed in the batched product is the specific failure this example
    // exists to catch, and it is worth naming rather than reporting as "an
    // answer changed": an answer that belongs to a different lane says the
    // batch mixed conversations, which is a different bug from a model being
    // wrong.
    let crossed = together.iter().enumerate().any(|(lane, answer)| {
        !answer.trim().is_empty()
            && alone
                .iter()
                .position(|a| a.trim() == answer.trim())
                .is_some_and(|found| found != lane)
    });

    println!();
    if ok {
        println!(
            "result        : four conversations advanced by one pass over the weights each step, \
             and every lane answered its own question. The speed is worth having because the \
             answers did not change."
        );
        std::process::ExitCode::SUCCESS
    } else {
        if crossed {
            println!(
                "result        : FAILED — an answer belonging to one lane came back on another. \
                 That is lanes crossed in the batched product, not a model being wrong."
            );
        }
        std::process::ExitCode::FAILURE
    }
}
