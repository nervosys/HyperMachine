#!/usr/bin/env bash
# Workload: Linux kernel build (`make defconfig && time make -j$jobs`).
# Env: KERNEL_URL, MAKE_JOBS
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

KERNEL_URL="${KERNEL_URL:?KERNEL_URL required}"
JOBS="${MAKE_JOBS:-0}"

# Determine jobs inside guest if 0.
if [[ "$JOBS" == "0" ]]; then
    JOBS=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "nproc")
fi

# Idempotent: skip download if already present (image cache).
ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "
    set -e
    mkdir -p ~/kbench && cd ~/kbench
    fname=\$(basename '$KERNEL_URL')
    [ -f \$fname ] || curl -sLO '$KERNEL_URL'
    dname=\$(tar -tf \$fname | head -1 | cut -d/ -f1)
    [ -d \$dname ] || tar -xf \$fname
    cd \$dname
    make mrproper >/dev/null 2>&1 || true
    make defconfig >/dev/null
" >/dev/null

# Time the build.
start=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "date +%s.%N")
ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "
    cd ~/kbench/\$(ls ~/kbench | grep ^linux- | head -1)
    make -s -j$JOBS >/dev/null
" >/dev/null
end=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" "date +%s.%N")

elapsed=$(python3 -c "print(f'{$end - $start:.3f}')")
printf "kernel_build_seconds\t%s\ts\n" "$elapsed"
printf "kernel_build_jobs\t%s\tcount\n" "$JOBS"
