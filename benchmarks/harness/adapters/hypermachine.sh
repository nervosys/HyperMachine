#!/usr/bin/env bash
# Adapter: HyperMachine via hm-cli. Mirrors the ABI of qemu_kvm.sh.
# NOTE: hm-cli surface area may evolve; commands below assume the subcommands
# `vm create`, `vm start`, `vm stop`, `vm destroy`, `vm info --json`. Adjust
# to actual CLI if it has diverged.
set -euo pipefail

VERB="${1:?verb required}"; shift || true
HM_BIN="${HM_BIN:-../target/release/hm-cli}"
STATE_ROOT="${BENCH_STATE_ROOT:-$(pwd)/.state}/hypermachine"
mkdir -p "$STATE_ROOT"

_vm_dir() { echo "$STATE_ROOT/$1"; }

case "$VERB" in
  setup)
    image="$1" cpus="$2" mem="$3" disk="$4" netif="$5"
    vm_id="hm-$(date +%s%N)-$$"
    d="$(_vm_dir "$vm_id")"; mkdir -p "$d"
    ssh_port=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
    # hm-cli is expected to manage its own disk overlay; we just pass the path.
    cat > "$d/meta" <<EOF
cpus=$cpus
mem=$mem
netif=$netif
ssh_port=$ssh_port
image=$(realpath "$image")
EOF
    # Define VM but don't start it yet.
    "$HM_BIN" vm create \
        --name "$vm_id" \
        --cpus "$cpus" \
        --memory "${mem}MiB" \
        --disk "$(realpath "$image")" \
        --net "user,hostfwd=tcp:127.0.0.1:${ssh_port}-:22" \
        --serial "file:$d/serial.log" \
        >/dev/null
    echo "$vm_id"
    ;;
  start)
    vm_id="$1"
    "$HM_BIN" vm start "$vm_id" >/dev/null
    # hm-cli is expected to report pid via `vm info --json`.
    "$HM_BIN" vm info "$vm_id" --json | python3 -c "import sys,json; print(json.load(sys.stdin).get('pid',0))"
    ;;
  wait_ssh)
    vm_id="$1" ssh_port="$2" timeout="$3"
    # shellcheck disable=SC1091
    source "$(dirname "$0")/../lib/ssh.sh"
    USER=${BENCH_SSH_USER:-bench}; KEY=${BENCH_SSH_KEY:?BENCH_SSH_KEY required}
    ssh_wait "$USER" "$KEY" 127.0.0.1 "$ssh_port" "$timeout"
    ;;
  snapshot)
    vm_id="$1" name="$2"
    "$HM_BIN" vm snapshot "$vm_id" "$name" >/dev/null 2>&1 || exit 78
    ;;
  restore)
    vm_id="$1" name="$2"
    "$HM_BIN" vm restore "$vm_id" "$name" >/dev/null 2>&1 || exit 78
    ;;
  stop)
    vm_id="$1"
    "$HM_BIN" vm stop "$vm_id" >/dev/null 2>&1 || true
    ;;
  destroy)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"
    "$HM_BIN" vm destroy "$vm_id" >/dev/null 2>&1 || true
    [[ -d "$d" ]] && rm -rf "$d"
    ;;
  metrics)
    vm_id="$1"
    pid=$("$HM_BIN" vm info "$vm_id" --json 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('pid',0))" 2>/dev/null || echo 0)
    if [[ "$pid" -gt 0 ]] && [[ -r "/proc/$pid/status" ]]; then
        rss=$(awk '/^VmRSS/{print $2}' "/proc/$pid/status")
        cpu=$(ps -o %cpu= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)
        echo "rss_kib=$rss,cpu_pct=$cpu"
    else
        echo "rss_kib=0,cpu_pct=0"
    fi
    ;;
  *) echo "unknown verb $VERB" >&2; exit 2 ;;
esac
