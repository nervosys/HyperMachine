#!/usr/bin/env bash
# benchmarks/harness/lib/common.sh
#
# Shared helpers sourced by run.sh, adapters, and workloads.
# Adapter ABI:
#   hv_setup    image cpus mem_mib disk_path netif  -> echoes "vm_id"
#   hv_start    vm_id                                -> echoes pid; async
#   hv_wait_ssh vm_id ssh_port timeout_s             -> 0 on ready, 124 on timeout
#   hv_snapshot vm_id name                           -> 0 ok, 78 if unsupported
#   hv_restore  vm_id name                           -> 0 ok, 78 if unsupported
#   hv_stop     vm_id                                -> 0
#   hv_destroy  vm_id                                -> 0
#   hv_metrics  vm_id                                -> echoes "rss_kib=...,cpu_pct=..."

set -euo pipefail

# Resolved at source-time.
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$(cd "$HARNESS_DIR/.." && pwd)"
ADAPTERS_DIR="$HARNESS_DIR/adapters"
WORKLOADS_DIR="$HARNESS_DIR/workloads"

# ----- logging -----
_log() { printf '[%s] %s %s\n' "$(date +%H:%M:%S)" "$1" "$2" >&2; }
log_info()  { _log "INFO" "$*"; }
log_warn()  { _log "WARN" "$*"; }
log_error() { _log "ERR " "$*"; }
die()       { log_error "$*"; exit 1; }

# ----- TOML reader (no python required for trivial cases). Falls back to python3.
toml_get() {
    # toml_get <file> <dotted.key>  -> stdout value (string/number/bool)
    python3 - "$1" "$2" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    d = tomllib.load(f)
cur = d
for part in sys.argv[2].split("."):
    if part in cur:
        cur = cur[part]
    else:
        sys.exit(0)
if isinstance(cur, list):
    print(" ".join(str(x) for x in cur))
elif isinstance(cur, bool):
    print("true" if cur else "false")
else:
    print(cur)
PY
}

toml_enabled_keys() {
    # toml_enabled_keys <file> <table>  -> echoes enabled sub-keys, newline-sep
    python3 - "$1" "$2" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    d = tomllib.load(f)
section = d.get(sys.argv[2], {})
for k, v in section.items():
    if isinstance(v, dict) and v.get("enabled", False):
        print(k)
PY
}

# ----- adapter dispatch -----
hv() {
    # hv <hv_name> <verb> [args...]
    local hv_name="$1"; shift
    local verb="$1"; shift
    local script="$ADAPTERS_DIR/${hv_name}.sh"
    [[ -f "$script" ]] || die "no adapter for hypervisor '$hv_name' (expected $script)"
    HV_NAME="$hv_name" bash "$script" "$verb" "$@"
}

# ----- env capture -----
capture_env() {
    # capture_env <out_dir>
    local out="$1/env.json"
    mkdir -p "$1"
    python3 - "$out" <<'PY'
import json, os, platform, shutil, subprocess, sys
def run(cmd):
    try: return subprocess.check_output(cmd, shell=True, text=True, timeout=5).strip()
    except Exception as e: return f"<error: {e}>"
env = {
    "uname":   platform.platform(),
    "python":  sys.version.split()[0],
    "cpu":     run("lscpu 2>/dev/null | head -25 || sysctl -n machdep.cpu.brand_string 2>/dev/null || wmic cpu get name /value 2>nul"),
    "memtotal_kib": run("awk '/^MemTotal/{print $2}' /proc/meminfo 2>/dev/null || echo 0"),
    "kernel":  run("uname -a"),
    "tools": {},
}
for tool in ("qemu-system-x86_64","firecracker","cloud-hypervisor","VBoxManage","hm-cli","sysbench","fio","iperf3","netperf"):
    if shutil.which(tool):
        env["tools"][tool] = run(f"{tool} --version 2>&1 | head -1")
    else:
        env["tools"][tool] = None
with open(sys.argv[1], "w") as f: json.dump(env, f, indent=2)
PY
    log_info "wrote $out"
}

# ----- statistics -----
summarize_csv() {
    # summarize_csv <input_csv_with_single_value_per_line> <out_csv>
    python3 - "$1" "$2" <<'PY'
import csv, statistics, sys
xs = [float(line.strip()) for line in open(sys.argv[1]) if line.strip()]
if not xs:
    sys.exit(0)
xs.sort()
def pct(p): return xs[max(0, min(len(xs)-1, int(round(p/100*(len(xs)-1)))))]
with open(sys.argv[2], "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["n","min","p50","p95","p99","max","mean","stdev"])
    w.writerow([
        len(xs), xs[0], pct(50), pct(95), pct(99), xs[-1],
        statistics.fmean(xs),
        statistics.pstdev(xs) if len(xs) > 1 else 0.0,
    ])
PY
}

# ----- host prep -----
host_prep() {
    if command -v cpupower >/dev/null 2>&1; then
        sudo cpupower frequency-set -g performance >/dev/null 2>&1 || log_warn "cpupower failed; not root?"
    fi
    # Disable transparent huge pages randomness (optional).
    if [[ -w /sys/kernel/mm/transparent_hugepage/enabled ]]; then
        echo always > /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null || true
    fi
}

host_restore() {
    # Best-effort restore; ignore failures.
    if command -v cpupower >/dev/null 2>&1; then
        sudo cpupower frequency-set -g schedutil >/dev/null 2>&1 || true
    fi
}

# ----- run id helpers -----
new_run_id() { date -u +'%Y%m%dT%H%M%SZ'; }
