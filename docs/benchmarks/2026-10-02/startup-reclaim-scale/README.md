# Startup trim under larger prepared bursts

The [startup trim candidate](../startup-reclaim/README.md) is rejected from
adoption after these higher-concurrency comparisons. It reduces held memory,
but both matched pairs at concurrency 50 and both at concurrency 100 have
worse mean and P99 readiness. All 2400 scored restores and Firecracker controls
pass, including state validation and cleanup. Passing the readiness limit does
not establish acceptable tail performance. Production behavior remains unchanged.

Only the harness concurrency bounds change, from 16/8 to 100, below the owned
daemon's capacity of 128. The binaries, guest images, one-vCPU/1024-MiB resources,
commands, readiness limits, five-second hold, source preparation and cleanup
are unchanged. Two outer baseline/candidate pairs alternate AB/BA per profile;
each variant runs two internal HM/FC and FC/HM pairs in a fresh daemon.
Raw reports retain every planned child and control. The frozen source, patch,
compiler comments and build context bind the exact candidate and accepted
daemon, with the three provisional core edits excluded.

| Concurrency | HyperMachine variant | Passed / planned | P50 ms | P95 ms | P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|---|
| 50 | baseline | 200/200 | 491.357 | 722.110 | 752.592 | 204.311 | 154.682 |
| 50 | candidate | 200/200 | 483.604 | 1283.051 | 1312.038 | 171.388 | 150.105 |
| 100 | baseline | 400/400 | 992.517 | 1359.799 | 1410.779 | 339.304 | 281.585 |
| 100 | candidate | 400/400 | 1712.829 | 8526.072 | 8656.581 | 319.895 | 286.447 |

C50: candidate mean and P99 readiness are worse in both matched pairs. Held PSS is lower in both; the median aggregate reduction is 32.923 MiB.

C100: candidate mean and P99 readiness are worse in both matched pairs. Held PSS is lower in both; the median aggregate reduction is 19.410 MiB.

| Concurrency | Firecracker control side | Passed / planned | P50 ms | P99 ms |
|---|---|---|---|---|
| 50 | baseline | 200/200 | 474.212 | 674.605 |
| 50 | candidate | 200/200 | 515.513 | 616.801 |
| 100 | baseline | 400/400 | 1000.670 | 1185.063 |
| 100 | candidate | 400/400 | 993.382 | 1335.024 |

PSS is the whole held HyperMachine daemon, including the preceding empty-node
baseline. Incremental PSS subtracts that baseline. Guest sources are resident
or cache-warm; file hashing occurs outside restore timing and no caches are
dropped. PSS excludes kernel memory and unmapped file cache, and does not prove
fleet density. The host is shared WSL/KVM with uncontrolled background load.
Percentiles use nearest rank and are conditional on successful guest validation
and cleanup; memory is conditional on complete successful batches. There are
200 samples per HyperMachine variant at C50 and 400 at C100. Each matched-pair
P99 uses 100 or 200 samples respectively. These remain short burst experiments. Guest ages vary with startup completion;
the five-second hold begins after all validations, not at equal guest age.
Slower batches can therefore have older guests when memory is read.

The candidate calls trim once after initial template preparation, before
listening; its marker appears exactly once per candidate node and never in a
baseline node. There is no allocator preload or periodic worker. The test
does not identify the cause of the tail regression, nor prove that every use
of one-time reclaim would regress. It establishes enough contrary evidence
to avoid adopting this particular candidate. Sustained lifecycle tests are
not claimed, and the earlier C8 passes do not override the larger-burst result.

A control-contract distinction also needs to remain explicit. HyperMachine's
create path sends the guest a `Restored` operation with the current host time
and 64 fresh entropy bytes before creation completes. The direct Firecracker
control loads its snapshot, pings the guest and executes the marker checks;
it does not send `Restored`. Frozen accepted `agent_vm.rs` and `guest_agent.rs`
bind that behavior to the compiled source. Both HyperMachine variants perform
the same maintenance, so the trim comparison retains its contract. Firecracker
controls are direct-engine observations, not a service with an identical
guest-startup contract. The next fairer service-contract comparison must add
the clock/RNG maintenance to Firecracker; earlier raw cohorts remain preserved.
No managed-platform performance win is claimed.

From the repository root, verify hashes, source bindings, profiles, marker
counts, every attempt, recomputed statistics and cleanup:

```sh
python3 -O tools/verify-startup-reclaim.py docs/benchmarks/2026-10-02/startup-reclaim-scale
python3 -O tools/check-startup-reclaim.py docs/benchmarks/2026-10-02/startup-reclaim-scale/c100-report.json
```

Linux and Windows evidence checks run with assertions disabled. Ten malformed
cases must fail, including missing/repeated startup trims, wrong binary,
changed snapshot, wrong resources, missing prepared state and an adoption claim.
Windows checks validate evidence, not the GNU/Linux candidate runtime.

To reproduce the larger profile with the binaries and images in `manifest.json`:

```sh
python3 tools/bench-startup-reclaim.py --baseline /path/accepted-hv2-sandboxd --candidate /path/candidate-hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 2 --concurrency 100 --output /new/path/report.json
python3 -O tools/analyze-startup-reclaim.py /new/path/report.json --output /new/path/analysis.json
```

Candidate build instructions remain in the linked startup experiment. Output
paths must be fresh. The harness owns its temporary guests/processes and
retains each completed cohort. Executables and snapshot memory are not committed.
