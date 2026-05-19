#!/usr/bin/env bash
# benchmarks/images/build-guest.sh — build the standard benchmark guest image.
# Idempotent: re-running skips steps whose outputs already exist.
set -euo pipefail

cd "$(dirname "$0")"
BUILD=./build
mkdir -p "$BUILD"

DEBIAN_URL="${DEBIAN_URL:-https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-generic-amd64.qcow2}"
BASE="$BUILD/debian-12-base.qcow2"
OUT="$BUILD/debian-12-bench.qcow2"
KEY="$BUILD/id_ed25519"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }
need qemu-img
need curl

# 1. SSH key.
if [[ ! -f "$KEY" ]]; then
    ssh-keygen -t ed25519 -N '' -C bench -f "$KEY" >/dev/null
    echo "[+] generated $KEY"
fi

# 2. Base image.
if [[ ! -f "$BASE" ]]; then
    echo "[+] downloading Debian cloud image"
    curl -L --fail -o "$BASE" "$DEBIAN_URL"
fi

# 3. Output image (overlay or copy + customize).
if [[ ! -f "$OUT" ]]; then
    cp "$BASE" "$OUT"
    # Grow to 20 GiB so kernel_build has room.
    qemu-img resize "$OUT" 20G >/dev/null

    if command -v virt-customize >/dev/null 2>&1; then
        echo "[+] customizing via virt-customize (faster)"
        virt-customize -a "$OUT" \
            --update \
            --install "sysbench,fio,iperf3,netperf,build-essential,curl,openssh-server,qemu-guest-agent" \
            --run-command "useradd -m -s /bin/bash bench && echo 'bench ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/bench" \
            --ssh-inject "bench:file:$KEY.pub" \
            --run-command "systemctl enable ssh && systemctl disable cloud-init || true" \
            --run-command "echo 'PasswordAuthentication no' > /etc/ssh/sshd_config.d/no-pw.conf" \
            --selinux-relabel
    else
        echo "[!] virt-customize not found; fall back to cloud-init seed boot."
        need cloud-localds
        SEED="$BUILD/seed.iso"
        cat > "$BUILD/user-data" <<EOF
#cloud-config
users:
  - name: bench
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - $(cat "$KEY.pub")
packages: [sysbench, fio, iperf3, netperf, build-essential, curl]
runcmd:
  - systemctl disable cloud-init
EOF
        printf 'instance-id: bench\nlocal-hostname: bench\n' > "$BUILD/meta-data"
        cloud-localds "$SEED" "$BUILD/user-data" "$BUILD/meta-data"
        echo "[+] booting once to apply cloud-init (≈ 90 s)"
        qemu-system-x86_64 -machine q35,accel=kvm -cpu host -m 2048 -smp 2 \
            -drive "file=$OUT,if=virtio,format=qcow2" \
            -drive "file=$SEED,if=virtio,format=raw" \
            -nographic -no-reboot -serial null >/dev/null 2>&1 || true
        rm -f "$SEED"
    fi
    echo "[+] $OUT ready"
fi

# 4. Firecracker / Cloud Hypervisor artifacts.
KERNEL="$BUILD/firecracker-vmlinux"
ROOTFS="$BUILD/firecracker-rootfs.ext4"

if [[ ! -f "$KERNEL" || ! -f "$ROOTFS" ]]; then
    if ! command -v virt-cat >/dev/null 2>&1 || ! command -v virt-tar-out >/dev/null 2>&1; then
        echo "[!] libguestfs-tools not installed — skipping firecracker/cloud-hypervisor artifacts."
        echo "    apt install libguestfs-tools, then re-run."
        exit 0
    fi
    # Pull the latest installed kernel out of the qcow2.
    echo "[+] extracting kernel from guest"
    KVER=$(virt-ls -a "$OUT" /lib/modules | sort -V | tail -1)
    virt-cat -a "$OUT" "/boot/vmlinuz-$KVER" > "$BUILD/vmlinuz-$KVER.gz" || true
    # Uncompress in case it's a bzImage; both Firecracker and CH want raw ELF/vmlinux.
    if file "$BUILD/vmlinuz-$KVER.gz" | grep -q 'bzImage'; then
        # extract-vmlinux is shipped with the kernel source tree, but we can use
        # a minimal Python decompressor here as a fallback.
        echo "[!] bzImage detected; you'll need to convert to raw vmlinux. See"
        echo "    https://github.com/torvalds/linux/blob/master/scripts/extract-vmlinux"
        exit 0
    fi
    cp "$BUILD/vmlinuz-$KVER.gz" "$KERNEL"
    cp "$KERNEL" "$BUILD/ch-vmlinux"

    echo "[+] producing flat ext4 rootfs ($ROOTFS)"
    truncate -s 4G "$ROOTFS"
    mkfs.ext4 -F -q "$ROOTFS"
    MNT=$(mktemp -d)
    sudo mount -o loop "$ROOTFS" "$MNT"
    virt-tar-out -a "$OUT" / - | sudo tar -xpf - -C "$MNT"
    sudo umount "$MNT"
    rmdir "$MNT"
fi

echo "[✓] image build complete"
ls -lh "$BUILD"
