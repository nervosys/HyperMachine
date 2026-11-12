#!/usr/bin/env bash
# Build the sandbox guest's initramfs: busybox, hv2-guest-agentd, and this
# directory's init, plus whatever else a template needs.
#
#   tools/guest-image/build.sh -o guest.cpio.gz \
#       [--busybox /bin/busybox] [--agent path/to/hv2-guest-agentd] \
#       [--extra /path/to/curl[:NAME] ...] [--ca-bundle /etc/ssl/certs/ca-certificates.crt] \
#       [--rootfs DIR]   # start from a root filesystem, e.g. from-oci.sh's
#       [--init FILE]    # PID 1; init in this directory by default, or init-systemd
#       [--no-busybox]   # no busybox, no /bin/sh, no bash shim: only --init (required, static,
#                        # e.g. hv2-guest-agentd as PID 1), the agent, the --extra binaries and
#                        # empty directories. Cannot be combined with --rootfs.
#
# Every binary must be static: the image has no libc of its own. The agent is
# built static by default if --agent is not given:
#
#   RUSTFLAGS="-C target-feature=+crt-static" cargo build --release \
#       -p hv2-guest-agent --target x86_64-unknown-linux-gnu
#
# Needs no root: files are recorded as owned by root in the archive (cpio -R),
# not chowned on disk. Deterministic up to the inputs: fixed mtimes, sorted
# entries, gzip -n.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
out=""
busybox=$(command -v busybox || true)
agent=""
ca_bundle=""
rootfs=""
init=""
no_busybox=0
extras=()

while [ $# -gt 0 ]; do
    case "$1" in
        -o|--output) out=$2; shift 2 ;;
        --busybox) busybox=$2; shift 2 ;;
        --agent) agent=$2; shift 2 ;;
        --extra) extras+=("$2"); shift 2 ;;
        --ca-bundle) ca_bundle=$2; shift 2 ;;
        --rootfs) rootfs=$2; shift 2 ;;
        --init) init=$2; shift 2 ;;
        --no-busybox) no_busybox=1; shift ;;
        -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
        *) echo "build.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$out" ] || { echo "build.sh: -o OUTPUT is required" >&2; exit 2; }
if [ "$no_busybox" = 1 ]; then
    [ -n "$init" ] || { echo "build.sh: --no-busybox needs --init FILE (a static PID 1, e.g. hv2-guest-agentd)" >&2; exit 2; }
    [ -z "$rootfs" ] || { echo "build.sh: --no-busybox cannot be combined with --rootfs (it brings its own shell)" >&2; exit 2; }
fi
: "${init:=$here/init}"
[ -f "$init" ] || { echo "build.sh: no init $init" >&2; exit 2; }
out=$(realpath -m "$out")

static() {
    # A dynamically linked binary starts nowhere in an image with no libc, and
    # fails as "not found" -- the loader it names is what is missing.
    if file -L "$1" | grep -q "dynamically linked"; then
        echo "build.sh: $1 is dynamically linked; the image has no libc" >&2
        exit 1
    fi
}

if [ "$no_busybox" = 1 ]; then
    # Nothing in the image may be a shell, so the init is held to the same rule
    # as every other binary: static, or it starts nowhere.
    static "$init"
else
    [ -n "$busybox" ] && [ -x "$busybox" ] || { echo "build.sh: no busybox (use --busybox)" >&2; exit 1; }
    static "$busybox"
fi

if [ -z "$agent" ]; then
    (cd "$repo" && RUSTFLAGS="-C target-feature=+crt-static" cargo build --release \
        -p hv2-guest-agent --target x86_64-unknown-linux-gnu \
        --target-dir "$repo/target/guest-image") >&2
    agent="$repo/target/guest-image/x86_64-unknown-linux-gnu/release/hv2-guest-agentd"
fi
static "$agent"

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
# A root filesystem to start from -- an OCI image's, from from-oci.sh -- in
# which case what it already has is kept: its own shell, its own bash, its
# libc for everything else. Busybox fills in what it lacks at boot.
if [ -n "$rootfs" ]; then
    cp -a "$rootfs"/. "$root"/
fi
if [ "$no_busybox" = 1 ]; then
    mkdir -p "$root"/{bin,dev,proc,sys,tmp,etc,root}
    install -m 0755 "$agent" "$root/bin/hv2-guest-agentd"
    install -m 0755 "$init" "$root/init"
else
    mkdir -p "$root"/{bin,sbin,dev,proc,sys,tmp,etc,root}
    install -m 0755 "$busybox" "$root/bin/busybox"
    [ -e "$root/bin/sh" ] || [ -L "$root/bin/sh" ] || ln -s busybox "$root/bin/sh"
    install -m 0755 "$agent" "$root/bin/hv2-guest-agentd"
    install -m 0755 "$init" "$root/init"
    # The SDK runs everything through /bin/bash; an --extra named bash, or the
    # rootfs's own, replaces this.
    [ -e "$root/bin/bash" ] || install -m 0755 "$here/bash-shim" "$root/bin/bash"
fi
for spec in "${extras[@]}"; do
    # SRC or SRC:NAME -- the name it has in the image, when not its own.
    x=${spec%%:*}
    name=${spec#*:}
    [ "$name" = "$spec" ] && name=$(basename "$x")
    if [ "$no_busybox" = 1 ]; then
        case "$name" in
            sh|bash|busybox) echo "build.sh: --no-busybox: an --extra named $name would be a shell" >&2; exit 2 ;;
        esac
    fi
    static "$x"
    install -m 0755 "$x" "$root/bin/$name"
done
if [ -n "$ca_bundle" ]; then
    mkdir -p "$root/etc/ssl/certs"
    install -m 0644 "$ca_bundle" "$root/etc/ssl/certs/ca-certificates.crt"
fi

(cd "$root" && find . -print0 | LC_ALL=C sort -z | xargs -0 touch -h -d @0 \
    && find . -print0 | LC_ALL=C sort -z \
    | cpio --null -o -H newc -R 0:0 --reproducible --quiet | gzip -9 -n) > "$out"
echo "build.sh: $out ($(du -h "$out" | cut -f1))"
