#!/usr/bin/env bash
# End-to-end egress check: real KVM guests, created through the E2B API,
# trying to leave through hv2-sandboxd's gateway.
#
#   HV2_KERNEL=bzImage HV2_INITRD=guest-net.cpio.gz tools/e2e-egress.sh path/to/hv2-sandboxd
#
# The initramfs needs curl (tools/guest-image/build.sh builds one). Needs
# /dev/kvm, python3, curl and jq on the host, and the internet for the one
# connection that is meant to succeed. Exits non-zero if any check fails.
#
# What it asserts:
#   1. A sandbox that configures no network is refused (the operator default
#      is deny) -- the Phase D exit criterion.
#   2. A sandbox with allowOut ["0.0.0.0/0"] reaches the internet, and does
#      NOT reach a private address on this host (a stand-in for the cluster
#      store) or the cloud metadata address.
#   3. With the operator's --tenant-reserved-cidr for that address, the same
#      sandbox does reach it -- so (2)'s refusal is the policy, not the setup.
#
# Uses ports 13980/13981, not the daemon's defaults, so it can run beside one.
set -u
bin=${1:?usage: $0 path/to/hv2-sandboxd}
: "${HV2_KERNEL:?set HV2_KERNEL}" "${HV2_INITRD:?set HV2_INITRD}"
export HV2_KERNEL HV2_INITRD

api=http://127.0.0.1:13980
work=$(mktemp -d)
private=$(ip -4 -o route get 1.1.1.1 | sed -n 's/.* src \([0-9.]*\).*/\1/p')
failures=0
daemon=""

mkdir -p "$work/www" && echo "STORE-REACHED" >"$work/www/marker"
(cd "$work/www" && exec python3 -m http.server 18080 --bind "$private") >"$work/http.log" 2>&1 &
http=$!
cleanup() {
    [ -n "$daemon" ] && kill "$daemon" 2>/dev/null
    kill "$http" 2>/dev/null
    rm -rf "$work"
}
trap cleanup EXIT

check() { # check NAME EXPECTED ACTUAL: EXPECTED is a grep -E pattern
    if printf '%s' "$3" | grep -qE "$2"; then
        echo "ok    $1"
    else
        echo "FAIL  $1: expected /$2/, got: $3"
        failures=$((failures + 1))
    fi
}
create() {
    curl -s -XPOST "$api/sandboxes" -H content-type:application/json -d "$1" | jq -r '.sandboxID'
}
run() {
    curl -s -XPOST "$api/sandboxes/$1/exec" -H content-type:application/json \
        -d "$(jq -n --arg c "$2" '{cmd: $c, timeout_secs: 30}')" |
        jq -r '(.stdout // "") + (.stderr // "")'
}
start() {
    "$bin" --port 13980 --proxy-port 13981 --capacity 4 --network "$@" >"$work/daemon.log" 2>&1 &
    daemon=$!
    for _ in $(seq 1 120); do curl -sf "$api/metrics" >/dev/null && return; sleep 0.5; done
    echo "FAIL  daemon did not come up:"; tail -20 "$work/daemon.log"; exit 1
}
stop() { kill "$daemon"; wait "$daemon" 2>/dev/null; daemon=""; }

echo "private address standing in for the store: $private:18080"

start
sbx=$(create '{"templateID":"base","timeout":120}')
check "no network config: the internet is refused" "Failed to connect|Could not resolve" \
    "$(run "$sbx" 'curl -sS -m 8 http://1.1.1.1/ 2>&1')"
sbx=$(create '{"templateID":"base","timeout":120,"network":{"allowOut":["0.0.0.0/0"]}}')
check "allowOut 0.0.0.0/0: the internet is reached" "^[1-5][0-9][0-9]$" \
    "$(run "$sbx" 'curl -sS -m 8 -o /dev/null -w %{http_code} http://1.1.1.1/ 2>&1')"
check "allowOut 0.0.0.0/0: a private address is refused" "Failed to connect" \
    "$(run "$sbx" "curl -sS -m 8 http://$private:18080/marker 2>&1")"
check "allowOut 0.0.0.0/0: the metadata address is refused" "Failed to connect" \
    "$(run "$sbx" 'curl -sS -m 5 http://169.254.169.254/ 2>&1')"
# The curl failures above could be the network's doing (there is no metadata
# service outside a cloud); the gateway's own record says whether it was policy.
log=$(curl -s "$api/sandboxes/$sbx/network/decisions" |
    jq -r '.decisions[] | "\(.verdict) \(.destination) \(.reason)"')
check "gateway: the private address was denied as reserved" "deny $private:18080 reserved address" "$log"
check "gateway: the metadata address was denied as reserved" "deny 169.254.169.254:80 reserved address" "$log"
stop

start --tenant-reserved-cidr "$private/32"
sbx=$(create '{"templateID":"base","timeout":120,"network":{"allowOut":["0.0.0.0/0"]}}')
check "operator grant: the granted address is reached" "STORE-REACHED" \
    "$(run "$sbx" "curl -sS -m 8 http://$private:18080/marker 2>&1")"
check "operator grant: the metadata address is still refused" "Failed to connect" \
    "$(run "$sbx" 'curl -sS -m 5 http://169.254.169.254/ 2>&1')"
stop

[ "$failures" -eq 0 ] && echo "all egress checks passed" || echo "$failures check(s) failed"
exit "$failures"
