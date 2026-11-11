#!/usr/bin/env bash
# Build a minimal ext4 root-disk image for linux_whpx_boot.rs's
# root_from_pci_disk test (HV2_TEST_DISK), run in WSL (needs root, losetup,
# mkfs.ext4, sfdisk). NOT part of the normal guest-image build; this is a
# one-off local test fixture, not something CI builds.
#
#   sudo tools/guest-image/build-root-disk-fixture.sh -o disk.img [--busybox /usr/bin/busybox]
#
# The test's own comment says to run it with HV2_TEST_INITRD unset: the
# kernel mounts /dev/vda1 itself and runs init=/bin/sh from it, so this disk
# needs a real /bin/sh (and /bin/mount, /bin/grep, /bin/sleep) on an ext4
# partition recognized as vda1 by the kernel's partition scan.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
out=""
busybox=$(command -v busybox || true)
size_mib=32

while [ $# -gt 0 ]; do
    case "$1" in
        -o|--output) out=$2; shift 2 ;;
        --busybox) busybox=$2; shift 2 ;;
        --size-mib) size_mib=$2; shift 2 ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "build-root-disk-fixture.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out" ] || { echo "build-root-disk-fixture.sh: -o OUTPUT is required" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "build-root-disk-fixture.sh: needs root (losetup/mount)" >&2; exit 2; }
[ -n "$busybox" ] && [ -x "$busybox" ] || { echo "build-root-disk-fixture.sh: no busybox (use --busybox)" >&2; exit 1; }
if file -L "$busybox" | grep -q "dynamically linked"; then
    echo "build-root-disk-fixture.sh: $busybox is dynamically linked; the disk has no libc" >&2
    exit 1
fi
out=$(realpath -m "$out")

truncate -s "${size_mib}M" "$out"
sfdisk "$out" <<<'type=83'

loopdev=$(losetup --find --show --partscan "$out")
cleanup() { losetup -d "$loopdev" 2>/dev/null || true; }
trap cleanup EXIT

part="${loopdev}p1"
# losetup --partscan needs a moment to expose the partition node.
for _ in $(seq 1 20); do [ -e "$part" ] && break; sleep 0.1; done
[ -e "$part" ] || { echo "build-root-disk-fixture.sh: $part never appeared" >&2; exit 1; }

mkfs.ext4 -q -F -L hv2root "$part"

mnt=$(mktemp -d)
trap 'umount "$mnt" 2>/dev/null || true; rmdir "$mnt" 2>/dev/null || true; cleanup' EXIT
mount "$part" "$mnt"

mkdir -p "$mnt"/{bin,sbin,proc,sys,dev,tmp,etc,root}
install -m 0755 "$busybox" "$mnt/bin/busybox"
for applet in sh mount grep sleep cat echo; do
    ln -sf busybox "$mnt/bin/$applet"
done

sync
umount "$mnt"
rmdir "$mnt"
losetup -d "$loopdev"
trap - EXIT

echo "build-root-disk-fixture.sh: $out ($(du -h "$out" | cut -f1))"
