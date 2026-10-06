#!/usr/bin/env bash
# Count KVM exits per guest boot (kvm_exit/kvm_pio/kvm_mmio tracepoints) for one
# HyperMachine daemon variant ($1 = baseline|candidate) and Firecracker as control.
# Exit counts and memory do not depend on host load; latency does.
set -u
T=/sys/kernel/tracing
out=/var/tmp/hm-thp-trace-$1
mkdir -p $out
echo 0 > $T/tracing_on
echo > $T/trace
echo 262144 > $T/buffer_size_kb
echo 0 > $T/events/enable
echo 1 > $T/events/kvm/kvm_exit/enable
echo 1 > $T/events/kvm/kvm_pio/enable
echo 1 > $T/events/kvm/kvm_mmio/enable
echo "other KVM users before: $(ls -l /proc/*/fd 2>/dev/null | grep -c kvm-vm)"
# Sample host AnonHugePages while the bench runs, to see whether THP is used.
( while :; do grep AnonHugePages /proc/meminfo; sleep 0.2; done ) > $out/anonhuge.txt &
sampler=$!
echo 1 > $T/tracing_on
cd /path/to/HyperMachine
python3 tools/bench-local-engines.py \
    --hypermachine /var/tmp/hm-thp-$1 \
    --firecracker /var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64 \
    --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
    --initrd /var/tmp/hm-competitive/guest-output-drain.cpio.gz \
    --environment "exit-count probe, shared WSL" --pairs 3 > $out/bench.json 2> $out/bench.err
echo "bench exit: $?"
echo 0 > $T/tracing_on
kill $sampler
cp $T/trace $out/trace.txt
echo 0 > $T/events/enable
echo "trace lines: $(wc -l < $out/trace.txt); lost events: $(grep -c LOST $out/trace.txt)"
echo "max AnonHugePages during run: $(awk '{print $2}' $out/anonhuge.txt | sort -n | tail -1) kB"
python3 ./analyze.py $out/trace.txt
python3 - "$out/bench.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
for s in d['samples']:
    m = s.get('node_memory_ready') or {}
    print(f"{s['engine']:13} pair {s['pair']} ready {s['ready_ms']:.0f} ms  daemon Rss {m.get('Rss_kib','-')} kB Pss {m.get('Pss_kib','-')} kB")
EOF
