# Matched prepared-snapshot startup

These runs compare HyperMachine's named snapshot restore with Firecracker
1.17.0 full-snapshot restore on the same shared WSL/KVM host. The accepted
HyperMachine daemon is unchanged. Its build context and compiled main are
frozen here; the three provisional core files in the working tree were excluded.
This measures native prepared startup, not managed competitors or fleet density.

Each guest has one vCPU and 1024 MiB configured RAM. A parent runs a background
shell with a unique environment marker, records its PID, and writes a marker
file. Every child must recover the file, the live process and its environment,
then write a unique child marker into a previously absent file. Both engines
use the same commands, kernel and initrd. Source snapshots are hashed before
and after the cohort. Source preparation and hashing are outside restore timing.

The coordinator pins itself and descendants to eight CPUs. Twenty matched
pairs alternate engine order AB/BA. Latency starts at child creation and ends
after the verified guest command. HyperMachine uses a persistent HTTP daemon;
Firecracker starts a new VMM and loads through its Unix API with a unique
vsock path. Engine-generated kernel arguments differ. The readiness budget
is 15 seconds and command execution has a separate 10-second budget; API and
VMM setup have separate 30-second limits. These are not a total 15-second SLA.

Both source paths are resident or cache-warm. The coordinator hashes full
memory files, and does not drop caches. Firecracker uses a paused full snapshot,
then stops its parent and loads each child with a file memory backend and
`resume_vm`. This follows the versioned [snapshot documentation](https://github.com/firecracker-microvm/firecracker/blob/v1.17.0/docs/snapshotting/snapshot-support.md)
and [API schema](https://github.com/firecracker-microvm/firecracker/blob/v1.17.0/src/firecracker/swagger/firecracker.yaml).

After a batch finishes validation, children remain running for a requested
five-second hold before memory is read. Actual hold elapsed time is not recorded.
Held PSS is the whole HyperMachine daemon or sum of Firecracker VMM processes.
Incremental PSS subtracts the immediately preceding empty-daemon baseline;
Firecracker's no-process baseline is zero. PSS excludes kernel memory and
unmapped file cache. A retained background shell is part of both workloads.

| Concurrency / cohort | Engine | Passed / planned | P50 ms | P95 ms | P99 ms | Held PSS MiB | Incremental PSS MiB |
|---|---|---|---|---|---|---|---|
| 1 / c1 | hypermachine | 20/20 | 144.550 | 776.709 | 996.605 | 72.299 | 31.340 |
| 1 / c1 | firecracker | 20/20 | 251.022 | 814.257 | 965.727 | 22.324 | 22.324 |
| 8 / c8 | hypermachine | 160/160 | 224.202 | 630.366 | 676.490 | 91.011 | 46.083 |
| 8 / c8 | firecracker | 160/160 | 275.289 | 930.576 | 1151.289 | 39.786 | 39.786 |
| 8 / c8-repeat | hypermachine | 160/160 | 242.313 | 529.611 | 609.110 | 92.385 | 46.428 |
| 8 / c8-repeat | firecracker | 160/160 | 252.420 | 429.677 | 455.355 | 41.003 | 41.003 |

c1: HyperMachine had lower paired mean latency in 15/20 pairs; lower paired P99 in 15/20; lower held PSS in 0/20.

c8: HyperMachine had lower paired mean latency in 12/20 pairs; lower paired P99 in 15/20; lower held PSS in 0/20.

c8-repeat: HyperMachine had lower paired mean latency in 14/20 pairs; lower paired P99 in 13/20; lower held PSS in 0/20.

All 684 scored attempts (including four smoke attempts) passed, and scored
cohort cleanup was verified. HyperMachine has lower aggregate median latency
in these profiles, but higher held and incremental PSS. Single-restore P99
is worse, and aggregate concurrency-eight P95/P99 rankings reverse on repeat. The shared-host timing observations do not establish universal
performance superiority or a managed-service win.


All scored reports and exact driver versions are retained. The two-pair smoke
run is a harness check and is excluded from the main timing comparison. Three
earlier setup failures are also retained: the first tried to snapshot a
cold-created parent, the second timed out before phase instrumentation, and
the third queried the template listing instead of the named-snapshot listing.
Each failed before scored restores: 12 planned restores were unattempted,
not runtime restore failures. All three owned daemons stopped with exit zero.
The first report recorded one guest before fallback deletion and has no
post-deletion inventory, so empty-inventory cleanup is not verified for it.
The next two report empty inventories. No setup failure is discarded.

To verify hashes, source provenance, profiles, all scored attempts, cleanup
and recomputed statistics, from the repository root run:

```sh
python3 -O tools/verify-prepared-engines.py docs/benchmarks/2026-10-02/prepared-engines
python3 -O tools/check-prepared-analysis.py tools/analyze-prepared-engines.py docs/benchmarks/2026-10-02/prepared-engines/c8-report.json
```

To reproduce on a Linux KVM host, supply locally built accepted binaries and
the matching guest images (SHA-256 values are in `manifest.json`):

```sh
python3 tools/bench-prepared-engines.py --hypermachine /path/hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 20 --concurrency 8 --output /new/path/report.json
python3 -O tools/analyze-prepared-engines.py /new/path/report.json --output /new/path/analysis.json
```

The harness requires fresh output paths and owns all temporary guests and
processes. Snapshot memory and executable binaries are not committed. Host
background load remains uncontrolled. Percentiles use nearest rank and are
conditional on successful verified attempts and cleanup; memory is conditional
on complete successful batches. With only 20 single-restore observations per
engine, P99 is the maximum. No runtime optimization is adopted from these runs.
