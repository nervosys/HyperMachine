#!/usr/bin/env python3
"""Generate an isolated cold-boot admission candidate from the verified daemon source."""
import argparse
import difflib
import hashlib
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline-main", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
if args.output.exists(): parser.error("output exists; preserve earlier candidates")
raw = args.baseline_main.read_bytes()
if hashlib.sha256(raw).hexdigest() != "92697b46eb245dbc4c7677717e5fcb8d4b2f33b94e5806866fdf38964c9f6f08":
    parser.error("requires the verified registration-mutation daemon source")
original = raw.decode()
text = original


def replace(old, new):
    global text
    if text.count(old) != 1:
        raise ValueError("candidate anchor missing or ambiguous: " + old[:60])
    text = text.replace(old, new)


# The frozen source uses platform-native line endings. Edit normalized text,
# then preserve them on output so the only changes are the intended candidate.
newline = "\r\n" if "\r\n" in original else "\n"
text = text.replace("\r\n", "\n")
replace("    ready_timeout: Duration,\n", "    ready_timeout: Duration,\n    cold_start_concurrency: Option<usize>,\n")
replace("        ready_timeout: Duration::from_secs(15),\n", "        ready_timeout: Duration::from_secs(15),\n        cold_start_concurrency: None,\n")
replace('            "--capacity" => opts.capacity = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,', '''            "--cold-start-concurrency" => {
                let limit: usize = value(&mut i)?.parse().map_err(|_| "--cold-start-concurrency requires 1..1024")?;
                if !(1..=1024).contains(&limit) {
                    return Err("--cold-start-concurrency requires 1..1024".into());
                }
                opts.cold_start_concurrency = Some(limit);
            }
            "--capacity" => opts.capacity = value(&mut i)?.parse().map_err(|e| format!("{e}"))?,''')
replace("[--capacity N] [--no-template", "[--capacity N] [--cold-start-concurrency N] [--no-template")
replace("    slots: Arc<tokio::sync::Semaphore>,\n", "    slots: Arc<tokio::sync::Semaphore>,\n    /// Optional cold-boot budget, held through agent readiness. Restores bypass it.\n    cold_boot_slots: Option<Arc<tokio::sync::Semaphore>>,\n")
replace("    let opts_capacity = opts.capacity as usize;\n", "    let opts_capacity = opts.capacity as usize;\n    let cold_boot_slots = opts.cold_start_concurrency.map(|limit| Arc::new(tokio::sync::Semaphore::new(limit)));\n")
replace("        slots: Arc::new(tokio::sync::Semaphore::new(opts_capacity)),\n", "        slots: Arc::new(tokio::sync::Semaphore::new(opts_capacity)),\n        cold_boot_slots,\n")
replace("/// Bring up a sandbox's VM: from `snapshot` when given, else from the\n", '''/// One cold-boot permit. Failures and cancellation release it through Drop.
struct ColdBootAdmission {
    _permit: tokio::sync::OwnedSemaphorePermit,
    sandbox_id: String,
}

impl Drop for ColdBootAdmission {
    fn drop(&mut self) {
        tracing::debug!(target: "hv2_sandboxd::cold_admission", vm = %self.sandbox_id,
            "cold boot admission released");
    }
}

/// Bring up a sandbox's VM: from `snapshot` when given, else from the
''')
replace("    let t0 = std::time::Instant::now();\n    let initrd = state", '''    let t0 = std::time::Instant::now();
    let cold_boot_permit = if snapshot.is_none() {
        match &state.cold_boot_slots {
            Some(slots) => {
                let permit = Arc::clone(slots).acquire_owned().await.map_err(|_| (
                    StatusCode::SERVICE_UNAVAILABLE, "cold boot admission unavailable".to_string()))?;
                tracing::debug!(target: "hv2_sandboxd::cold_admission", vm = sandbox_id,
                    queue_ms = t0.elapsed().as_secs_f64() * 1000.0, "cold boot admitted");
                Some(ColdBootAdmission { _permit: permit, sandbox_id: sandbox_id.to_owned() })
            }
            None => None,
        }
    } else { None };
    let initrd = state''')
replace("    let answered = t0.elapsed();\n", "    drop(cold_boot_permit);\n    let answered = t0.elapsed();\n")
result = text.replace("\n", newline)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_bytes(result.encode())
args.output.with_suffix(".patch").write_text("".join(difflib.unified_diff(
    original.splitlines(keepends=True), result.splitlines(keepends=True),
    fromfile="baseline/main.rs", tofile="candidate/main.rs")))
