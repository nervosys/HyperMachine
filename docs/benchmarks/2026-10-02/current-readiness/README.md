# Current high-concurrency readiness diagnosis

The [scored current comparison](../current-native-engines/README.md) retains a failed 100-guest HyperMachine batch. Its request start offsets span 1.54–59.80 ms; failure response durations span 15.73–18.07 seconds, with median 17.32 seconds. That rules out client submission spread as the larger component, but does not measure server queueing. Timeout diagnostics report 64 halted and 36 runnable guest vCPU samples; 38 have zero recent exits. Sixty-four share instruction address `0xffffffff81eda95f`. These samples do not establish a cause. The retained older symbol query used a different kernel hash, so the address is deliberately unresolved here. Quiet boot output also prevents inferring boot progress from console silence alone.

A separate diagnostic used the identical daemon, Firecracker, kernel and guest-image bytes, two alternating pairs at concurrency 100, eight host CPUs and no added CPU worker. Existing daemon debug tracing collected startup and guest-agent stages. All 400 attempts passed; 200 HyperMachine guest IDs match both stage logs and successful requests. No records remained, cleanup reported no errors and artifacts were unchanged. The failed batch was **not reproduced**. Tracing and shared-host conditions affect timing; these results are unscored and do not replace the benchmark failure.

| HyperMachine diagnostic phase | Samples | Median ms | Mean ms | Maximum ms |
|---|---:|---:|---:|---:|
| Blocking-worker queue | 200 | 7.32 | 10.56 | 181.88 |
| Agent connection wait | 200 | 8986.04 | 8642.17 | 10406.84 |
| Agent ping | 200 | 4.99 | 11.07 | 118.77 |
| VM construction | 200 | 4.94 | 6.17 | 27.76 |
| Launch | 200 | 70.18 | 87.55 | 1237.78 |
| Network and envd setup | 200 | 0.03 | 0.09 | 10.77 |

Connection wait includes Linux boot, device/driver readiness and agent listener readiness. Its large duration does not identify transport overhead. Worker queueing was small in this passing diagnostic; that does not prove it was small during the earlier failure. The next investigation should observe boot/driver/listener progress under matched high-concurrency conditions, preserving the existing deadlines, before choosing a runtime change. No runtime change or performance win is established here.

The raw diagnostic, full readiness log embedded in it, profile exit, frozen wrapper/harnesses/coordinator, analyzer and clean-source build context are hashed in `manifest.json`. `analysis.json` references the preserved scored failure by SHA-256 and retains both failure distributions and successful diagnostic stages. The coordinator's `--cold-readiness` mode is explicitly diagnostic. Run `python docs/benchmarks/2026-10-02/current-readiness/verify.py` to verify both linked cohorts and the analysis.
