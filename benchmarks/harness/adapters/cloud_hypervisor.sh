#!/usr/bin/env bash
# Adapter: Cloud Hypervisor. Similar to Firecracker but supports qcow2 + virtio-net user.
set -euo pipefail

VERB="${1:?verb required}"; shift || true
STATE_ROOT="${BENCH_STATE_ROOT:-$(pwd)/.state}/cloud_hypervisor"
mkdir -p "$STATE_ROOT"
_vm_dir() { echo "$STATE_ROOT/$1"; }

CH_KERNEL="${CH_KERNEL:-images/build/ch-vmlinux}"

case "$VERB" in
  setup)
    image="$1" cpus="$2" mem="$3" disk="$4" netif="$5"
    vm_id="ch-$(date +%s%N)-$$"
    d="$(_vm_dir "$vm_id")"; mkdir -p "$d"
    # Cloud Hypervisor accepts raw or qcow2; copy as overlay-style raw if needed.
    cp --reflink=auto "$image" "$d/disk.img"
    ssh_port=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
    cat > "$d/meta" <<EOF
cpus=$cpus
mem=$mem
netif=$netif
ssh_port=$ssh_port
api_sock=$d/ch.sock
pid_file=$d/ch.pid
EOF
    echo "$vm_id"
    ;;
  start)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    rm -f "$api_sock"
    cloud-hypervisor \
        --api-socket "$api_sock" \
        --kernel "$CH_KERNEL" \
        --cpus "boot=$cpus" \
        --memory "size=${mem}M" \
        --disk "path=$d/disk.img" \
        --net "tap=,mac=,ip=192.168.249.1,mask=255.255.255.0" \
        --serial "file=$d/serial.log" \
        --console off \
        --daemonize --pidfile "$pid_file" >/dev/null 2>&1
    cat "$pid_file"
    ;;
  wait_ssh)
    vm_id="$1" ssh_port="$2" timeout="$3"
    # shellcheck disable=SC1091
    source "$(dirname "$0")/../lib/ssh.sh"
    USER=${BENCH_SSH_USER:-bench}; KEY=${BENCH_SSH_KEY:?BENCH_SSH_KEY required}
    HOST=${CH_GUEST_IP:-192.168.249.2}
    ssh_wait "$USER" "$KEY" "$HOST" 22 "$timeout"
    ;;
  snapshot)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    ch-remote --api-socket "$api_sock" pause >/dev/null || exit 78
    ch-remote --api-socket "$api_sock" snapshot "file://$d/snap-$name" >/dev/null || exit 78
    ch-remote --api-socket "$api_sock" resume >/dev/null || true
    ;;
  restore)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    ch-remote --api-socket "$api_sock" restore "source_url=file://$d/snap-$name" >/dev/null || exit 78
    ;;
  stop)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    [[ -f "$pid_file" ]] || exit 0
    pid=$(cat "$pid_file")
    kill -TERM "$pid" 2>/dev/null || true
    for _ in 1 2 3 4; do kill -0 "$pid" 2>/dev/null || break; sleep 0.3; done
    kill -KILL "$pid" 2>/dev/null || true
    ;;
  destroy)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"
    [[ -d "$d" ]] && rm -rf "$d"
    ;;
  metrics)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    pid=$(cat "$pid_file" 2>/dev/null || echo 0)
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
