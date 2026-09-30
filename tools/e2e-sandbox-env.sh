#!/usr/bin/env bash
# End-to-end check of E2B's envVars on real KVM guests, created through the
# E2B API.
#
#   HV2_KERNEL=bzImage HV2_INITRD=guest.cpio.gz tools/e2e-sandbox-env.sh path/to/hv2-sandboxd
#
# The initramfs needs a guest agent from this tree or later (one that reads
# the template defaults at every command). Needs /dev/kvm, curl and jq.
# Exits non-zero if any check fails.
#
# What it asserts: a sandbox created with envVars has them in every command
# it runs, and keeps them through a pause and resume, a fork, and a snapshot
# another sandbox is created from -- because they live in the guest, not on
# the host. And no API response carries a value back out.
#
# Uses ports 13990/13991, not the daemon's defaults, so it can run beside one.
set -u
bin=${1:?usage: $0 path/to/hv2-sandboxd}
: "${HV2_KERNEL:?set HV2_KERNEL}" "${HV2_INITRD:?set HV2_INITRD}"
export HV2_KERNEL HV2_INITRD

api=http://127.0.0.1:13990
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
refute() { # refute NAME PATTERN ACTUAL
    if printf '%s' "$3" | grep -qE "$2"; then
        echo "FAIL  $1: /$2/ found in: $3"
        failures=$((failures + 1))
    else
        echo "ok    $1"
    fi
}
run() {
    curl -s -XPOST "$api/sandboxes/$1/exec" -H content-type:application/json \
        -d "$(jq -n --arg c "$2" '{cmd: $c, timeout_secs: 30}')" |
        jq -r '(.stdout // "") + (.stderr // "")'
}

"$bin" --port 13990 --proxy-port 13991 --capacity 6 >"$work/daemon.log" 2>&1 &
daemon=$!
for _ in $(seq 1 120); do curl -sf "$api/metrics" >/dev/null && break; sleep 0.5; done
curl -sf "$api/metrics" >/dev/null || { echo "FAIL  daemon did not come up:"; tail -20 "$work/daemon.log"; exit 1; }

secret="s3cr3t-$RANDOM$RANDOM"
created=$(curl -s -XPOST "$api/sandboxes" -H content-type:application/json -d \
    "{\"templateID\":\"base\",\"timeout\":300,\"envVars\":{\"GREETING\":\"hello world\",\"API_KEY\":\"$secret\"}}")
sbx=$(printf '%s' "$created" | jq -r '.sandboxID')
check "create: accepted" '^[a-z0-9-]+$' "$sbx"
refute "create: the response does not carry a value" "$secret" "$created"

check "a command sees envVars" '^hello world$' "$(run "$sbx" 'echo "$GREETING"')"
check "a second variable, too" "^$secret\$" "$(run "$sbx" 'echo "$API_KEY"')"
check "the rest of the environment is intact" '/bin' "$(run "$sbx" 'echo "$PATH"')"
refute "detail does not carry a value" "$secret" "$(curl -s "$api/sandboxes/$sbx")"
refute "listing does not carry a value" "$secret" "$(curl -s "$api/sandboxes")"

bad=$(curl -s -o /dev/null -w '%{http_code}' -XPOST "$api/sandboxes" -H content-type:application/json \
    -d '{"templateID":"base","timeout":30,"envVars":{"A=B":"x"}}')
check "a name with '=' is refused" '^400$' "$bad"

curl -s -XPOST "$api/sandboxes/$sbx/pause" >/dev/null
curl -s -XPOST "$api/sandboxes/$sbx/resume" -H content-type:application/json -d '{"timeout":300}' >/dev/null
check "after pause and resume" '^hello world$' "$(run "$sbx" 'echo "$GREETING"')"

fork=$(curl -s -XPOST "$api/sandboxes/$sbx/fork" -H content-type:application/json -d '{"count":1}' |
    jq -r '.[0].sandbox.sandboxID')
check "fork: created" '^[a-z0-9-]+$' "$fork"
check "a fork has them" '^hello world$' "$(run "$fork" 'echo "$GREETING"')"

snap=$(curl -s -XPOST "$api/sandboxes/$sbx/snapshots" -H content-type:application/json \
    -d '{"name":"env-e2e"}' | jq -r '.snapshotID')
from=$(curl -s -XPOST "$api/sandboxes" -H content-type:application/json \
    -d "{\"templateID\":\"$snap\",\"timeout\":120}" | jq -r '.sandboxID')
check "a sandbox from its snapshot has them" '^hello world$' "$(run "$from" 'echo "$GREETING"')"

plain=$(curl -s -XPOST "$api/sandboxes" -H content-type:application/json \
    -d '{"templateID":"base","timeout":60}' | jq -r '.sandboxID')
check "a sandbox without envVars does not" '^unset$' "$(run "$plain" 'echo "${GREETING-unset}"')"

[ "$failures" -eq 0 ] && echo "all envVars checks passed" || echo "$failures check(s) failed"
exit "$failures"
