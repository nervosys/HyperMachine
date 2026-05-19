#!/usr/bin/env bash
# Workload: sysbench CPU.
# Env: BENCH_SSH_{USER,KEY,HOST,PORT}, THREADS, DURATION.
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

THREADS="${THREADS:-1}"
DURATION="${DURATION:-30}"

out=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "sysbench cpu --threads=$THREADS --time=$DURATION run 2>/dev/null")

eps=$(echo "$out" | awk -F: '/events per second/ {gsub(/ /,"",$2); print $2}')
lat=$(echo "$out" | awk '/avg:/ {print $2; exit}')
printf "cpu_events_per_sec\t%s\tevt/s\n" "$eps"
printf "cpu_latency_avg_ms\t%s\tms\n"   "$lat"
