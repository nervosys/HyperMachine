#!/usr/bin/env python3
"""Generate a reclaim candidate guarded by the entire cold-start budget."""
import argparse
import difflib
import hashlib
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("periodic", Path(__file__).with_name("experiment-periodic-heap.py"))
periodic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(periodic)


def replace(source, anchor, value):
    if source.count(anchor) != 1: raise ValueError("ambiguous anchor: " + anchor)
    return source.replace(anchor, value)


CONFIG = r'''    let heap_reclaim_interval: Option<Duration> = match std::env::var("HV2_EXPERIMENT_HEAP_RECLAIM_MS") {
        Ok(value) => {
            let interval_ms: u64 = match value.parse() {
                Ok(value) if (100..=60000).contains(&value) => value,
                _ => {
                    eprintln!("heap experiment requires 100..60000 milliseconds");
                    return std::process::ExitCode::FAILURE;
                }
            };
            if opts.cold_start_concurrency.is_none() {
                eprintln!("guarded heap experiment requires a cold-start budget");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
            {
                let _ = interval_ms;
                eprintln!("heap experiment requires Linux GNU libc");
                return std::process::ExitCode::FAILURE;
            }
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            Some(Duration::from_millis(interval_ms))
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => {
            eprintln!("heap experiment interval is not Unicode");
            return std::process::ExitCode::FAILURE;
        }
    };
'''
START = r'''    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    let _heap_reclaim_experiment = match (heap_reclaim_interval, &cold_boot_slots, opts.cold_start_concurrency) {
        (Some(interval), Some(slots), Some(budget)) => {
            match HeapReclaimExperiment::start(interval, Arc::clone(slots), budget as u32) {
                Ok(worker) => Some(worker),
                Err(error) => {
                    eprintln!("heap experiment worker: {error}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
        _ => None,
    };
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = heap_reclaim_interval;
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-main", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output exists; retain earlier candidates")
    raw = args.baseline_main.read_bytes()
    if hashlib.sha256(raw).hexdigest() != periodic.BASE_SHA256: parser.error("baseline identity mismatch")
    source = raw.decode().replace("\r\n", "\n")
    worker = replace(periodic.WORKER, "fn start(interval: Duration)", "fn start(interval: Duration, slots: Arc<tokio::sync::Semaphore>, budget: u32)")
    worker = replace(worker, "                let mut calls = 0u64;", "                let mut busy_skips = 0u64;\n                let mut calls = 0u64;")
    worker = replace(worker, "                    let started = std::time::Instant::now();", r'''                    let Ok(_idle_budget) = Arc::clone(&slots).try_acquire_many_owned(budget) else {
                        busy_skips += 1;
                        continue;
                    };
                    tracing::debug!(target: "hv2_sandboxd::heap_admission", "heap reclamation admitted");
                    let started = std::time::Instant::now();''')
    worker = replace(worker, "                    calls += 1;", r'''                    tracing::debug!(target: "hv2_sandboxd::heap_admission", "heap reclamation released");
                    calls += 1;''')
    escaped_quote = chr(92) + chr(34)
    before = "{{" + escaped_quote + "calls" + escaped_quote + ":{calls}"
    after = "{{" + escaped_quote + "busy_skips" + escaped_quote + ":{busy_skips}," + escaped_quote + "calls" + escaped_quote + ":{calls}"
    worker = replace(worker, before, after)
    candidate = replace(source, "#[tokio::main]\nasync fn main()", worker + "#[tokio::main]\nasync fn main()")
    candidate = replace(candidate, "    let port = opts.port;\n", CONFIG + "    let port = opts.port;\n")
    anchor = "    let cold_boot_slots = opts\n        .cold_start_concurrency\n        .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit)));\n"
    candidate = replace(candidate, anchor, anchor + START)
    args.output.write_text(candidate, newline="\n")
    args.output.with_suffix(".patch").write_text("".join(difflib.unified_diff(source.splitlines(True), candidate.splitlines(True), fromfile="baseline-main.rs", tofile="guarded-main.rs")), newline="\n")


if __name__ == "__main__":
    main()
