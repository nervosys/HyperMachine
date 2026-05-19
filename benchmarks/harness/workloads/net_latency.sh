#!/usr/bin/env bash
# Workload: netperf TCP_RR for round-trip latency (1B request / 1B response).
set -euo pipefail
source "$(dirname "$0")/../lib/ssh.sh"

DURATION="${DURATION:-30}"
HOST_IP="${BENCH_HOST_GUEST_IP:-10.0.2.2}"

# Start netserver on host (auto-port via -p).
port=$(python3 -c "import socket; s=socket.socket(); s.bind(('0.0.0.0',0)); print(s.getsockname()[1]); s.close()")
netserver -p "$port" -L 0.0.0.0 >/dev/null 2>&1 || true
trap 'pkill -f "netserver -p $port" 2>/dev/null || true' EXIT
sleep 0.2

out=$(ssh_run "$BENCH_SSH_USER" "$BENCH_SSH_KEY" "$BENCH_SSH_HOST" "$BENCH_SSH_PORT" \
    "netperf -H $HOST_IP -p $port -t TCP_RR -l $DURATION -- -O 'min_latency,mean_latency,p50_latency,p99_latency,trans_rate'")

# netperf prints space-separated values on the last line.
line=$(echo "$out" | awk 'NF{l=$0} END{print l}')
min=$(echo "$line" | awk '{print $1}')
mean=$(echo "$line" | awk '{print $2}')
p50=$(echo "$line" | awk '{print $3}')
p99=$(echo "$line" | awk '{print $4}')
rate=$(echo "$line" | awk '{print $5}')

printf "net_rr_min_us\t%s\tus\n"  "$min"
printf "net_rr_mean_us\t%s\tus\n" "$mean"
printf "net_rr_p50_us\t%s\tus\n"  "$p50"
printf "net_rr_p99_us\t%s\tus\n"  "$p99"
printf "net_rr_rate_tps\t%s\ttrans/s\n" "$rate"
