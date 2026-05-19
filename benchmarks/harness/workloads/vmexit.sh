#!/usr/bin/env bash
# Workload: vmexit microbench (compile-and-run inside guest).
# Env: ITERATIONS
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

ITERATIONS="${ITERATIONS:-1000000}"
SRC="$(dirname "$0")/vmexit.c"

# Upload + compile once per invocation. (Image is expected to have gcc.)
scp_to "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "$SRC" /tmp/vmexit.c
ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "cc -O2 -o /tmp/vmexit /tmp/vmexit.c" >/dev/null
ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "/tmp/vmexit $ITERATIONS"
