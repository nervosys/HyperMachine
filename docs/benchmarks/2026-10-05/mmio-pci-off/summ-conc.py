import json, glob, statistics, sys
rows = []
for f in sorted(glob.glob(sys.argv[1] + '/block*.json')):
    d = json.load(open(f))
    r = d['ready_ms']; hm, fc = r['hypermachine'], r['firecracker']
    v = f.split('-')[-1][:-5]
    rows.append((v, hm['p50'] - fc['p50'], hm['p99'] - fc['p99']))
    print(f'{f.split("/")[-1]:24} HM p50/p99 {hm["p50"]:7.1f}/{hm["p99"]:7.1f}  FC p50/p99 {fc["p50"]:7.1f}/{fc["p99"]:7.1f}  n {hm["n"]}/{fc["n"]}  ok {d["success"]}')
for v in ('baseline', 'candidate'):
    print(f'{v}: mean gap to Firecracker p50 {statistics.mean(a for w, a, b in rows if w == v):.1f} ms, p99 {statistics.mean(b for w, a, b in rows if w == v):.1f} ms')
