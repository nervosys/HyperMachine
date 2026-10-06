#!/usr/bin/env bash
# Count KVM exits per guest boot for HyperMachine and Firecracker, using the
# host kernel's kvm_exit tracepoint: one instrument for both engines, and a
# count does not depend on how loaded the host is.
set -u
T=/sys/kernel/tracing
out=/var/tmp/hm-exit-trace-$1
mkdir -p $out
echo 0 > $T/tracing_on
echo > $T/trace
echo 262144 > $T/buffer_size_kb
echo 0 > $T/events/enable
echo 1 > $T/events/kvm/kvm_exit/enable
echo 1 > $T/events/kvm/kvm_pio/enable
echo 1 > $T/events/kvm/kvm_mmio/enable
echo "other KVM users before: $(ls -l /proc/*/fd 2>/dev/null | grep -c kvm-vm)"
echo 1 > $T/tracing_on
cd "$(git rev-parse --show-toplevel)"
python3 tools/bench-local-engines.py \
    --hypermachine /var/tmp/hm-pcioff-$1 \
    --firecracker /var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64 \
    --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
    --initrd /var/tmp/hm-competitive/guest-output-drain.cpio.gz \
    --environment "exit-count probe, shared WSL" --pairs 3 > $out/bench.json 2> $out/bench.err
echo "bench exit: $?"
echo 0 > $T/tracing_on
cp $T/trace $out/trace.txt
echo 0 > $T/events/enable
echo "trace lines: $(wc -l < $out/trace.txt); lost events: $(grep -c LOST $out/trace.txt)"
