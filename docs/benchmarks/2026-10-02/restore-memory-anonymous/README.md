# Anonymous residency at restore boundaries

Eight activated real restores pass, including four HyperMachine guests with exactly ordered address-bound probes; cleanup and source/binary/runner hashes are verified. Strict core/agent-library Clippy passes. Windows and Linux regenerate identical source hashes and patches. Eighteen malformed contracts are rejected with assertions disabled. No runtime change is adopted and timings are excluded from rankings.

| Phase | Median RSS KiB | Median private dirty KiB | Median anonymous KiB | Median private clean KiB |
|---|---:|---:|---:|---:|
| before_run | 1408.0 | 1408.0 | 1408.0 | 0.0 |
| after_notice | 21848.0 | 21580.0 | 1436.0 | 278.0 |
| after_exec | 27286.0 | 26874.0 | 1912.0 | 412.0 |

Anonymous measures anonymous residency; private dirty can also include dirty file-cache pages. Clean/dirty components must sum to RSS, and Anonymous and PSS must not exceed RSS. Observations after acknowledgment and execution occur while the guest/device model can still run. They identify when counters change, not the writing code. Single-guest findings do not establish the cause of the separately measured C100 gap. Firecracker restores verify the matched guest workload, but equivalent Firecracker boundary probes are absent.
