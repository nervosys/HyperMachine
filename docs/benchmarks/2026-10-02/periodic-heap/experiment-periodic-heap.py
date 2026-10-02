#!/usr/bin/env python3
"""Generate an isolated, default-disabled periodic glibc reclaim candidate."""
import argparse
import difflib
import hashlib
from pathlib import Path

BASE_SHA256 = "e1b85b8a455a9afad0452113739a089f202b67787f419fc0d76a60a172334d5c"
WORKER = r'''
// Isolated experiment only. No deployed daemon or defaults use this worker.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
struct HeapReclaimExperiment {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
impl HeapReclaimExperiment {
    fn start(interval: Duration) -> std::io::Result<Self> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("heap-reclaim-experiment".into())
            .spawn(move || {
                let mut calls = 0u64;
                let mut released = 0u64;
                let mut total_ns = 0u128;
                let mut maximum_ns = 0u128;
                loop {
                    std::thread::park_timeout(interval);
                    if stopped.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    let started = std::time::Instant::now();
                    // SAFETY: GNU malloc_trim takes only a padding value, acts
                    // on free allocator chunks, and is thread-safe. This runs
                    // on a normal dedicated thread, never a signal handler.
                    let result = unsafe { libc::malloc_trim(0) };
                    let duration_ns = started.elapsed().as_nanos();
                    calls += 1;
                    released += u64::from(result != 0);
                    total_ns += duration_ns;
                    maximum_ns = maximum_ns.max(duration_ns);
                }
                eprintln!("HV2_HEAP_RECLAIM_EXPERIMENT {{\"calls\":{calls},\"release_calls\":{released},\"total_ns\":{total_ns},\"maximum_ns\":{maximum_ns}}}");
            })?;
        Ok(Self { stop, thread: Some(thread) })
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
impl Drop for HeapReclaimExperiment {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
struct HeapReclaimExperiment;

'''
START = r'''    // Only the isolated experimental fixture sets this variable.
    let _heap_reclaim_experiment: Option<HeapReclaimExperiment> = match std::env::var("HV2_EXPERIMENT_HEAP_RECLAIM_MS") {
        Ok(value) => {
            let interval_ms: u64 = match value.parse() {
                Ok(value) if (100..=60000).contains(&value) => value,
                _ => {
                    eprintln!("heap experiment requires 100..60000 milliseconds");
                    return std::process::ExitCode::FAILURE;
                }
            };
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            {
                match HeapReclaimExperiment::start(Duration::from_millis(interval_ms)) {
                    Ok(worker) => Some(worker),
                    Err(error) => {
                        eprintln!("heap experiment worker: {error}");
                        return std::process::ExitCode::FAILURE;
                    }
                }
            }
            #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
            {
                let _ = interval_ms;
                eprintln!("heap experiment requires Linux GNU libc");
                return std::process::ExitCode::FAILURE;
            }
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => {
            eprintln!("heap experiment interval is not Unicode");
            return std::process::ExitCode::FAILURE;
        }
    };
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-main", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; retain previous candidate")
    raw = args.baseline_main.read_bytes()
    if hashlib.sha256(raw).hexdigest() != BASE_SHA256: parser.error("baseline main identity mismatch")
    source = raw.decode().replace("\r\n", "\n")
    for anchor in ["#[tokio::main]\nasync fn main()", "    let port = opts.port;\n"]:
        if source.count(anchor) != 1: parser.error("ambiguous candidate anchor")
    candidate = source.replace("#[tokio::main]\nasync fn main()", WORKER + "#[tokio::main]\nasync fn main()")
    candidate = candidate.replace("    let port = opts.port;\n", START + "    let port = opts.port;\n")
    args.output.write_text(candidate, newline="\n")
    args.output.with_suffix(".patch").write_text("".join(difflib.unified_diff(source.splitlines(True), candidate.splitlines(True), fromfile="baseline-main.rs", tofile="candidate-main.rs")), newline="\n")


if __name__ == "__main__":
    main()
