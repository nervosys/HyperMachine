#!/usr/bin/env bash
set -u
c=$1; pairs=$2
out=/var/tmp/hm-pcioff-abba-c$c
mkdir -p $out
cd "$(git rev-parse --show-toplevel)"
i=0
for v in baseline candidate candidate baseline; do
    i=$((i+1))
    python3 tools/bench-local-engines-concurrent.py \
        --hypermachine /var/tmp/hm-pcioff-$v \
        --firecracker /var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64 \
        --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
        --initrd /var/tmp/hm-competitive/guest-output-drain.cpio.gz \
        --environment "pci=off C$c ABBA block $i ($v), shared WSL" --pairs $pairs --concurrency $c \
        > $out/block$i-$v.json 2> $out/block$i-$v.err
    echo "C$c block $i $v exit $?"
done
python3 - "$out" <<'PY'
import json, glob, statistics, sys
rows = []
for f in sorted(glob.glob(sys.argv[1] + '/block*.json')):
    d = json.load(open(f))
    def ready(engine):
        return [s['ready_ms'] for s in d['samples'] if s.get('engine') == engine and s.get('success')]
    hm, fc = ready('hypermachine'), ready('firecracker')
    tot = sum(1 for s in d['samples'] if s.get('engine') == 'hypermachine'), sum(1 for s in d['samples'] if s.get('engine') == 'firecracker')
    v = f.split('-')[-1][:-5]
    q = lambda xs, p: sorted(xs)[min(len(xs) - 1, int(p * len(xs)))]
    rows.append((v, statistics.median(hm) - statistics.median(fc)))
    print(f'{f.split("/")[-1]:24} HM p50/p99 {statistics.median(hm):7.1f}/{q(hm,.99):7.1f}  FC p50/p99 {statistics.median(fc):7.1f}/{q(fc,.99):7.1f}  passed {len(hm)}/{tot[0]} {len(fc)}/{tot[1]}  ok {d.get("success")}')
for v in ('baseline', 'candidate'):
    print(f'{v}: mean p50 gap to Firecracker {statistics.mean(g for w, g in rows if w == v):.1f} ms')
PY
