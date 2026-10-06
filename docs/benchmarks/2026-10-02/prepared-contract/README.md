# Prepared restore with a matched clock/RNG contract

Both engines now complete guest clock and RNG resynchronization before the
timed prepared-state command. HyperMachine already does this in its create
path. The Firecracker control sends the same guest protocol's `Restored`
operation with current host time and 64 fresh entropy bytes, requires an
`Acknowledged` response, and then executes the state command. This replaces
the control's readiness ping. Earlier direct-engine cohorts omitted this
operation and remain preserved; their startup contract differs from this one.

No production binary changes. The accepted daemon, Firecracker 1.17.0,
kernel, initrd, one-vCPU/1024-MiB resources and prepared-state workload match
the earlier [prepared comparison](../prepared-engines/README.md). The
HyperMachine create acknowledgement is bound to frozen accepted main,
`agent_vm.rs` and `guest_agent.rs`: successful creation waits for `Restored`
to be acknowledged. Both engines recover the prepared file and live process
environment, then verify independent child writes. The three provisional
working-tree core edits are excluded from the accepted build.

| Concurrency | Engine | Passed / planned | P50 ms | P95 ms | P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|---|
| 8 | hypermachine | 160/160 | 122.771 | 366.650 | 551.961 | 87.950 | 41.496 |
| 8 | firecracker | 160/160 | 121.982 | 686.716 | 952.504 | 40.954 | 40.954 |
| 100 | hypermachine | 400/400 | 1687.128 | 5592.722 | 5739.314 | 336.362 | 258.194 |
| 100 | firecracker | 400/400 | 1133.562 | 1395.058 | 1423.038 | 285.053 | 285.053 |

C8: HyperMachine paired mean readiness is lower in 9/20 pairs, paired P99 in 9/20, and total held PSS in 0/20.

C100: HyperMachine paired mean readiness is lower in 1/4 pairs, paired P99 in 1/4, and total held PSS in 0/4.

All 1124 scored restores, including four smoke restores, pass with cleanup
verified. C8 median readiness is nearly equal; HyperMachine has lower aggregate
P99 but higher total PSS. At C100, HyperMachine has worse aggregate median
and tails, with higher total PSS despite lower incremental PSS. Tail latency
and fixed node memory remain gaps under the matched guest contract.

The concurrency-eight cohort has twenty counterbalanced HM/FC AB/BA pairs;
the concurrency-100 cohort has four. A two-pair single-guest smoke run is
excluded from the main timing comparison. All reports retain every planned
attempt, source hashes and cleanup. Latency starts at child creation and ends
after the state command; it includes guest resynchronization. Source preparation
and hashing happen outside timing. Both sources are resident or cache-warm,
with no cache drop. This is not node startup or storage durability latency.

The control creates a new notice after transport reconnects, using current
time and fresh entropy. It stores only entropy length and SHA-256 fingerprints,
not the entropy bytes. Every successful control must record acknowledgement,
64 bytes, valid time, positive attempt count and a unique fingerprint across
the archive. Unique input fingerprints do not prove statistical properties of
guest RNG output. Failed attempts remain failures; there is no unmeasured
guest-key or cryptographic security claim. Four portable transport tests verify
default cold ping behavior, notice acknowledgement, rejection of a pong as
a notice acknowledgement, and fresh notices after response loss. Native KVM
execution is Linux-only; Windows tests validate transport logic with mocked
sockets and the evidence parser.

Children are held for a requested five seconds after the batch completes
validation. Actual hold elapsed time is not recorded, and guest ages vary
with startup completion. Held PSS includes the whole HyperMachine daemon or
sum of owned Firecracker VMMs. HyperMachine incremental PSS subtracts the
immediately preceding empty-daemon baseline; Firecracker uses a zero-process
baseline. PSS excludes kernel memory and unmapped file cache, and does not
prove fleet density. Percentiles use nearest rank and are conditional on
successful state validation and cleanup; memory is conditional on complete
successful batches. P99 at C8 uses 160 samples and at C100 uses 400 per engine.

The host is shared WSL nested KVM with uncontrolled background load and eight
pinned driver CPUs. HyperMachine uses a persistent HTTP daemon; Firecracker
starts a fresh VMM through a Unix API. Engine-generated kernel arguments and
hypervisor restore implementations differ. Matching the guest maintenance
contract improves comparability, but does not establish a managed-platform
or universal performance win. No runtime optimization is adopted.

From the repository root:

```sh
python3 -O tools/verify-prepared-contract.py docs/benchmarks/2026-10-02/prepared-contract
python3 -O tools/check-prepared-analysis.py tools/analyze-prepared-engines.py docs/benchmarks/2026-10-02/prepared-contract/c100-report.json
python3 tools/test-firecracker-readiness-contract.py
```

Linux and Windows checks reject seventeen malformed reports with assertions
disabled. The new cases cover missing clock/RNG validation, wrong entropy
size, absent acknowledgement, invalid time and reused fingerprints. Hashes
bind the raw reports, exact coordinator/dependencies and accepted source;
analysis recomputes all samples, paired statistics and cleanup.

To reproduce on a Linux KVM host with the matching inputs in `manifest.json`:

```sh
python3 tools/bench-prepared-engines.py --hypermachine /path/hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 4 --concurrency 100 --output /new/path/report.json
python3 -O tools/analyze-prepared-engines.py /new/path/report.json --output /new/path/analysis.json
```

Output paths must be fresh. The coordinator owns its temporary guests and
processes and stops them on failure. Executables and snapshot memory are not
committed. Earlier archives and their frozen harnesses remain unchanged.
