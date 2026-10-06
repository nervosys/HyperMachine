# Cold readiness phase diagnostic (2026-10-01)

The scored [fixed-arrival comparison](fixed-arrivals.md) retains timestamps
before and after cleanup. Mean cleanup duration was 47.92 ms for HyperMachine
versus 58.67 ms for Firecracker at five arrivals/s, and 61.74 ms versus 78.51 ms
at 25 arrivals/s. Cleanup is therefore not the larger HyperMachine component
in these particular cohorts; command readiness is slower despite its shorter
cleanup. This does not generalize to other workloads or hosts.

An opt-in diagnostic then ran 16 cold attempts per engine at five arrivals/s,
with eight workers and eight pinned host CPUs. All 32 attempts passed; image,
binary and harness hashes remained unchanged, and no sandbox records remained.
The HyperMachine daemon enabled only its existing build/readiness debug logs:
`warn,hv2_sandboxd=debug,hv2_agent::cold_readiness=debug`. Its complete bounded
decoded diagnostic log was retained without truncation. This changes timing;
the diagnostic is not a scored comparison or an optimization result.

| HyperMachine phase | Samples | Mean (ms) |
|---|---|---|
| Blocking worker queue | 16 | 0.08 |
| Waiting for guest-agent connection | 16 | 347.83 |
| Agent ping | 16 | 1.73 |

Connection waiting begins after launch and includes Linux boot until its agent
can answer; it is not a measure of transport overhead alone. Existing daemon
logs also retain VM construction, launch and envd setup durations. The next
investigation should compare boot-to-agent progress and transport events,
including corresponding Firecracker phases, rather than assume the absolute
connection duration explains the relative gap. No runtime change was adopted.

The shared harness now accepts `--daemon-log-filter`, defaults to `warn`, records
the filter and flags diagnostic runs as unscored. When enabled, it retains up to
1 MiB of decoded daemon output with an explicit truncation indicator; normal
runs retain their existing tail. Seventeen harness tests pass on Windows and
Linux. Raw report, execution manifest, exact executed coordinator/harness,
per-sandbox timing rows and analyzer are retained beside this document.
