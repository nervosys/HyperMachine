#!/usr/bin/env bash
# Interleaved A/B of single-guest cold creation: baseline/candidate/candidate/baseline,
# each block measuring HyperMachine against Firecracker on the same host.
set -u
out=/var/tmp/hm-pcioff-abba
mkdir -p $out
cd "$(git rev-parse --show-toplevel)"
i=0
for v in baseline candidate candidate baseline; do
    i=$((i+1))
    python3 tools/bench-local-engines.py \
        --hypermachine /var/tmp/hm-pcioff-$v \
        --firecracker /var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64 \
        --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
        --initrd /var/tmp/hm-competitive/guest-output-drain.cpio.gz \
        --environment "pci=off ABBA block $i ($v), shared WSL" --pairs 10 > $out/block$i-$v.json 2> $out/block$i-$v.err
    echo "block $i $v exit $?"
done
python3 - <<'EOF'
import json, glob, statistics
rows = []
for f in sorted(glob.glob('/var/tmp/hm-pcioff-abba/block*.json')):
    d = json.load(open(f))
    hm = [s['ready_ms'] for s in d['samples'] if s['engine'] == 'hypermachine' and s['success']]
    fc = [s['ready_ms'] for s in d['samples'] if s['engine'] == 'firecracker' and s['success']]
    v = f.split('-')[-1][:-5]
    rows.append((v, statistics.median(hm), statistics.median(fc), len(hm), len(fc), d['success']))
    print(f'{f.split("/")[-1]:24} HM p50 {statistics.median(hm):7.1f}  FC p50 {statistics.median(fc):7.1f}  gap {statistics.median(hm)-statistics.median(fc):6.1f}  n {len(hm)}/{len(fc)}  ok {d["success"]}')
for v in ('baseline', 'candidate'):
    g = [r[1] - r[2] for r in rows if r[0] == v]
    print(f'{v}: mean of block gaps to Firecracker {statistics.mean(g):.1f} ms')
EOF
