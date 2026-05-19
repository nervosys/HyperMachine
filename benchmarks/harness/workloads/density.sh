#!/usr/bin/env bash
# Workload: density. Spawn N idle VMs, wait for settle, sample RSS per VM.
# Special-cased by runner: invoked once per hypervisor (not per VM).
# Env: HV, COUNT, SETTLE_S, BENCH_OUT_DIR
set -euo pipefail
source "$(dirname "$0")/../lib/common.sh"

HV="${HV:?HV required}"
COUNT="${COUNT:-32}"
SETTLE_S="${SETTLE_S:-30}"
IMAGE="${BENCH_GUEST_IMAGE:?BENCH_GUEST_IMAGE required}"
CPUS="${BENCH_GUEST_CPUS:-1}"
MEM="${BENCH_GUEST_MEM_MIB:-256}"
NETIF="${BENCH_GUEST_NETIF:-user}"

declare -a vms=()
cleanup() {
    for v in "${vms[@]}"; do
        hv "$HV" stop "$v"    >/dev/null 2>&1 || true
        hv "$HV" destroy "$v" >/dev/null 2>&1 || true
    done
}
trap cleanup EXIT

for i in $(seq 1 "$COUNT"); do
    log_info "density: setup VM $i/$COUNT"
    id=$(hv "$HV" setup "$IMAGE" "$CPUS" "$MEM" "" "$NETIF")
    vms+=("$id")
    hv "$HV" start "$id" >/dev/null
done

log_info "density: settling ${SETTLE_S}s"
sleep "$SETTLE_S"

total_rss=0
for v in "${vms[@]}"; do
    m=$(hv "$HV" metrics "$v")
    rss=$(echo "$m" | awk -F'[=,]' '{for(i=1;i<=NF;i++) if($i=="rss_kib") print $(i+1)}')
    total_rss=$((total_rss + rss))
done

avg_mib=$(python3 -c "print(f'{$total_rss/$COUNT/1024:.1f}')")
printf "density_vms_started\t%s\tcount\n" "$COUNT"
printf "density_avg_rss_MiB\t%s\tMiB\n"   "$avg_mib"
printf "density_total_rss_MiB\t%s\tMiB\n" "$(python3 -c "print(f'{$total_rss/1024:.1f}')")"
