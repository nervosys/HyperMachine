#!/usr/bin/env bash
# A shell REPL inside a HyperMachine VM (KVM), on the guest's serial console.
#
# From Windows:  wsl -e bash -lc /mnt/c/path/to/HyperMachine/tools/serial-repl.sh
# From Linux:    tools/serial-repl.sh
#
# KERNEL / INITRD override the guest images; HV2_BOOT_LOG=1 shows kernel boot
# messages. Ctrl-D stops the VM.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
KERNEL="${KERNEL:-/var/tmp/kbuild/bzImage}"
INITRD="${INITRD:-/var/tmp/kbuild/initramfs.cpio.gz}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.hm-wsl-target}"
[ -r /dev/kvm ] || { echo "no readable /dev/kvm" >&2; exit 1; }
cd "$here"
cargo build -q -p hv2-core --example serial_repl
HV2_INITRD="$INITRD" exec "$CARGO_TARGET_DIR/debug/examples/serial_repl" "$KERNEL"
