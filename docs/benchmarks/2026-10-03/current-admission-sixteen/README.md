# Current daemon: uncapped versus sixteen cold starts

Two four-pair AB/BA cohorts compare the identical optimized current daemon at 100 simultaneous cold requests. Both use eight allowed CPUs, one-vCPU/1,024-MiB guests, the same kernel/image, unchanged 15-second post-launch guest deadline and a five-second idle memory hold. Queueing is included in readiness latency. Each variant gets a fresh daemon.

| Cohort | Cold-start slots | Passed/attempted | Successful P50 ms | Successful P99 ms | Median held PSS MiB |
|---|---|---:|---:|---:|---:|
| initial | Uncapped | 300/400 | 13212.22 | 13644.12 | 8609.69 |
| initial | 16 | 400/400 | 4333.42 | 14032.90 | 8543.17 |
| repeat | Uncapped | 271/400 | 10883.98 | 16053.42 | 8577.84 |
| repeat | 16 | 400/400 | 5131.46 | 19190.02 | 8574.49 |

The bounded setting passed 800/800 attempts; uncapped passed 571/800, with 229 creation failures retained. Both cohorts therefore exit nonzero, and are not reported as all-passing. All 1,600 planned attempts were executed, input hashes remain unchanged, no guests remain and all owned daemons are reaped. Failure details, including guest-state diagnostics, remain in the raw reports.

Among five fully successful pairs, sixteen slots lowered paired mean and P50 in four, but lowered P99 in only two. The repeat includes one pair with worse mean and P50 under the limit. Pooled successful P99 is higher with the limit in both cohorts. Pooled latencies exclude failures; memory medians include only fully successful runs, so these figures cannot substitute for equal reliability or prove a general memory gain. Tail queueing is part of the result.

This establishes a tested local reliability/median tradeoff, not a universally superior budget or competitor win. No runtime default changed. Lower concurrency, multi-vCPU guests, sustained arrivals, other hosts and managed competitors remain unmeasured for this setting. The older same-host engine sweep is a separate cohort, and its all-passing result does not erase the newly observed failures.

Independent validation checks exact arguments, budget isolation, input identities, planned attempts and cleanup; the existing memory analyzer verifies idle timing and baseline subtraction. Twelve malformed-evidence mutations of the first six successful runs were rejected. Full original reports, including failed pairs, were never modified. Firecracker in the experiment input catalog names its shared Python helper; this is a HyperMachine configuration comparison and launches no competing VMM.

Reproduce using run-cohorts.py and validate-cohorts.py with equivalent frozen paths/hashes and a fresh output directory. Protected modified root core files remain excluded from the accepted isolated release build.
