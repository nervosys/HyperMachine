"""Print one guest's port I/O and MMIO accesses in time order, collapsing repeats.

usage: window.py TRACE COMM_PREFIX START_MS END_MS
Times are relative to that vCPU thread's first traced event. IOAPIC accesses are decoded.
"""
import re, sys

path, prefix, start, end = sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4])
line_re = re.compile(r'^\s*(.+?)-(\d+)\s+\[\d+\]\s+\S+\s+([\d.]+): (kvm_exit|kvm_pio|kvm_mmio): (.*)$')
tid0 = t0 = None
sel = 0
prev, count, first_t = None, 0, 0.0


def flush():
    if prev is not None:
        print(f'{first_t:7.1f} ms  x{count:<4} {prev}')


for line in open(path, encoding='utf-8', errors='replace'):
    m = line_re.match(line)
    if not m:
        continue
    comm, tid, ts, kind, rest = m.groups()
    if not comm.strip().startswith(prefix):
        continue
    if tid0 is None:
        tid0, t0 = tid, float(ts)
    if tid != tid0 or kind == 'kvm_exit':
        continue
    t = (float(ts) - t0) * 1000
    if t > end:
        break
    if kind == 'kvm_pio':
        r = re.search(r'pio_(\w+) at (0x[0-9a-f]+) size \d+ count \d+ val (0x[0-9a-f]+)', rest)
        desc = f'pio {r.group(1)} {r.group(2)}' if r else rest
        val = int(r.group(3), 16) if r else 0
    else:
        r = re.search(r'mmio (\w+) len \d+ gpa (0x[0-9a-f]+) val (0x[0-9a-f]+)', rest)
        if not r:
            continue
        op, gpa, val = r.group(1), int(r.group(2), 16), int(r.group(3), 16)
        if gpa & ~0xfff == 0xfec00000:
            off = gpa & 0xfff
            if off == 0 and op == 'write':
                sel = val
                continue  # folded into the IOWIN access that follows
            reg = f'RTE{(sel - 0x10) // 2}.{"lo" if (sel - 0x10) % 2 == 0 else "hi"}' if sel >= 0x10 else f'reg{sel}'
            mask = ' masked' if op == 'write' and sel >= 0x10 and (sel - 0x10) % 2 == 0 and val & 0x10000 else ''
            desc = f'ioapic {op} {reg}{mask}'
        else:
            desc = f'mmio {op} {gpa:#x}'
    if t < start:
        continue
    if desc == prev:
        count += 1
    else:
        flush()
        prev, count, first_t = desc, 1, t
flush()
