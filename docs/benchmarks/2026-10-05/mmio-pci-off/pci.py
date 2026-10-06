import re, collections, sys
cur = {}
acc = collections.Counter(); devs = collections.Counter(); buses = collections.Counter()
times = collections.defaultdict(list)
for line in open(sys.argv[1], errors='replace'):
    m = re.match(r'\s*(vcpu-0)-(\d+)\s+\[\d+\]\s+\S+\s+([\d.]+): kvm_pio: pio_(\w+) at (0x[0-9a-f]+) size \d+ count \d+ val (0x[0-9a-f]+)', line)
    if not m: continue
    _, tid, ts, rw, port, val = m.groups(); port = int(port, 16); val = int(val, 16)
    if port == 0xcf8 and rw == 'write':
        cur[tid] = val; times[tid].append(float(ts))
    elif 0xcfc <= port <= 0xcff and tid in cur:
        a = cur[tid]; bus = (a >> 16) & 0xff; dev = (a >> 11) & 0x1f; fn = (a >> 8) & 7
        buses[bus] += 1; devs[(bus, dev, fn)] += 1; times[tid].append(float(ts))
n = len(times)
print('guests', n, 'config accesses per guest by bus:', {b: c // n for b, c in sorted(buses.items())})
present = {k: v // n for k, v in devs.items() if v // n > 2}
print('bus/dev/fn with more than 2 data accesses per guest:', present)
print('distinct bus/dev/fn probed per guest:', len(devs))
for tid, ts in times.items():
    print(f'guest {tid}: PCI config window {1000*(max(ts)-min(ts)):.1f} ms ({len(ts)} events)')
