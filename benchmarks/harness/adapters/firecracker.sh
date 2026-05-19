#!/usr/bin/env bash
# Adapter: Firecracker. Requires uncompressed kernel + ext4 rootfs.
# Snapshot/restore use Firecracker's built-in snapshot API.
set -euo pipefail

VERB="${1:?verb required}"; shift || true
STATE_ROOT="${BENCH_STATE_ROOT:-$(pwd)/.state}/firecracker"
mkdir -p "$STATE_ROOT"
_vm_dir() { echo "$STATE_ROOT/$1"; }

# Expected layout in images/build/:
#   firecracker-vmlinux       (uncompressed kernel)
#   firecracker-rootfs.ext4   (ext4 rootfs with ssh enabled)
FC_KERNEL="${FC_KERNEL:-images/build/firecracker-vmlinux}"
FC_ROOTFS="${FC_ROOTFS:-images/build/firecracker-rootfs.ext4}"

_curl_sock() { # _curl_sock <sock> <method> <url> [data]
    local sock="$1" method="$2" url="$3" data="${4:-}"
    if [[ -n "$data" ]]; then
        curl -s --unix-socket "$sock" -X "$method" "http://localhost$url" \
             -H 'Accept: application/json' -H 'Content-Type: application/json' -d "$data"
    else
        curl -s --unix-socket "$sock" -X "$method" "http://localhost$url" -H 'Accept: application/json'
    fi
}

case "$VERB" in
  setup)
    image="$1" cpus="$2" mem="$3" disk="$4" netif="$5"  # image ignored; FC uses kernel+rootfs
    vm_id="fc-$(date +%s%N)-$$"
    d="$(_vm_dir "$vm_id")"; mkdir -p "$d"
    cp "$FC_ROOTFS" "$d/rootfs.ext4"
    ssh_port=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
    cat > "$d/meta" <<EOF
cpus=$cpus
mem=$mem
netif=$netif
ssh_port=$ssh_port
api_sock=$d/fc.sock
pid_file=$d/fc.pid
EOF
    echo "$vm_id"
    ;;
  start)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    rm -f "$api_sock"
    firecracker --api-sock "$api_sock" >"$d/fc.log" 2>&1 &
    echo $! > "$pid_file"
    # Wait for API socket.
    for _ in $(seq 1 40); do [[ -S "$api_sock" ]] && break; sleep 0.05; done
    _curl_sock "$api_sock" PUT /boot-source \
        "{\"kernel_image_path\":\"$(realpath "$FC_KERNEL")\",\"boot_args\":\"console=ttyS0 reboot=k panic=1 pci=off\"}" >/dev/null
    _curl_sock "$api_sock" PUT /drives/rootfs \
        "{\"drive_id\":\"rootfs\",\"path_on_host\":\"$d/rootfs.ext4\",\"is_root_device\":true,\"is_read_only\":false}" >/dev/null
    _curl_sock "$api_sock" PUT /machine-config \
        "{\"vcpu_count\":$cpus,\"mem_size_mib\":$mem,\"smt\":false}" >/dev/null
    # Networking: requires a tap on the host. For SSH-via-user-mode Firecracker
    # doesn't support hostfwd; the harness expects a tap+NAT prepared out of band.
    # Skip wiring net here; the rootfs is responsible for `dhcp eth0`.
    _curl_sock "$api_sock" PUT /actions '{"action_type":"InstanceStart"}' >/dev/null
    cat "$pid_file"
    ;;
  wait_ssh)
    vm_id="$1" ssh_port="$2" timeout="$3"
    # shellcheck disable=SC1091
    source "$(dirname "$0")/../lib/ssh.sh"
    USER=${BENCH_SSH_USER:-bench}; KEY=${BENCH_SSH_KEY:?BENCH_SSH_KEY required}
    HOST=${FC_GUEST_IP:-127.0.0.1}
    ssh_wait "$USER" "$KEY" "$HOST" "$ssh_port" "$timeout"
    ;;
  snapshot)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    _curl_sock "$api_sock" PATCH /vm '{"state":"Paused"}' >/dev/null || exit 78
    _curl_sock "$api_sock" PUT /snapshot/create \
        "{\"snapshot_type\":\"Full\",\"snapshot_path\":\"$d/snap-$name.bin\",\"mem_file_path\":\"$d/snap-$name.mem\"}" >/dev/null || exit 78
    _curl_sock "$api_sock" PATCH /vm '{"state":"Resumed"}' >/dev/null || true
    ;;
  restore)
    vm_id="$1" name="$2"; d="$(_vm_dir "$vm_id")"; . "$d/meta"
    rm -f "$api_sock"
    firecracker --api-sock "$api_sock" >"$d/fc.log" 2>&1 &
    echo $! > "$pid_file"
    for _ in $(seq 1 40); do [[ -S "$api_sock" ]] && break; sleep 0.05; done
    _curl_sock "$api_sock" PUT /snapshot/load \
        "{\"snapshot_path\":\"$d/snap-$name.bin\",\"mem_file_path\":\"$d/snap-$name.mem\",\"enable_diff_snapshots\":false,\"resume_vm\":true}" >/dev/null || exit 78
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
