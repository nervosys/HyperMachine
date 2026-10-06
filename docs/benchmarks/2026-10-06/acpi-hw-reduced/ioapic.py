"""Decode IOAPIC MMIO accesses (gpa 0xfec00000) per guest vCPU thread from a kvm trace.

IOREGSEL (offset 0x00) selects a register; IOWIN (0x10) reads/writes it; 0x40 is the EOI
register on version 0x20+ IOAPICs. Redirection entry N is registers 0x10+2N (low) and 0x11+2N.
"""
import collections, re, sys

line_re = re.compile(r'^\s*(.+?)-(\d+)\s+\[\d+\]\s+\S+\s+([\d.]+): kvm_mmio: mmio (\w+) len \d+ gpa (0x[0-9a-f]+) val (0x[0-9a-f]+)')
per = collections.defaultdict(list)
comm_of = {}
for line in open(sys.argv[1], encoding='utf-8', errors='replace'):
    m = line_re.match(line)
    if not m:
        continue
    comm, tid, ts, op, gpa, val = m.groups()
    gpa = int(gpa, 16)
    if gpa & ~0xfff != 0xfec00000:
        continue
    comm_of[tid] = comm.strip()
    per[tid].append((float(ts), op, gpa & 0xfff, int(val, 16)))

for tid, ev in per.items():
    if len(ev) < 20:
        continue
    t0 = ev[0][0]
    print(f'== {comm_of[tid]} tid {tid}: {len(ev)} IOAPIC accesses over {1000*(ev[-1][0]-t0):.0f} ms')
    sel = None
    kinds = collections.Counter()
    by_rte = collections.Counter()
    eoi = 0
    timeline = collections.Counter()
    for ts, op, off, val in ev:
        timeline[int((ts - t0) * 1000) // 50 * 50] += 1
        if off == 0x00 and op == 'write':
            sel = val & 0xff
            kinds['select'] += 1
        elif off == 0x10:
            if sel is not None and sel >= 0x10:
                rte = (sel - 0x10) // 2
                half = 'lo' if (sel - 0x10) % 2 == 0 else 'hi'
                by_rte[f'RTE{rte}.{half} {op}'] += 1
                if op == 'write' and half == 'lo':
                    kinds['rte-lo write masked' if val & 0x10000 else 'rte-lo write unmasked'] += 1
            else:
                kinds[f'reg{sel} {op}'] += 1
        elif off == 0x40:
            eoi += 1
        else:
            kinds[f'off{off:#x} {op}'] += 1
    print('  kinds:', dict(kinds), 'EOI writes:', eoi)
    print('  busiest RTE accesses:', by_rte.most_common(8))
    print('  accesses per 50 ms from first:', sorted(timeline.items())[:20])
    break  # one guest is representative
