#!/usr/bin/env bash
# End-to-end check of checkpoints on real KVM guests: save a running
# sandbox, change it, and roll it back in place.
#
#   HV2_KERNEL=bzImage HV2_INITRD=guest.cpio.gz tools/e2e-checkpoints.sh path/to/hv2-sandboxd
#
# Needs /dev/kvm, curl and jq. Exits non-zero if any check fails.
#
# What it asserts: a restore brings back the filesystem *and* memory as they
# were -- a file edited, a file created and a process killed after the
# checkpoint are all as they were before -- under the same sandbox ID and
# access token, with the proxy routing to the restored guest. A checkpoint
# can be restored more than once; names are unique; a sandbox keeps at most
# ten; a deleted one cannot be restored; a paused sandbox must be resumed
# first.
#
# Uses ports 13985/13986, not the daemon's defaults, so it can run beside one.
set -u
bin=${1:?usage: $0 path/to/hv2-sandboxd}
: "${HV2_KERNEL:?set HV2_KERNEL}" "${HV2_INITRD:?set HV2_INITRD}"
export HV2_KERNEL HV2_INITRD

api=http://127.0.0.1:13985
proxy=127.0.0.1:13986
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
run() {
    curl -s -XPOST "$api/sandboxes/$1/exec" -H content-type:application/json \
        -d "$(jq -n --arg c "$2" '{cmd: $c, timeout_secs: 20}')" |
        jq -r '(.stdout // "") + (.stderr // "")'
}
code() { # code METHOD PATH [BODY]: a JSON content type only with a body
    if [ $# -ge 3 ]; then
        curl -s -o /dev/null -w '%{http_code}' -X"$1" "$api$2" -H content-type:application/json -d "$3"
    else
        curl -s -o /dev/null -w '%{http_code}' -X"$1" "$api$2"
    fi
}
# How many `sleep 7777` are running. The guest's busybox has no pgrep, and
# the bracket keeps this command's own line from matching itself.
sleepers='ps | grep -c "[s]leep 777[7]"'


"$bin" --port 13985 --proxy-port 13986 --capacity 4 >"$work/daemon.log" 2>&1 &
daemon=$!
for _ in $(seq 1 120); do curl -sf "$api/metrics" >/dev/null && break; sleep 0.5; done
curl -sf "$api/metrics" >/dev/null || { echo "FAIL  daemon did not come up:"; tail -20 "$work/daemon.log"; exit 1; }

created=$(curl -s -XPOST "$api/sandboxes" -H content-type:application/json \
    -d '{"templateID":"base","timeout":600,"envVars":{"MARK":"env-kept"}}')
sbx=$(printf '%s' "$created" | jq -r '.sandboxID')
token=$(printf '%s' "$created" | jq -r '.envdAccessToken')
check "created" '^sbx-' "$sbx"

run "$sbx" 'echo before > /root/a.txt; setsid sleep 7777 </dev/null >/dev/null 2>&1 &' >/dev/null
check "a process is running before the checkpoint" '^1$' "$(run "$sbx" "$sleepers")"
check "save a checkpoint" '^201$' "$(code POST "/sandboxes/$sbx/checkpoints" '{"name":"before"}')"
check "the same name again is refused" '^409$' "$(code POST "/sandboxes/$sbx/checkpoints" '{"name":"before"}')"
check "a bad name is refused" '^400$' "$(code POST "/sandboxes/$sbx/checkpoints" '{"name":"no/slash"}')"
check "it is listed" '^before$' "$(curl -s "$api/sandboxes/$sbx/checkpoints" | jq -r '.[].name')"

run "$sbx" 'echo after > /root/a.txt; echo new > /root/new.txt; kill $(ps | grep "[s]leep 777[7]" | awk "{print \$1}")' >/dev/null
check "changed: the file says after" '^after$' "$(run "$sbx" 'cat /root/a.txt')"
check "changed: the process is gone" '^0$' "$(run "$sbx" "$sleepers")"

check "restore it" '^200$' "$(code POST "/sandboxes/$sbx/checkpoints/before/restore")"
check "restored: the edited file is as it was" '^before$' "$(run "$sbx" 'cat /root/a.txt')"
check "restored: the new file is gone" '^absent$' "$(run "$sbx" 'test -e /root/new.txt && echo present || echo absent')"
check "restored: the killed process runs again (memory, not just disk)" '^1$' \
    "$(run "$sbx" "$sleepers")"
check "restored: envVars are kept" '^env-kept$' "$(run "$sbx" 'echo "$MARK"')"
check "restored: same sandbox, running" '^running$' "$(curl -s "$api/sandboxes/$sbx" | jq -r '.state')"
check "restored: same access token" "^$token\$" \
    "$(curl -s -XPOST "$api/sandboxes/$sbx/connect" -H content-type:application/json -d '{"timeout":600}' | jq -r '.envdAccessToken')"
check "restored: the proxy routes to the new guest" '^[2-4][0-9][0-9]$' \
    "$(curl -s -m 20 -o /dev/null -w '%{http_code}' -H "Host: 49983-$sbx.sandbox.local" "http://$proxy/health")"

run "$sbx" 'echo again > /root/a.txt' >/dev/null
check "restore the same checkpoint twice" '^200$' "$(code POST "/sandboxes/$sbx/checkpoints/before/restore")"
check "and it is as it was again" '^before$' "$(run "$sbx" 'cat /root/a.txt')"

for i in $(seq 1 9); do code POST "/sandboxes/$sbx/checkpoints" "{\"name\":\"c$i\"}" >/dev/null; done
check "ten are kept" '^10$' "$(curl -s "$api/sandboxes/$sbx/checkpoints" | jq 'length')"
check "an eleventh is refused" '^409$' "$(code POST "/sandboxes/$sbx/checkpoints" '{"name":"c10"}')"
check "delete one" '^204$' "$(code DELETE "/sandboxes/$sbx/checkpoints/c1")"
check "a deleted checkpoint cannot be restored" '^404$' "$(code POST "/sandboxes/$sbx/checkpoints/c1/restore")"

code POST "/sandboxes/$sbx/pause" >/dev/null
check "a paused sandbox must be resumed first" '^409$' "$(code POST "/sandboxes/$sbx/checkpoints/before/restore")"
code POST "/sandboxes/$sbx/resume" '{"timeout":600}' >/dev/null
check "after a resume, restore works again" '^200$' "$(code POST "/sandboxes/$sbx/checkpoints/before/restore")"

check "killing the sandbox" '^204$' "$(code DELETE "/sandboxes/$sbx")"
check "takes its checkpoints with it" '^404$' "$(code GET "/sandboxes/$sbx/checkpoints")"

[ "$failures" -eq 0 ] && echo "all checkpoint checks passed" || echo "$failures check(s) failed"
exit "$failures"
