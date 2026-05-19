#!/usr/bin/env bash
# Workload: iperf3 throughput between host and guest, both directions.
# Env: DURATION, PARALLEL. The host runs iperf3 -s on a free port; the guest
# acts as client.
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

DURATION="${DURATION:-30}"
PARALLEL="${PARALLEL:-4}"

# Pick a free port on the host, start iperf3 server bound to it.
port=$(python3 -c "import socket; s=socket.socket(); s.bind(('0.0.0.0',0)); print(s.getsockname()[1]); s.close()")
iperf3 -s -1 -p "$port" >/dev/null 2>&1 &
srv_pid=$!
# Allow server to bind.
sleep 0.2

# Determine the host IP the guest can reach.
HOST_IP="${BENCH_HOST_GUEST_IP:-10.0.2.2}"  # QEMU user-mode default gateway

# Guest → host (TX from guest perspective).
tx_json=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "iperf3 -c $HOST_IP -p $port -t $DURATION -P $PARALLEL -J 2>/dev/null") || true
kill "$srv_pid" 2>/dev/null || true
wait "$srv_pid" 2>/dev/null || true

# Restart server for reverse direction.
iperf3 -s -1 -p "$port" >/dev/null 2>&1 &
srv_pid=$!
sleep 0.2
rx_json=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "iperf3 -c $HOST_IP -p $port -t $DURATION -P $PARALLEL -R -J 2>/dev/null") || true
kill "$srv_pid" 2>/dev/null || true
wait "$srv_pid" 2>/dev/null || true

python3 - <<PY
import json
for label, blob in (("net_g2h_Gbps", """$tx_json"""), ("net_h2g_Gbps", """$rx_json""")):
    try:
        d = json.loads(blob)
        bps = d["end"]["sum_received"]["bits_per_second"]
        print(f"{label}\t{bps/1e9:.3f}\tGbit/s")
    except Exception:
        print(f"{label}\t0\tGbit/s")
PY
