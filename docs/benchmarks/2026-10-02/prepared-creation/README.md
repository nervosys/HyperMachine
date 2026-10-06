# Prepared restore creation-stage diagnostics

The unchanged accepted daemon's existing debug logs split successful creation
into build, snapshot launch, agent answering and network/envd setup. All 1600
C100 restores pass across two fresh cohorts of four alternating HM/FC pairs,
plus four smoke restores. Cleanup, prepared sources and input hashes verify.
All 802 successful HyperMachine children match exactly one creation trace and
one successful readiness trace. Each cohort retains its preparation-parent
trace explicitly as an uncorrelated identity.

These are logging-enabled diagnostics on shared nested WSL/KVM, not ranking
cohorts or a managed competitor comparison. Host background load is not
controlled. Firecracker receives the matched guest state, resource allocation,
clock/RNG `Restored` maintenance and state command, but its fresh VMM/Unix API
differs from HyperMachine's persistent daemon/HTTP path. Logging can perturb
timing; variation from earlier evidence cannot be attributed to a single cause.

| Cohort | Engine | Passed | Ready P50 ms | Ready P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---:|---:|---:|---:|---:|
| C100 | HyperMachine | 400/400 | 4371.722 | 9743.606 | 335.914 | 254.688 |
| C100 | Firecracker | 400/400 | 1624.202 | 3904.115 | 297.352 | 297.352 |
| C100 repeat | HyperMachine | 400/400 | 1485.546 | 6606.737 | 336.702 | 257.678 |
| C100 repeat | Firecracker | 400/400 | 1395.313 | 1678.759 | 273.139 | 273.139 |

| HyperMachine component | First mean ms | Repeat mean ms | First fraction of total time | Repeat fraction of total time |
|---|---:|---:|---:|---:|
| Build | 59.062 | 24.719 | 1.31% | 0.88% |
| Snapshot launch | 93.812 | 70.982 | 2.08% | 2.53% |
| Agent answering | 1835.306 | 1093.953 | 40.76% | 38.98% |
| Network/envd setup | 4.573 | 0.891 | 0.10% | 0.03% |
| Outside logged creation | 1043.582 | 902.885 | 23.18% | 32.17% |
| Client execution | 1466.712 | 713.040 | 32.57% | 25.41% |

Build plus launch contributes 3.39%/3.41% (rounded only after summation),
so these measurements do not support prioritizing launch offloading.
Agent answering encloses the source-bound blocking queue, guest connection
and `Restored` acknowledgement durations plus other scheduling time. The
analyzer checks that those readiness stages fit inside this phase. Outside
creation is client create time minus the sum of the four server components;
it includes HTTP/handler and registration/response work, not a measured internal
cause. This fixture uses neither guest networking nor volume mounts; the
network/envd label is the existing source label, not a network benchmark.

The predefined slow threshold is 1000 ms, with 347/400 and 349/400 successful
HyperMachine attempts meeting it. Slow attempts are never excluded from the
overall summaries. Raw logs and ten slowest correlated attempts are retained.
Fractions use summed component time divided by summed readiness time; phase
percentiles are not additive. Stable floating-point summation supports exact
offline recomputation on the tested Linux and Windows Python versions.

Rust duration logs are parsed with explicit ns/µs/ms/s units. The `up in` total
and final component use separate elapsed reads in the accepted source, so both
the logged total and component sum are retained. Both must fit within the
client request, allowing only 0.001 ms for floating-point conversion. The VM
name is source-bound to the sandbox ID through `new_vm`; the frozen main,
agent sources, accepted build context and 550-file source catalog establish
that binding. The three provisional workspace core files are excluded.

```sh
python3 bench-prepared-engines.py --creation-diagnostics --pairs 4 --concurrency 100 \
  --hypermachine /path/to/accepted/hv2-sandboxd \
  --firecracker /path/to/firecracker-v1.17.0-x86_64 \
  --kernel /path/to/bzImage-known-uart-irq \
  --initrd /path/to/guest-output-drain.cpio.gz --output /new/path/report.json
python3 -O verify-prepared-creation.py .
python3 -O check-prepared-creation.py c100-report.json
```

Inputs must match `manifest.json`. The opt-in filter is
`warn,hv2_agent::agent_vm=debug,hv2_sandboxd=debug`; it implies readiness
diagnostics. Only the owned daemon's log is retained, before scratch cleanup,
with a 16 MiB retention limit. Truncation rejects the diagnostic. Twenty-three
corrupted reports are rejected on Linux and Windows with assertions disabled,
including missing/duplicate identities, incorrect units/targets, invalid
durations, enclosure violations and missing metadata. Windows verification is
offline analysis, not native KVM execution.

No production runtime change or performance benefit follows. A separate
isolated sixteen-slot prepared-restore admission candidate is the next
experiment: it targets simultaneous guest readiness work and includes all
admission wait in client readiness. Its performance and lifecycle behavior
remain unverified by this diagnostic archive.
