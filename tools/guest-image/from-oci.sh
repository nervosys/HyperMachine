#!/usr/bin/env bash
# Build a sandbox template's initramfs from an OCI image -- what E2B builds a
# template from a Dockerfile for.
#
#   tools/guest-image/from-oci.sh IMAGE -o template.cpio.gz [build.sh options...]
#
#   tools/guest-image/from-oci.sh python:3.12-slim -o python.cpio.gz \
#       --busybox /bin/busybox --ca-bundle /etc/ssl/certs/ca-certificates.crt
#
# The image's filesystem becomes the guest's root, as an initramfs held in
# memory; its ENV is loaded by init before the agent starts, so commands see
# the PATH and variables the image was built for. Its ENTRYPOINT and CMD are
# not run: a sandbox's processes are the ones its user starts. Then:
#
#   hv2-sandboxd --template python=python.cpio.gz ...
#   Sandbox.create(template="python")
#
# Uses `docker` (or $DOCKER) to create and export the image; the image is
# pulled if it is not local. The export needs no root: files are recorded as
# root's in the archive. Device nodes in the image are skipped -- the guest's
# /dev is devtmpfs.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
docker=${DOCKER:-docker}
image=${1:?usage: from-oci.sh IMAGE -o OUTPUT [build.sh options...]}
shift

rootfs=$(mktemp -d)
container=""
cleanup() {
    [ -n "$container" ] && "$docker" rm -f "$container" >/dev/null 2>&1 || true
    chmod -R u+w "$rootfs" 2>/dev/null || true
    rm -rf "$rootfs"
}
trap cleanup EXIT

container=$("$docker" create --platform linux/amd64 "$image" /bin/true)
"$docker" export "$container" \
    | tar -x -C "$rootfs" --no-same-owner --exclude='dev/*' 2>/dev/null || true
[ -d "$rootfs/usr" ] || { echo "from-oci.sh: exporting $image produced no root filesystem" >&2; exit 1; }

# The image's environment, for init to load. Each variable single-quoted,
# with any single quote in a value closed, escaped and reopened.
"$docker" image inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$image" \
    | while IFS= read -r line; do
        [ -n "$line" ] || continue
        key=${line%%=*}
        value=${line#*=}
        printf "export %s='%s'\n" "$key" "${value//\'/\'\\\'\'}"
    done > "$rootfs/etc/hv2-env"
# Docker writes these into a container's /etc; in a sandbox they are the
# gateway's and the kernel's to say.
rm -f "$rootfs/etc/resolv.conf" "$rootfs/etc/hostname" "$rootfs/etc/hosts" "$rootfs/.dockerenv"

exec "$here/build.sh" --rootfs "$rootfs" "$@"
