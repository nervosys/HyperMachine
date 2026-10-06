import collections, re, sys
path = sys.argv[1]
line_re = re.compile(r'^\s*(.+?)-(\d+)\s+\[\d+\]\s+\S+\s+([\d.]+): (kvm_exit|kvm_pio|kvm_mmio): (.*)$')
by_tid = collections.defaultdict(lambda: {'comm': None, 'exits': collections.Counter(),
                                           'pio': collections.Counter(), 'mmio': collections.Counter(),
                                           'first': None, 'last': None})
for line in open(path, encoding='utf-8', errors='replace'):
    m = line_re.match(line)
    if not m:
        continue
    comm, tid, ts, kind, rest = m.groups()
    t = by_tid[tid]
    t['comm'] = comm.strip()
    ts = float(ts)
    t['first'] = t['first'] or ts
    t['last'] = ts
    if kind == 'kvm_exit':
        r = re.search(r'reason (\S+)', rest)
        t['exits'][r.group(1) if r else '?'] += 1
    elif kind == 'kvm_pio':
        r = re.search(r'pio_(\w+) at (0x[0-9a-f]+)', rest)
        t['pio'][f'{r.group(1)} {r.group(2)}'] += 1
    else:
        r = re.search(r'mmio (\w+) len \d+ gpa (0x[0-9a-f]+)', rest)
        if r:
            gpa = int(r.group(2), 16)
            region = f'{r.group(1)} {gpa & ~0xfff:#x}'
            t['mmio'][region] += 1


def engine(comm):
    return 'firecracker' if comm.startswith('fc_vcpu') else 'hypermachine' if comm.startswith('vcpu') else None


guests = collections.defaultdict(list)
for tid, t in by_tid.items():
    e = engine(t['comm'])
    if e and sum(t['exits'].values()) > 1000:
        guests[e].append(t)
for e, ts in guests.items():
    totals = [sum(t['exits'].values()) for t in ts]
    print(f'== {e}: {len(ts)} guests, exits per guest {totals}, traced span ms {[round((t["last"]-t["first"])*1000) for t in ts]}')
    reasons = collections.Counter()
    pio = collections.Counter()
    mmio = collections.Counter()
    for t in ts:
        reasons.update(t['exits']); pio.update(t['pio']); mmio.update(t['mmio'])
    n = len(ts)
    print('  exits by reason, per guest:', ', '.join(f'{k} {v/n:.0f}' for k, v in reasons.most_common(12)))
    print('  port I/O, per guest:', ', '.join(f'{k} {v/n:.0f}' for k, v in pio.most_common(10)))
    print('  MMIO pages, per guest:', ', '.join(f'{k} {v/n:.0f}' for k, v in mmio.most_common(10)))
