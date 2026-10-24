#!/usr/bin/env bash
# A sandbox template with the development tools already in it: Ubuntu 24.04
# with git, Python 3, Node.js, npm, gcc, make, curl, jq, less, ps and ssh.
#
#   sudo tools/guest-image/build-tools-image.sh -o tools.cpio.gz \
#       --busybox /path/to/static/busybox --agent /path/to/static/hv2-guest-agentd \
#       [--work DIR] [--packages "pkg pkg ..."] [--systemd]
#
# --systemd adds systemd and makes it the guest's first process (init-systemd):
# see docs/TOOLS_IMAGE.md#systemd.
#
# --docker docker-X.tgz (Docker's static binaries) adds them, iptables and a
# docker.service, and implies --systemd. The guest needs a kernel built with
# build-kernel.sh --docker: see docs/TOOLS_IMAGE.md#docker.
# --compose BIN (the docker-compose-linux-x86_64 release binary) adds `docker compose`.
#
# Then run a node with `--template base=tools.cpio.gz` (or HV2_INITRD) and give
# it at least 1536 MiB per sandbox: see docs/TOOLS_IMAGE.md for why.
#
# It does not use Docker or any container runtime. debootstrap makes the root
# filesystem from Ubuntu's archive (the host needs `debootstrap` and
# `ubuntu-keyring`, and network), apt installs the packages inside a chroot,
# and build.sh packs the result with the guest agent and init. Needs root,
# for chroot and mount. Nothing is installed on the host.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
out="" busybox="" agent="" work=/var/tmp/hm-tools-image init="" systemd="" docker="" compose=""
packages="ca-certificates curl git python3 nodejs npm make gcc libc6-dev jq less procps openssh-client"
while [ $# -gt 0 ]; do
    case "$1" in
        -o|--output) out=$2; shift 2 ;;
        --busybox) busybox=$2; shift 2 ;;
        --agent) agent=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --packages) packages=$2; shift 2 ;;
        --systemd) systemd=1; shift ;;
        --docker) docker=$(realpath "$2"); shift 2 ;;
        --compose) compose=$(realpath "$2"); shift 2 ;;
        -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
        *) echo "build-tools-image.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
# Whatever order the options came in: --packages replaces the list, --systemd adds to it.
[ -z "$docker" ] || { systemd=1; packages="$packages iptables kmod"; }
if [ -n "$systemd" ]; then
    packages="$packages systemd systemd-sysv dbus iproute2"
    init=$here/init-systemd
fi
[ -n "$out" ] && [ -n "$busybox" ] && [ -n "$agent" ] ||
    { echo "build-tools-image.sh: -o, --busybox and --agent are required" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "build-tools-image.sh: run as root (chroot and mount)" >&2; exit 1; }
command -v debootstrap >/dev/null || { echo "build-tools-image.sh: needs debootstrap" >&2; exit 1; }
keyring=/usr/share/keyrings/ubuntu-archive-keyring.gpg
[ -f "$keyring" ] || { echo "build-tools-image.sh: needs ubuntu-keyring ($keyring)" >&2; exit 1; }
out=$(realpath -m "$out")
busybox=$(realpath "$busybox")
agent=$(realpath "$agent")

root="$work/rootfs"
unmount() {
    umount -R -l "$root/dev" 2>/dev/null || true
    umount -l "$root/proc" 2>/dev/null || true
}
unmount
rm -rf "$work"
mkdir -p "$work"
trap unmount EXIT

# minbase, then apt in the chroot: debootstrap's --include leaves a package
# with many dependencies (npm) unconfigured.
debootstrap --variant=minbase --components=main,universe --arch=amd64 \
    --keyring="$keyring" noble "$root" http://archive.ubuntu.com/ubuntu/ >"$work/debootstrap.log" 2>&1
cat >"$root/etc/apt/sources.list" <<'EOF'
deb http://archive.ubuntu.com/ubuntu noble main universe
deb http://archive.ubuntu.com/ubuntu noble-updates main universe
deb http://security.ubuntu.com/ubuntu noble-security main universe
EOF
mount -t proc proc "$root/proc"
mount --rbind /dev "$root/dev"
cp /etc/resolv.conf "$root/etc/resolv.conf"
chroot "$root" apt-get update -qq >>"$work/apt.log" 2>&1
# shellcheck disable=SC2086
DEBIAN_FRONTEND=noninteractive chroot "$root" apt-get install -y -qq --no-install-recommends $packages \
    >>"$work/apt.log" 2>&1
chroot "$root" apt-get clean
unmount
trap - EXIT
# Nothing the guest needs: documentation, apt's lists, and the host's resolver
# (removed, not emptied: init links /etc/resolv.conf to the kernel's own
# nameserver file only when there is none).
rm -rf "$root"/var/cache/apt/archives/*.deb "$root"/var/lib/apt/lists/* \
    "$root"/usr/share/doc/* "$root"/usr/share/man/* "$root"/usr/share/info/*
rm -f "$root/etc/resolv.conf"
# apt cannot write its cache without these.
mkdir -p "$root/var/lib/apt/lists/partial" "$root/var/cache/apt/archives/partial"

if [ -n "$docker" ]; then
    tar -xzf "$docker" -C "$root/usr/local/bin" --strip-components=1
    mkdir -p "$root/etc/systemd/system/multi-user.target.wants" "$root/etc/docker"
    [ -z "$compose" ] || { mkdir -p "$root/usr/local/lib/docker/cli-plugins"; install -m 0755 "$compose" "$root/usr/local/lib/docker/cli-plugins/docker-compose"; }
    install -m 0755 "$here/runc-nopivot" "$root/usr/local/bin/runc-nopivot"
    echo '{"iptables": true, "default-runtime": "nopivot", "runtimes": {"nopivot": {"path": "/usr/local/bin/runc-nopivot"}}}'         >"$root/etc/docker/daemon.json"
    # The guest kernel has the legacy iptables and not nf_tables.
    chroot "$root" update-alternatives --set iptables /usr/sbin/iptables-legacy
    chroot "$root" update-alternatives --set ip6tables /usr/sbin/ip6tables-legacy
    cat >"$root/etc/systemd/system/docker.service" <<'UNIT'
[Unit]
Description=Docker engine
After=network.target
[Service]
ExecStart=/usr/local/bin/dockerd
Restart=on-failure
[Install]
WantedBy=multi-user.target
UNIT
    ln -sf /etc/systemd/system/docker.service "$root/etc/systemd/system/multi-user.target.wants/docker.service"
fi

bash "$here/build.sh" -o "$out" --busybox "$busybox" --agent "$agent" --rootfs "$root" ${init:+--init "$init"}
echo "build-tools-image.sh: rootfs $(du -sm "$root" | cut -f1) MiB; packages: $packages"
