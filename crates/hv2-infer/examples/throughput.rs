//! How fast a forward pass is, and what that is a rate *of*.
//!
//! `generate` reports milliseconds per token, which is the number a user feels
//! and a poor number to optimise against: it moves with the length of the
//! answer, with tokenisation, and with whichever question was asked. This does
//! a fixed amount of work — a set number of forward passes over the same
//! context — and reports the rate.
//!
//! The rate is expressed as bytes of weights per second as well as tokens per
//! second, because that is what the cost actually is: a forward pass reads
//! every parameter exactly once, and at `Q8_0` that is a byte and a bit per
//! parameter.
//!
//! What limits that rate is *not* settled, and this example exists partly to
//! stop it being asserted. It scales nearly linearly to about a third of this
//! machine's cores and then gets worse, and widening the arithmetic to AVX2
//! buys about 1.3x — so it is neither purely memory-bound (or the arithmetic
//! would not matter) nor purely compute-bound (or more cores would keep
//! helping). Both knobs are on the command line so the next person can find
//! their own machine's answer rather than inherit this one's.
//!
//! ```text
//! cargo run --release -p hv2-infer --example throughput -- <model.gguf> [passes]
//! ```

use std::time::Instant;

use hv2_infer::{Model, Runner, Session};

/// Forward passes to time. The first is excluded: it faults in the mapping,
/// which is a cost paid once for the life of the process and not per token.
const DEFAULT_PASSES: usize = 8;

/// Runs to take, so that a median means something.
///
/// This host is shared and its load moves: the same build measured 503, 709 and
/// 1745 ms per pass within a few minutes. One run of this benchmark measures
/// what else the machine was doing, exactly as one boot did.
const RUNS: usize = 5;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: throughput <model.gguf> [passes] [threads] [lanes]");
        eprintln!();
        eprintln!("Threads defaults to what the scheduler would choose. Sweeping it is how the");
        eprintln!("knee below was found, and how it should be re-found on another machine.");
        return std::process::ExitCode::FAILURE;
    };
    let passes = args
        .get(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(DEFAULT_PASSES);

    // Conversations carried by one pass. The whole question this example
    // exists to answer: a pass reads the entire model to produce a token, so
    // does producing eight tokens cost eight passes or one?
    let lanes = args.get(3).and_then(|n| n.parse().ok()).unwrap_or(1usize);

    // Zero means "choose", the same as `Limits::threads` — otherwise it would
    // ask rayon for a pool of no threads, which it answers by using all of
    // them, which is the one setting this example exists to argue against.
    let threads = match args.get(2).and_then(|n| n.parse::<usize>().ok()) {
        Some(0) | None => hv2_infer::schedule::default_threads(lanes),
        Some(n) => n,
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("a thread pool");

    let model = match Model::load(path) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // The weights, in bytes, as stored. Every one is read once per pass.
    let bytes = model.weight_bytes();
    println!(
        "model         : {:.0} MiB of weights, {} layers, width {}, vocab {}",
        bytes as f64 / (1024.0 * 1024.0),
        model.shape.layers,
        model.shape.width,
        model.shape.vocab
    );
    println!(
        "threads       : {threads} of {} the machine reports",
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    println!(
        "target        : built for {}",
        if cfg!(target_feature = "avx2") {
            "AVX2"
        } else {
            "the x86-64 baseline (SSE2)"
        }
    );

    let mut runner = Runner::new(&model, lanes);
    let mut sessions: Vec<Session> = (0..lanes).map(|_| Session::open(&model)).collect();
    let token = model.bos.unwrap_or(1);
    println!(
        "lanes         : {lanes} conversation(s) per pass, {:.1} MiB of runner scratch",
        runner.scratch_bytes() as f64 / (1024.0 * 1024.0)
    );

    // One pass to fault the mapping in, then the timed ones. All of them inside
    // the pool, so the thread count above is the one that is measured.
    let mut work: Vec<(&mut Session, u32, usize)> = sessions
        .iter_mut()
        .map(|session| (session, token, 0usize))
        .collect();
    if let Err(e) = pool.install(|| runner.step(&mut work)) {
        eprintln!("warm-up       : FAILED — {e}");
        return std::process::ExitCode::FAILURE;
    }
    drop(work);

    let mut per_pass = Vec::with_capacity(RUNS);
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..passes {
            // Position 1 every time: the same work, over a cache holding one
            // token. A growing context would make later passes cost more and
            // turn the rate into an average over a ramp.
            let mut work: Vec<(&mut Session, u32, usize)> = sessions
                .iter_mut()
                .map(|session| (session, token, 1usize))
                .collect();
            if let Err(e) = pool.install(|| runner.step(&mut work)) {
                eprintln!("pass          : FAILED — {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
        let each = started.elapsed().as_secs_f64() / passes as f64;
        per_pass.push(each);
        println!(
            "run {:<10}: {passes} passes, {:.1} ms each, {:.2} GiB/s, {:.2} tokens/s",
            run + 1,
            each * 1000.0,
            bytes as f64 / each / (1024.0 * 1024.0 * 1024.0),
            lanes as f64 / each
        );
    }

    let low = per_pass.iter().copied().fold(f64::INFINITY, f64::min);
    let high = per_pass.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mid = median(per_pass);
    println!();
    println!(
        "per pass      : {:.1} ms median   ({:.1} to {:.1})",
        mid * 1000.0,
        low * 1000.0,
        high * 1000.0
    );
    println!(
        "tokens        : {:.2} per second across {lanes} lane(s) — {:.1} ms per token",
        lanes as f64 / mid,
        mid * 1000.0 / lanes as f64
    );
    println!(
        "weight rate   : {:.2} GiB/s at the median, {:.2} at the best run. Every parameter is read once per pass, so this is what a pass costs.",
        bytes as f64 / mid / (1024.0 * 1024.0 * 1024.0),
        bytes as f64 / low / (1024.0 * 1024.0 * 1024.0)
    );
    std::process::ExitCode::SUCCESS
}
