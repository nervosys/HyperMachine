# Current native engine comparison

The current clean-source HyperMachine daemon and retained Firecracker 1.17.0 binary ran identical kernel/initrd, boot arguments, one vCPU and 1024 MiB guest memory on shared WSL nested KVM. Two alternating AB/BA batch pairs ran at concurrency 1, 8, 50 and 100, with eight host CPUs in the inherited affinity and no added CPU worker. Background host load was not excluded. Readiness includes a verified shell marker. Guests were held five seconds after the last validation; empty-daemon baselines and post-cleanup PSS use five-second holds.

**536/636 attempts passed: HyperMachine 218/318 and Firecracker 318/318.** Profiles 1, 8 and 50 passed completely. HyperMachine's first 100-guest batch returned 100 creation errors (HTTP 503); its second batch passed. Of the failures, 98 reported no guest-agent answer within 15 seconds and two reported refused connections within that deadline. The cause is unproven. No profile was rerun or discarded. All profiles reported zero remaining sandbox records, no cleanup errors, stopped owned engine processes and unchanged artifacts.

| Concurrency | Engine | Passed/attempted | Successful P50 ms | Successful P99 ms | Median held idle PSS MiB | Median incremental PSS MiB | Valid memory batches |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | hypermachine | 2/2 | 374.99 | 386.59 | 120.88 | 105.70 | 2 |
| 1 | firecracker | 2/2 | 335.47 | 344.79 | 85.92 | 85.92 | 2 |
| 8 | hypermachine | 16/16 | 494.68 | 574.86 | 859.08 | 771.60 | 2 |
| 8 | firecracker | 16/16 | 429.11 | 437.95 | 670.95 | 670.95 | 2 |
| 50 | hypermachine | 100/100 | 3121.66 | 3372.61 | 4243.23 | 4203.18 | 2 |
| 50 | firecracker | 100/100 | 2560.44 | 2694.52 | 4180.90 | 4180.90 | 2 |
| 100 | hypermachine | 100/200 | 8484.27 | 8616.95 | 8467.62 | 8317.98 | 1 |
| 100 | firecracker | 200/200 | 5766.80 | 8674.39 | 8360.17 | 8360.17 | 2 |

Latency percentiles use only successful, cleaned-up attempts and nearest-rank selection. At concurrency 1 there are only two samples per engine. At concurrency 100, successful percentiles omit the failed HyperMachine batch; nominal tail differences cannot establish a reliability or performance win. Its memory statistic uses one valid batch versus two for Firecracker, so that row is not a matched two-pair memory comparison.

HyperMachine held PSS includes its persistent daemon; its increment subtracts the same-batch empty daemon. Firecracker sums fresh VMM processes against a zero-process baseline. These process metrics exclude kernel allocations and benchmark-controller memory and do not prove density or whole-host efficiency. Two batch pairs are limited repeatability evidence, not independent per-guest memory samples or an SLA. Managed endpoints, snapshots, fleet scale and dedicated bare-metal measurements are absent.

Firecracker has lower successful P50 readiness in every profile and lower held/incremental PSS in the fully passing profiles. This refresh establishes a current baseline and a high-concurrency readiness failure to investigate. It does not isolate a causal regression from the intervening feature work; prior cohorts used different daemon artifacts and host/load conditions. No runtime optimization was adopted.

Raw profiles retain every attempt and full failure diagnostics. `matrix.json` retains profile exits, including exit 1 at concurrency 100. Frozen harnesses, coordinator, analyzer and clean-source build context are hashed in `manifest.json`. Run `python tools/refresh-local-engines.py --hypermachine DAEMON --firecracker FIRECRACKER --kernel KERNEL --initrd INITRD --harness tools/bench-local-engines-concurrent.py --output DIRECTORY --pairs 2` on an owned Linux x86-64 KVM host, then `python tools/summarize-local-engine-refresh.py DIRECTORY`. Run this archive's `verify.py` to verify evidence integrity; its successful verification explicitly reports a failed benchmark cohort.
