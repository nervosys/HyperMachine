//! How fast this machine can read the model at all.
//!
//! `throughput` says a forward pass stops getting faster at about a third of
//! this host's cores and then gets worse, and the code deliberately does not
//! assert why. This is the experiment that narrows it: the same bytes, the same
//! thread counts, and *no transformer* — just a sum over every byte of every
//! tensor.
//!
//! If pure reading knees in the same place, the limit is the machine or the way
//! the work is handed to threads, and no amount of rearranging the arithmetic
//! will move it. If pure reading keeps scaling where the forward pass stops,
//! the limit is in the forward pass and is worth chasing.
//!
//! It is a floor rather than a model of the pass: summing bytes touches each
//! cache line once and does almost nothing with it, so it is the friendliest
//! possible read of the same memory.
//!
//! ```text
//! cargo run --release -p hv2-infer --example bandwidth -- <model.gguf>
//! ```

use std::time::Instant;

use rayon::prelude::*;

use hv2_infer::Gguf;

/// Thread counts to sweep.
const SWEEP: [usize; 8] = [1, 2, 4, 6, 8, 12, 16, 24];

/// Passes per thread count, so a median means something on a host whose load
/// moves.
const RUNS: usize = 3;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: bandwidth <model.gguf>");
        return std::process::ExitCode::FAILURE;
    };

    // The header is read for the tensor table; the bytes below are the tensor
    // data itself, which is what a forward pass actually streams.
    let gguf = match Gguf::open(&path) {
        Ok(gguf) => gguf,
        Err(e) => {
            eprintln!("model         : FAILED — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut blocks: Vec<&[u8]> = Vec::new();
    for info in gguf.tensors.values() {
        match gguf.bytes(info) {
            Ok(bytes) => blocks.push(bytes),
            Err(e) => {
                eprintln!("tensor        : FAILED — {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let total: usize = blocks.iter().map(|b| b.len()).sum();
    println!(
        "model         : {:.0} MiB of tensor data across {} tensors",
        total as f64 / (1024.0 * 1024.0),
        blocks.len()
    );
    println!(
        "machine       : {} logical CPUs",
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    println!();

    // Fault it all in once, so the sweep measures reading rather than mapping.
    let warm: u64 = blocks
        .iter()
        .map(|b| {
            b.iter()
                .fold(0u64, |acc, byte| acc.wrapping_add(u64::from(*byte)))
        })
        .sum();
    std::hint::black_box(warm);

    let mut best = (0usize, 0.0f64);
    for threads in SWEEP {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("a thread pool");
        let mut rates = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let started = Instant::now();
            let sum: u64 = pool.install(|| {
                blocks
                    .par_iter()
                    .map(|block| {
                        block
                            .iter()
                            .fold(0u64, |acc, byte| acc.wrapping_add(u64::from(*byte)))
                    })
                    .sum()
            });
            // Kept so the loop cannot be optimised away.
            std::hint::black_box(sum);
            let elapsed = started.elapsed().as_secs_f64();
            rates.push(total as f64 / elapsed / (1024.0 * 1024.0 * 1024.0));
        }
        let rate = median(rates);
        if rate > best.1 {
            best = (threads, rate);
        }
        println!("threads {threads:<3}   : {rate:>6.2} GiB/s");
    }

    println!();
    println!("fastest       : {} threads at {:.2} GiB/s", best.0, best.1);

    // The comparison this example exists for.
    println!(
        "read this      : against a forward pass, which streams the same bytes and also does the \
         arithmetic. If those two numbers are close, the pass is near what the machine can read \
         and batching is the only lever left; if plain reading is much faster, the pass is \
         leaving something on the table."
    );
    std::process::ExitCode::SUCCESS
}
