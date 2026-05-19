#!/usr/bin/env bash
# Adapter: VirtualBox (Linux or macOS host). Snapshot/restore supported.
set -euo pipefail

VERB="${1:?verb required}"; shift || true
STATE_ROOT="${BENCH_STATE_ROOT:-$(pwd)/.state}/virtualbox"
mkdir -p "$STATE_ROOT"
_vm_dir() { echo "$STATE_ROOT/$1"; }

VBM="${VBM:-VBoxManage}"

case "$VERB" in
  setup)
    image="$1" cpus="$2" mem="$3" disk="$4" netif="$5"
    vm_id="vbox-$(date +%s%N)-$$"
    d="$(_vm_dir "$vm_id")"; mkdir -p "$d"
    ssh_port=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")
    # Convert qcow2 → vdi (VirtualBox can't read qcow2 directly).
    if [[ "$image" == *.qcow2 ]]; then
        qemu-img convert -O vdi "$image" "$d/disk.vdi"
    else
        cp --reflink=auto "$image" "$d/disk.vdi"
    fi
    "$VBM" createvm --name "$vm_id" --basefolder "$d/vbox" --register >/dev/null
    "$VBM" modifyvm "$vm_id" --memory "$mem" --cpus "$cpus" --ioapic on --nic1 nat --natpf1 "ssh,tcp,127.0.0.1,${ssh_port},,22" >/dev/null
    "$VBM" storagectl "$vm_id" --name "SATA" --add sata --controller IntelAhci >/dev/null
    "$VBM" storageattach "$vm_id" --storagectl SATA --port 0 --device 0 --type hdd --medium "$d/disk.vdi" >/dev/null
    cat > "$d/meta" <<EOF
ssh_port=$ssh_port
EOF
    echo "$vm_id"
    ;;
  start)
    vm_id="$1"
    "$VBM" startvm "$vm_id" --type headless >/dev/null
    # VBoxHeadless pid:
    pgrep -nf "VBoxHeadless.*--comment $vm_id" || echo 0
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
    "$VBM" snapshot "$vm_id" take "$name" --live >/dev/null
    ;;
  restore)
    vm_id="$1" name="$2"
    "$VBM" controlvm "$vm_id" poweroff >/dev/null 2>&1 || true
    "$VBM" snapshot  "$vm_id" restore "$name" >/dev/null
    "$VBM" startvm   "$vm_id" --type headless >/dev/null
    ;;
  stop)
    vm_id="$1"
    "$VBM" controlvm "$vm_id" acpipowerbutton >/dev/null 2>&1 || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        state=$("$VBM" showvminfo "$vm_id" --machinereadable 2>/dev/null | awk -F= '/^VMState=/{gsub(/"/,"",$2);print $2}')
        [[ "$state" == "poweroff" ]] && break
        sleep 1
    done
    "$VBM" controlvm "$vm_id" poweroff >/dev/null 2>&1 || true
    ;;
  destroy)
    vm_id="$1"; d="$(_vm_dir "$vm_id")"
    "$VBM" unregistervm "$vm_id" --delete >/dev/null 2>&1 || true
    [[ -d "$d" ]] && rm -rf "$d"
    ;;
  metrics)
    vm_id="$1"
    pid=$(pgrep -nf "VBoxHeadless.*--comment $vm_id" 2>/dev/null || echo 0)
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
