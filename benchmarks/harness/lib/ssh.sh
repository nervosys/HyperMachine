#!/usr/bin/env bash
# benchmarks/harness/lib/ssh.sh — minimal idempotent SSH helpers.
set -euo pipefail

# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

_ssh_opts=(
    -o StrictHostKeyChecking=no
    -o UserKnownHostsFile=/dev/null
    -o LogLevel=ERROR
    -o ConnectTimeout=3
    -o ServerAliveInterval=5
)

ssh_ready() {
    # ssh_ready <user> <key> <host> <port> -> 0 if connect+exec works
    local user="$1" key="$2" host="$3" port="$4"
    ssh "${_ssh_opts[@]}" -i "$key" -p "$port" "${user}@${host}" true >/dev/null 2>&1
}

ssh_wait() {
    # ssh_wait <user> <key> <host> <port> <timeout_s> -> elapsed_s on stdout (or 124)
    local user="$1" key="$2" host="$3" port="$4" timeout="$5"
    local start=$(date +%s.%N) now elapsed
    while :; do
        if ssh_ready "$user" "$key" "$host" "$port"; then
            now=$(date +%s.%N)
            python3 -c "print(f'{$now - $start:.3f}')"
            return 0
        fi
        now=$(date +%s.%N)
        elapsed=$(python3 -c "print(int($now - $start))")
        if (( elapsed >= timeout )); then
            return 124
        fi
        sleep 0.25
    done
}

ssh_run() {
    # ssh_run <user> <key> <host> <port> <command...>
    local user="$1" key="$2" host="$3" port="$4"; shift 4
    ssh "${_ssh_opts[@]}" -i "$key" -p "$port" "${user}@${host}" "$@"
}

scp_to() {
    # scp_to <user> <key> <host> <port> <local> <remote>
    local user="$1" key="$2" host="$3" port="$4" src="$5" dst="$6"
    scp "${_ssh_opts[@]}" -P "$port" -i "$key" "$src" "${user}@${host}:${dst}" >/dev/null
}

scp_from() {
    local user="$1" key="$2" host="$3" port="$4" src="$5" dst="$6"
    scp "${_ssh_opts[@]}" -P "$port" -i "$key" "${user}@${host}:${src}" "$dst" >/dev/null
}
