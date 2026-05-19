#!/usr/bin/env bash
# Workload: fio disk I/O.
# Env: PATTERN={randread|randwrite|read|write}, BS=4k|1M, IODEPTH=N,
#      SIZE_MIB=N, FSYNC={0|1}
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

PATTERN="${PATTERN:-randread}"
BS="${BS:-4k}"
IODEPTH="${IODEPTH:-32}"
SIZE_MIB="${SIZE_MIB:-1024}"
FSYNC="${FSYNC:-0}"
NAME="${NAME:-bench}"

cmd="fio --name=$NAME --filename=/tmp/fio.bench --rw=$PATTERN --bs=$BS \
     --iodepth=$IODEPTH --size=${SIZE_MIB}M --direct=1 --ioengine=libaio \
     --runtime=30 --time_based --group_reporting --output-format=json"
[[ "$FSYNC" == "1" ]] && cmd="$cmd --fsync=1"

json=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "$cmd")

python3 - "$PATTERN" "$BS" <<PY
import json, sys
pat, bs = sys.argv[1], sys.argv[2]
data = json.loads("""$json""")
job  = data["jobs"][0]
side = "read" if "read" in pat else "write"
iops = job[side]["iops"]
bw_kib = job[side]["bw"]  # KiB/s
clat = job[side]["clat_ns"]
print(f"disk_{pat}_{bs}_iops\t{iops:.1f}\tIOPS")
print(f"disk_{pat}_{bs}_bw_MiBps\t{bw_kib/1024:.2f}\tMiB/s")
print(f"disk_{pat}_{bs}_lat_p50_us\t{clat['percentile']['50.000000']/1000:.2f}\tus")
print(f"disk_{pat}_{bs}_lat_p99_us\t{clat['percentile']['99.000000']/1000:.2f}\tus")
PY
