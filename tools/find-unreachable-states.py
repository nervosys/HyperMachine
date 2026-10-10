"""Find pub enum variants that a guard requires and nothing ever constructs.

The class `VCpuState::Running` belongs to: a state a guard demands before an
operation may proceed, which nothing in the tree ever writes. The guarded
operation then cannot succeed, ever, and rustc says nothing because the enum
is `pub` -- dead_code does not reason about public variants.

Two rules, and the second one is what the first pass got wrong:

  A `match` arm is NOT construction. `X::V => ...` maps a value that already
  exists; it is evidence of nothing.

  `Self::V` inside the file that defines the enum IS construction. A wire
  decoder writes `8 => Ok(Self::Q8_0)`, and a detector that misses that
  reports every decoded enum in the tree. This was the first pass's whole
  false-positive population.

Still imprecise where one file defines two enums sharing a variant name; that
direction is conservative (it suppresses, rather than invents, findings).
Every hit is read by hand before it is believed.
"""

import os
import re
from collections import defaultdict

ROOT = 'crates'

enum_re = re.compile(r'pub enum (\w+)\s*\{')
variant_re = re.compile(r'^\s*([A-Z]\w*)\s*(?:\{|\(|=|,|$)')

sources = []
for base, dirs, files in os.walk(ROOT):
    dirs[:] = [d for d in dirs if d not in ('target', '.git')]
    for f in files:
        if f.endswith('.rs'):
            path = os.path.join(base, f)
            try:
                sources.append((path, open(path, encoding='utf-8').read()))
            except Exception:
                pass

# One separator and a sorted order, so every platform walks the tree alike.
# Enum names repeat across files (32 of them); with an unsorted walk the last
# definition seen won, and that differed between Linux and Windows.
sources = sorted((p.replace(os.sep, '/'), x) for p, x in sources)

enums = {}
defined_in = defaultdict(list)          # path -> [enum names]
for path, text in sources:
    lines = text.split('\n')
    for i, line in enumerate(lines):
        m = enum_re.search(line)
        if not m:
            continue
        name = m.group(1)
        variants, depth = [], 0
        for j in range(i, min(i + 200, len(lines))):
            depth += lines[j].count('{') - lines[j].count('}')
            if j > i:
                vm = variant_re.match(lines[j])
                if vm:
                    variants.append(vm.group(1))
            if depth <= 0 and j > i:
                break
        if variants:
            if name in enums:
                # Same name, another file: keep the first in sorted order,
                # as before but now the same on every platform.
                continue
            enums[name] = (path, variants)
            defined_in[path].append(name)

built = defaultdict(int)
guarded = defaultdict(int)

for path, text in sources:
    local = defined_in.get(path, [])
    for line in text.split('\n'):
        if line.strip().startswith('//'):
            continue
        for m in re.finditer(r'\b(\w+)::(\w+)\b', line):
            qualifier, variant = m.group(1), m.group(2)

            if qualifier == 'Self':
                # Resolve against an enum this very file defines.
                owners = [e for e in local if variant in enums[e][1]]
                if len(owners) != 1:
                    continue
                enum_name = owners[0]
            else:
                enum_name = qualifier
                if enum_name not in enums or variant not in enums[enum_name][1]:
                    continue

            key = (enum_name, variant)
            before = line[:m.start()].rstrip()
            after = line[m.end():].lstrip()

            if before.endswith('==') or before.endswith('!='):
                guarded[key] += 1
            elif after.startswith('=>') or after.startswith('|'):
                pass                      # a match arm maps; it does not create
            else:
                built[key] += 1

# Reviewed and accepted, each with the reason it is not a defect. The same
# convention as `SKIP` in tools/sweep.sh, and for the same reason: this is the
# one place a real finding could hide, so an entry that does not say why is an
# entry nobody can check.
ACCEPTED = {
    ('Egress', 'Host'): (
        'hv2-sandbox request: serde builds it from `"egress": "host"` in a JSON '
        'request, which this script cannot see. Its guard refuses a request that '
        'also lists allowed hosts, and a test sends one and reads the refusal.'
    ),
    ('ApiRole', 'Operator'): (
        'The #[default] role: serde builds it for every key policy that names no '
        'role, which this script cannot see. Its guard (operator admin) is reachable.'
    ),
    ('VCpuState', 'Running'): (
        'True, and known. Nothing ever marks a vCPU Running, so VM::pause could '
        'never succeed -- it now returns NotSupported and says so. The variant '
        'stays because implementing suspend is still open; see docs/handoff.html.'
    ),
    ('PortState', 'Learning'): (
        'The one guard (vswitch can_learn) accepts Learning OR Forwarding, and '
        'ports start Forwarding, so it passes. The STP Learning state is not '
        'modelled. Hidden until 2026-09-29 by a walk-order bug that made this '
        'script analyse different same-named enums on Linux and Windows.'
    ),
    ('StoreBackend', 'File'): (
        'Public config field. `StoreConfig.backend` defaults to Memory and a '
        'caller sets File; the guard reads a choice made outside this crate.'
    ),
    ('StoreBackend', 'External'): 'As StoreBackend::File.',
    ('Report', 'None'): (
        'hm sandbox run --report none: clap builds it from the command line '
        '(ValueEnum), which this script cannot see.'
    ),
    ('MigrationStage', 'PostCopy'): (
        'Post-copy migration is declared and unimplemented. The controller drives '
        'Idle -> Setup -> PreCopy -> StopAndCopy -> Completed and never enters it. '
        'Harmless as written: the one guard *permits* PostCopy rather than '
        'requiring it, so nothing is blocked by its absence.'
    ),
}

findings = [
    (name, v, guarded[(name, v)], enums[name][0])
    for name, (path, variants) in sorted(enums.items())
    for v in variants
    if guarded[(name, v)] > 0 and built[(name, v)] == 0
]

fresh = [f for f in findings if (f[0], f[1]) not in ACCEPTED]

print('enums scanned:', len(enums))
print('variants a guard requires and nothing constructs: %d (%d reviewed, %d new)'
      % (len(findings), len(findings) - len(fresh), len(fresh)))
print()

for name, v, hits, path in findings:
    mark = '    ' if (name, v) in ACCEPTED else 'NEW '
    print('%s%-34s guard x%-2d  %s' % (mark, name + '::' + v, hits, path))

if fresh:
    print()
    print('Each NEW line is a guard that can never pass, or a false positive this')
    print('script cannot see through. Read it, then fix it or add it to ACCEPTED')
    print('with the reason. Do not add one without a reason.')

raise SystemExit(1 if fresh else 0)
