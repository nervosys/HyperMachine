#!/usr/bin/env bash
# End-to-end check of `hm jobs`: real workers, real processes, a real store.
#
#   tools/e2e-jobs.sh path/to/hm
#
# Runs on Linux and on Windows (Git Bash). Needs python and curl. Exits
# non-zero if any check fails.
#
# What it asserts:
#   1. A job runs, its stdout and stderr are kept, and HM_JOB_ID is set.
#   2. A failing job is `failed` with its exit code.
#   3. A job labelled gpu waits until a worker offering gpu takes it.
#   4. A cancel with a graceful stop creates the stop file; the program exits
#      0 by itself, and the job is `cancelled`, not `succeeded`.
#   5. A worker killed mid-job loses it: after the lease, another worker
#      requeues and finishes it, as attempt 2.
#   6. The REST mirror submits, lists, cancels and streams logs, and with a
#      token refuses requests without it.
set -u
hm=${1:?usage: $0 path/to/hm}
export MSYS_NO_PATHCONV=1
py=""
for c in python3 python; do
    if command -v "$c" >/dev/null && "$c" -c "" 2>/dev/null; then py=$(command -v "$c"); break; fi
done
[ -n "$py" ] || { echo "FAIL  no working python"; exit 1; }
# The path jobs run it by: a native one on Windows.
job_py=$py
command -v cygpath >/dev/null && job_py=$(cygpath -m "$py")
work=$(mktemp -d)
# Native programs on Windows need a Windows path, not an MSYS one.
command -v cygpath >/dev/null && work=$(cygpath -m "$work")
store="$work/store"
failures=0
pids=()
# Kill a background process for certain. Under Git Bash, $! is an MSYS pid,
# and kill -9 does not reliably reach the native program behind it.
hard_kill() {
    if [ -r "/proc/$1/winpid" ]; then
        taskkill //F //PID "$(cat "/proc/$1/winpid")" >/dev/null 2>&1
    fi
    kill -9 "$1" 2>/dev/null
}
cleanup() {
    for p in "${pids[@]}"; do hard_kill "$p"; done
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
field() { # field JSON KEY
    printf '%s' "$1" | "$py" -c "import json,sys; v=json.load(sys.stdin).get('$2'); print('' if v is None else v)"
}
state() { field "$("$hm" jobs --store "$store" status "$1")" state; }
wait_for() { # wait_for ID STATE-REGEX SECONDS
    for _ in $(seq 1 $(($3 * 4))); do
        state "$1" | grep -qE "$2" && return 0
        sleep 0.25
    done
    return 1
}
# A job spec, as JSON, running COMMAND (a Python one-liner) with extra JSON fields.
spec() { # spec PYTHON-CODE [EXTRA-JSON-FIELDS]
    "$py" - "$1" "${2:-}" "$job_py" <<'EOF'
import json, sys
code, extra, py = sys.argv[1], sys.argv[2], sys.argv[3]
spec = {"command": [py, "-c", code], "sandbox": {"net": "host"}}
if extra:
    spec.update(json.loads(extra))
print(json.dumps(spec))
EOF
}
submit() { spec "$@" >"$work/spec.json" && "$hm" jobs --store "$store" submit "$work/spec.json"; }
worker() { # worker ARGS...: in the background; its pid in $last_worker.
    # Not called as $(worker ...): a subshell would lose the pid list.
    "$hm" jobs --store "$store" worker "$@" >>"$work/worker.log" 2>&1 &
    last_worker=$!
    pids+=("$last_worker")
}

worker --concurrency 2 --lease-secs 15

ok=$(submit 'import os,sys; print("hello"); print("to stderr", file=sys.stderr); print(os.environ["HM_JOB_ID"])')
wait_for "$ok" succeeded 60
check "a job runs to succeeded" '^succeeded$' "$(state "$ok")"
check "its stdout is kept" 'hello' "$("$hm" jobs --store "$store" logs "$ok")"
check "its stderr is kept" 'to stderr' "$("$hm" jobs --store "$store" logs --stderr "$ok")"
check "it knows its own ID" "$ok" "$("$hm" jobs --store "$store" logs "$ok")"

bad=$(submit 'import sys; sys.exit(3)')
wait_for "$bad" failed 60
check "a failing job is failed" '^failed$' "$(state "$bad")"
check "with its exit code" '^3$' "$(field "$("$hm" jobs --store "$store" status "$bad")" exit_code)"

gpu=$(submit 'print("on the gpu box")' '{"labels":["gpu"]}')
sleep 3
check "a gpu job waits for a gpu worker" '^queued$' "$(state "$gpu")"
worker --labels gpu,cpu --lease-secs 15
wait_for "$gpu" succeeded 60
check "and a gpu worker runs it" '^succeeded$' "$(state "$gpu")"

stopfile="$work/STOP"
graceful=$(submit "import os,time
while not os.path.exists(r'$stopfile'): time.sleep(0.1)
print('stopped cleanly')" "{\"graceful_stop\":{\"create_file\":\"$(printf '%s' "$stopfile" | sed 's/\\/\\\\/g')\",\"grace_secs\":60}}")
wait_for "$graceful" running 60
"$hm" jobs --store "$store" cancel "$graceful" >/dev/null
wait_for "$graceful" cancelled 60
check "a graceful stop creates its stop file" 'yes' "$([ -e "$stopfile" ] && echo yes)"
check "the program stopped by itself" 'stopped cleanly' "$("$hm" jobs --store "$store" logs "$graceful")"
check "and the job is cancelled, with exit 0" '^cancelled 0$' \
    "$(state "$graceful") $(field "$("$hm" jobs --store "$store" status "$graceful")" exit_code)"

# Only one worker from here on, so the kill below takes the job it holds.
for p in "${pids[@]}"; do hard_kill "$p"; done
pids=()
worker --lease-secs 15
lone=$last_worker
long=$(submit 'import time; time.sleep(8); print("finished")' '{"max_attempts":2}')
wait_for "$long" running 60
hard_kill "$lone"
sleep 1
worker --lease-secs 15
wait_for "$long" succeeded 90
check "a job whose worker died is finished by another" '^succeeded$' "$(state "$long")"
check "as attempt 2" '^2$' "$(field "$("$hm" jobs --store "$store" status "$long")" attempts)"

"$hm" jobs --store "$store" serve --addr 127.0.0.1:17878 --token t0ken >>"$work/serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 40); do curl -s -o /dev/null http://127.0.0.1:17878/api/v1/jobs && break; sleep 0.25; done
api=http://127.0.0.1:17878/api/v1/jobs
check "REST: no token is refused" '^401$' "$(curl -s -o /dev/null -w '%{http_code}' $api)"
spec 'print("via rest")' >"$work/rest.json"
rid=$(field "$(curl -s -XPOST $api -H 'authorization: Bearer t0ken' -H content-type:application/json --data-binary @"$work/rest.json")" id)
check "REST: a job is submitted" '^j' "$rid"
wait_for "$rid" succeeded 60
check "REST: listing finds it" "$rid" "$(curl -s "$api?state=succeeded" -H 'authorization: Bearer t0ken')"
check "REST: its log streams" 'via rest' "$(curl -s -m 10 "$api/$rid/logs?follow=true" -H 'authorization: Bearer t0ken')"
q=$(submit 'print(1)' '{"labels":["nobody-has-this"]}')
check "REST: a queued job cancels" 'cancelled' "$(curl -s -XPOST "$api/$q/cancel" -H 'authorization: Bearer t0ken')"

[ "$failures" -eq 0 ] && echo "all jobs checks passed" || { echo "$failures check(s) failed"; tail -20 "$work/worker.log"; }
exit "$failures"
