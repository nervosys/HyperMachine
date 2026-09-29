#!/usr/bin/env bash
# End-to-end check of idle pause on real KVM guests, through the E2B API.
#
#   HV2_KERNEL=bzImage HV2_INITRD=guest.cpio.gz tools/e2e-idle-pause.sh path/to/hv2-sandboxd
#
# Needs /dev/kvm, curl and jq. Exits non-zero if any check fails. Takes about
# two minutes: idle windows are at least 30 s, by design.
#
# What it asserts:
#   1. A sandbox created with idleTimeout that nobody uses pauses by itself.
#   2. With autoResume, one request through the proxy wakes it, and it
#      answers that request.
#   3. A sandbox whose guest is busy computing -- no traffic at all -- is NOT
#      paused: idle means quiet, not unobserved.
#   4. A sandbox running a long, quiet command is not paused under it.
#   5. A sandbox without idleTimeout is left alone.
#   6. idleTimeout under 30 s is refused.
#
# Uses ports 13995/13996, not the daemon's defaults, so it can run beside one.
set -u
bin=${1:?usage: $0 path/to/hv2-sandboxd}
: "${HV2_KERNEL:?set HV2_KERNEL}" "${HV2_INITRD:?set HV2_INITRD}"
export HV2_KERNEL HV2_INITRD

api=http://127.0.0.1:13995
proxy=127.0.0.1:13996
work=$(mktemp -d)
failures=0
daemon=""
cleanup() {
    [ -n "$daemon" ] && kill "$daemon" 2>/dev/null
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
state() { curl -s "$api/sandboxes/$1" | jq -r '.state'; }
run() { # run ID CMD TIMEOUT
    curl -s -m $(($3 + 10)) -XPOST "$api/sandboxes/$1/exec" -H content-type:application/json \
        -d "$(jq -n --arg c "$2" --argjson t "$3" '{cmd: $c, timeout_secs: $t}')" |
        jq -r '(.stdout // "") + (.stderr // "")'
}

"$bin" --port 13995 --proxy-port 13996 --capacity 6 >"$work/daemon.log" 2>&1 &
daemon=$!
for _ in $(seq 1 120); do curl -sf "$api/metrics" >/dev/null && break; sleep 0.5; done
curl -sf "$api/metrics" >/dev/null || { echo "FAIL  daemon did not come up:"; tail -20 "$work/daemon.log"; exit 1; }

code=$(curl -s -o /dev/null -w '%{http_code}' -XPOST "$api/sandboxes" -H content-type:application/json \
    -d '{"templateID":"base","timeout":300,"idleTimeout":5}')
check "idleTimeout under 30 s is refused" '^400$' "$code"

idle=$(create '{"templateID":"base","timeout":600,"idleTimeout":30,"autoResume":{"enabled":true}}')
busy=$(create '{"templateID":"base","timeout":600,"idleTimeout":30}')
quiet_cmd=$(create '{"templateID":"base","timeout":600,"idleTimeout":30}')
plain=$(create '{"templateID":"base","timeout":600}')
check "created four sandboxes" '^sbx-' "$idle $busy $quiet_cmd $plain"

# A guest that computes and never talks to anyone. setsid, because a
# one-shot exec's process group ends with it; an SDK's background command
# (envd's Start) keeps running the same way.
run "$busy" 'setsid sh -c "while :; do :; done" </dev/null >/dev/null 2>&1 &' 10 >/dev/null
# A command that takes longer than the window and uses no CPU.
curl -s -m 75 -XPOST "$api/sandboxes/$quiet_cmd/exec" -H content-type:application/json \
    -d '{"cmd":"sleep 50; echo done","timeout_secs":65}' >"$work/quiet.out" 2>&1 &
quiet_pid=$!

sleep 55
# The node's own CPU samples, which the idle decision reads: proof that the
# busy guest was busy, so its staying up below is the CPU gate, not luck.
check "the busy guest was computing" '^([5-9][0-9]|100)' \
    "$(curl -s "$api/sandboxes/$busy/metrics" | jq -r 'map(.cpuUsedPct) | max')"
check "an unused sandbox paused by itself" '^paused$' "$(state "$idle")"
check "a busy guest with no traffic kept running" '^running$' "$(state "$busy")"
check "a sandbox without idleTimeout kept running" '^running$' "$(state "$plain")"
wait "$quiet_pid"
check "a long quiet command finished" '"stdout":"done' "$(cat "$work/quiet.out")"
check "and its sandbox was not paused under it" '^running$' "$(state "$quiet_cmd")"

# One request through the proxy, on E2B's envd port. envd answers it -- 401,
# as it carries no access token -- which only a running guest can do.
code=$(curl -s -m 30 -o /dev/null -w '%{http_code}' -H "Host: 49983-$idle.sandbox.local" \
    "http://$proxy/health")
check "a request through the proxy woke it and envd answered" '^[2-4][0-9][0-9]$' "$code"
check "and it is running again" '^running$' "$(state "$idle")"
check "and runs commands" '^awake$' "$(run "$idle" 'echo awake' 10)"

[ "$failures" -eq 0 ] && echo "all idle-pause checks passed" || echo "$failures check(s) failed"
exit "$failures"
