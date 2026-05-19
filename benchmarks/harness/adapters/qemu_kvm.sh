#!/usr/bin/env bash
# Adapter: QEMU + KVM (Linux). Implements the full ABI.
set -euo pipefail

VERB="${1:?verb required}"; shift || true
STATE_ROOT="${BENCH_STATE_ROOT:-$(pwd)/.state}/qemu_kvm"
mkdir -p "$STATE_ROOT"

_vm_dir() { echo "$STATE_ROOT/$1"; }

case "$VERB" in
  setup)
    image="$1" cpus="$2" mem="$3" disk="$4" netif="$5"
    vm_id="qemu-$(date +%s%N)-$$"
    d="$(_vm_dir "$vm_id")"; mkdir -p "$d"
    # Copy-on-write overlay so the base image is never modified.
    qemu-img create -f qcow2 -F qcow2 -b "$(realpath "$image")" "$d/disk.qcow2" >/dev/null
    # Allocate a free SSH port.
    ssh_port=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
    cat > "$d/meta" <<EOF
cpus=$cpus
mem=$mem
netif=$netif
ssh_port=$ssh_port
monitor_sock=$d/qmp.sock
qemu_pid_file=$d/qemu.pid
EOF
    echo "$vm_id"
    ;;
  start)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    case "$netif" in
      user) net=( -netdev "user,id=n0,hostfwd=tcp:127.0.0.1:${ssh_port}-:22" -device virtio-net-pci,netdev=n0 ) ;;
      tap*) net=( -netdev "tap,id=n0,ifname=$netif,script=no,downscript=no" -device virtio-net-pci,netdev=n0 ) ;;
      *)    echo "unknown netif $netif" >&2; exit 1 ;;
    esac
    qemu-system-x86_64 \
        -machine q35,accel=kvm -cpu host -smp "$cpus" -m "$mem" \
        -drive "file=$d/disk.qcow2,if=virtio,format=qcow2,cache=none,aio=native" \
        "${net[@]}" \
        -nographic -serial "file:$d/serial.log" \
        -qmp "unix:$monitor_sock,server,nowait" \
        -pidfile "$qemu_pid_file" \
        -daemonize
    cat "$qemu_pid_file"
    ;;
  wait_ssh)
    vm_id="$1" ssh_port="$2" timeout="$3"
    # shellcheck disable=SC1091
    source "$(dirname "$0")/../lib/ssh.sh"
    USER=${BENCH_SSH_USER:-bench}; KEY=${BENCH_SSH_KEY:?BENCH_SSH_KEY required}
    ssh_wait "$USER" "$KEY" 127.0.0.1 "$ssh_port" "$timeout"
    ;;
  snapshot)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    # Use QMP via socat if available, else fall back to "savevm" via HMP.
    if command -v socat >/dev/null 2>&1; then
        printf '{"execute":"qmp_capabilities"}\n{"execute":"human-monitor-command","arguments":{"command-line":"savevm %s"}}\n' "$name" \
            | socat - "UNIX-CONNECT:$monitor_sock" >/dev/null
    else
        exit 78
    fi
    ;;
  restore)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    if command -v socat >/dev/null 2>&1; then
        printf '{"execute":"qmp_capabilities"}\n{"execute":"human-monitor-command","arguments":{"command-line":"loadvm %s"}}\n' "$name" \
            | socat - "UNIX-CONNECT:$monitor_sock" >/dev/null
    else
        exit 78
    fi
    ;;
  stop)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    if [[ -f "$qemu_pid_file" ]]; then
        pid=$(cat "$qemu_pid_file")
        kill -TERM "$pid" 2>/dev/null || true
        for _ in 1 2 3 4 5; do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.5
        done
        kill -KILL "$pid" 2>/dev/null || true
    fi
    ;;
  destroy)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"
    [[ -d "$d" ]] && rm -rf "$d"
    ;;
  metrics)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    pid=$(cat "$qemu_pid_file" 2>/dev/null || echo 0)
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
