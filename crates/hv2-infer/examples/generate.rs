//! A model answers a question, on this machine, from real weights.
//!
//! This is the smallest thing that closes the largest gap on the project's
//! handoff: "no model has ever run inside any of this". It maps a GGUF file,
//! reads the shape and the vocabulary out of it, runs a forward pass per token
//! and prints what came back — with the cost of each part, because a number
//! nobody measured is the thing this repository keeps finding defects in.
//!
//! ```text
//! cargo run --release -p hv2-infer --example generate -- <model.gguf> ["a question"]
//! ```
//!
//! Needs a Llama-architecture GGUF quantised to `Q8_0` or stored as floats.
//! Nothing else: no GPU, no BLAS, no network.
//!
//! # What it asserts
//!
//! That the answer is not noise. A transformer with a defect anywhere in it —
//! a mis-sliced head, the wrong rotation convention, a scale read at the wrong
//! offset — still runs at full speed and still produces tokens; what it stops
//! producing is *language*. So the check is that the model answers a question
//! with a known answer, and the exit status says whether it did.

use std::time::Instant;

use hv2_infer::{generate, Model, Session};

/// A question whose answer is not a matter of opinion, so that "it worked" is
/// checkable rather than a judgement about prose.
const QUESTION: &str = "What is the capital of France? Answer in one word.";

/// What has to appear in the answer for this to have worked.
const EXPECTED: &str = "Paris";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: generate <model.gguf> [question]");
        eprintln!();
        eprintln!("Any Llama-architecture GGUF in Q8_0, F16 or F32.");
        return std::process::ExitCode::FAILURE;
    };
    let question = args.get(1).map_or(QUESTION, String::as_str);
    let checking = args.len() < 2;

    let started = Instant::now();
    let model = match Model::load(path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let loaded = started.elapsed();

    let s = &model.shape;
    println!(
        "model         : {} — {} layers, width {}, {} heads over {} kv heads, vocab {}",
        path, s.layers, s.width, s.heads, s.kv_heads, s.vocab
    );
    println!(
        "mapped        : {:.0} MiB in {:.1} ms — the header and the vocabulary were read, the \
         weights were not",
        model.mapped_bytes() as f64 / (1024.0 * 1024.0),
        loaded.as_secs_f64() * 1000.0
    );
    println!(
        "rotation      : {} — chosen by running both and keeping the confident one",
        model.rope.as_str()
    );
    println!(
        "kv cache      : {} bytes per token, which is what an agent cannot share with another \
         agent",
        model.cache_bytes_per_token()
    );
    println!();
    println!("question      : {question}");

    let mut session = Session::open(&model);
    let asked = Instant::now();
    let answer = match generate(&model, &mut session, question, 32) {
        Ok(answer) => answer,
        Err(e) => {
            eprintln!("generate      : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let elapsed = asked.elapsed();

    println!("answer        : {}", answer.trim());
    println!();
    println!(
        "tokens        : {} in the conversation, {:.2} s — {:.1} ms per token",
        session.len(),
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / session.len().max(1) as f64
    );
    println!(
        "cache         : {:.2} MiB held for this one conversation",
        session.cache_bytes() as f64 / (1024.0 * 1024.0)
    );

    if !checking {
        return std::process::ExitCode::SUCCESS;
    }

    println!();
    if answer.contains(EXPECTED) {
        println!(
            "result        : real weights, a real tokeniser and a real forward pass, and the \
             answer is {EXPECTED}. Everything this project has measured about inference was the \
             floor under this."
        );
        std::process::ExitCode::SUCCESS
    } else {
        println!(
            "result        : FAILED — the model produced tokens and not the answer. A \
             transformer with a defect in it runs at full speed and stops producing language, \
             which is what this is checking for."
        );
        std::process::ExitCode::FAILURE
    }
}
