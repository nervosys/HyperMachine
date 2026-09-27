#!/usr/bin/env bash
# Build the sandbox guest kernel: Linux 6.6.52, configured by hv2-guest.defconfig.
#
#   tools/guest-image/build-kernel.sh -o bzImage [--source linux-6.6.52.tar.xz]
#
# The defconfig is `make savedefconfig` of the configuration the sandbox
# guest was first booted and verified with: virtio-mmio devices named on the
# command line, virtio-vsock and virtio-net, IP autoconfiguration from `ip=`,
# an initramfs root, a serial console, and little else.
#
# Needs a C toolchain, bc, flex, bison and libelf headers; no root.
set -euo pipefail

version=6.6.52
sha256=1591ab348399d4aa53121158525056a69c8cf0fe0e90935b0095e9a58e37b4b8
here=$(cd "$(dirname "$0")" && pwd)
out=""
source=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o|--output) out=$2; shift 2 ;;
        --source) source=$2; shift 2 ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "build-kernel.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out" ] || { echo "build-kernel.sh: -o OUTPUT is required" >&2; exit 2; }
out=$(realpath -m "$out")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [ -z "$source" ]; then
    source="$work/linux-$version.tar.xz"
    curl -fsSL -o "$source" "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-$version.tar.xz"
fi
echo "$sha256  $source" | sha256sum -c --quiet -
tar -xf "$source" -C "$work"
cd "$work/linux-$version"
cp "$here/hv2-guest.defconfig" arch/x86/configs/hv2_guest_defconfig
make -s hv2_guest_defconfig
# A fixed build identity, so two builds of one tree produce one kernel.
export KBUILD_BUILD_TIMESTAMP="1970-01-01" KBUILD_BUILD_USER=hv2 KBUILD_BUILD_HOST=hv2
make -s -j"$(nproc)" bzImage
install -D -m 0644 arch/x86/boot/bzImage "$out"
echo "build-kernel.sh: $out ($(du -h "$out" | cut -f1))"
