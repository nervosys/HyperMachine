#!/usr/bin/env bash
# Workload: sysbench memory bandwidth (read + write, 1M block).
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

DURATION="${DURATION:-20}"

run_one() {
    local op="$1"
    ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
        "sysbench memory --memory-oper=$op --memory-block-size=1M --memory-total-size=10G --time=$DURATION run 2>/dev/null" \
        | awk '/transferred/ {for(i=1;i<=NF;i++) if($i ~ /MiB\/sec\)$/){gsub(/[()]/,"",$(i-1)); print $(i-1); exit}}'
}

r=$(run_one read)
w=$(run_one write)
printf "mem_read_MiBps\t%s\tMiB/s\n" "$r"
printf "mem_write_MiBps\t%s\tMiB/s\n" "$w"
