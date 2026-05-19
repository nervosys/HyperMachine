#!/usr/bin/env bash
# Workload: mgmt latency. Measures wallclock for setup + start + ready + destroy.
# Special-cased by runner: invoked once per hypervisor.
# Env: HV, CYCLES
set -euo pipefail
source "$(dirname "$0")/../lib/common.sh"

HV="${HV:?HV required}"
CYCLES="${CYCLES:-20}"
IMAGE="${BENCH_GUEST_IMAGE:?BENCH_GUEST_IMAGE required}"
CPUS="${BENCH_GUEST_CPUS:-1}"
MEM="${BENCH_GUEST_MEM_MIB:-512}"
NETIF="${BENCH_GUEST_NETIF:-user}"

for i in $(seq 1 "$CYCLES"); do
    t0=$(date +%s.%N)
    id=$(hv "$HV" setup "$IMAGE" "$CPUS" "$MEM" "" "$NETIF")
    t1=$(date +%s.%N)
    hv "$HV" start "$id" >/dev/null
    t2=$(date +%s.%N)
    hv "$HV" stop "$id"    >/dev/null 2>&1 || true
    hv "$HV" destroy "$id" >/dev/null 2>&1 || true
    t3=$(date +%s.%N)
    python3 -c "
print(f'mgmt_setup_ms\t{($t1-$t0)*1000:.2f}\tms')
print(f'mgmt_start_ms\t{($t2-$t1)*1000:.2f}\tms')
print(f'mgmt_destroy_ms\t{($t3-$t2)*1000:.2f}\tms')
print(f'mgmt_total_ms\t{($t3-$t0)*1000:.2f}\tms')
"
done
