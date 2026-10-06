# Prepared restore server readiness diagnostics

The accepted HyperMachine daemon is unchanged. Selective existing debug logs
correlate successful sandbox IDs with blocking queue, guest connection and
`Restored` clock/RNG acknowledgement durations. Two fresh C100 cohorts each
use four alternating HyperMachine/Firecracker pairs. All 1600 C100 restores
pass, plus four smoke restores; all owned processes stop and guest inventories
are empty. Inputs and prepared sources remain unchanged.

These are **logging-enabled diagnostics**, not performance ranking cohorts.
The WSL host is shared and its background load is uncontrolled. The direct
Firecracker control matches guest state, resources, clock/RNG maintenance and
state command, but uses a fresh VMM/Unix API while HyperMachine uses a persistent
HTTP daemon. Neither managed competitor superiority nor a runtime optimization
is established. Earlier logging-disabled evidence remains preserved.

| Cohort | Engine | Passed | Ready P50 ms | Ready P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---:|---:|---:|---:|---:|
| C100 | HyperMachine | 400/400 | 2268.082 | 5764.574 | 348.455 | 270.169 |
| C100 | Firecracker | 400/400 | 1386.681 | 2661.476 | 283.778 | 283.778 |
| C100 repeat | HyperMachine | 400/400 | 4279.664 | 5811.669 | 354.236 | 274.940 |
| C100 repeat | Firecracker | 400/400 | 1690.124 | 2741.850 | 270.736 | 270.736 |

Multi-second HyperMachine readiness is observed in both diagnostic cohorts.
All 800 successful HyperMachine children match exactly one successful readiness
trace. Each cohort also retains one uncorrelated preparation-parent trace,
reported explicitly in the analysis. The fixed slow threshold is 1000 ms:
358/400 and 400/400 children meet it. No slow attempt is removed.

| HyperMachine component | First mean ms | Repeat mean ms | First fraction of total time | Repeat fraction of total time |
|---|---:|---:|---:|---:|
| Blocking task queue | 45.737 | 45.017 | 1.63% | 1.10% |
| Guest connection | 604.293 | 850.973 | 21.59% | 20.84% |
| Restored acknowledgement | 332.042 | 440.775 | 11.87% | 10.80% |
| Other client create time | 1084.497 | 1456.629 | 38.75% | 35.68% |
| Client execution | 731.899 | 1289.348 | 26.15% | 31.58% |

Fractions divide summed component durations by summed readiness durations,
not averages of per-attempt fractions. Individual stage percentiles are not
additive. The raw log and ten slowest correlated attempts are retained.
The relatively small blocking-queue share does not explain the whole tail.
Connection and acknowledgement elapsed times do not distinguish host CPU
contention, guest scheduling, transport work or guest processing. Other create
time includes HTTP, provisioning, snapshot restore and response work. Selective
logging itself can perturb timing, and the difference from earlier cohorts
cannot be attributed to logging or any other single cause from these data.

The accepted main source passes the sandbox ID to `new_vm`, which sets the
VM name; accepted `after_restore` logs that name. The frozen source files are
bound to the accepted clean build and its 550-file source catalog. The three
provisional core files in the workspace are excluded. No Rust build or
runtime change is part of this diagnostic.

Reproduce using the frozen driver and accepted input hashes in `manifest.json`:

```sh
python3 bench-prepared-engines.py --readiness-diagnostics --pairs 4 --concurrency 100 \
  --hypermachine /path/to/accepted/hv2-sandboxd \
  --firecracker /path/to/firecracker-v1.17.0-x86_64 \
  --kernel /path/to/bzImage-known-uart-irq \
  --initrd /path/to/guest-output-drain.cpio.gz --output /new/path/report.json
python3 -O verify-prepared-stages.py .
python3 -O check-prepared-stages.py c100-report.json
```

The opt-in filter is `warn,hv2_agent::agent_vm=debug`. Logs come only from the
owned daemon and are captured before temporary scratch deletion, with a
16 MiB retention limit; overflow rejects the diagnostic. The analyzer rejects
missing/duplicate IDs, malformed or nonfinite/negative stages, failed traces
for successful clients, truncated logs and stages exceeding client creation.
Thirteen corrupted fixtures are rejected on Linux and Windows with assertions
disabled. Windows verification covers offline analysis, not native KVM.

The next diagnostic is the accepted daemon's existing build/launch timing log
to subdivide other create time. No production optimization is justified yet.
