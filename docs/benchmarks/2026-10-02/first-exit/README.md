# Current-kernel symbol and first backend-return diagnosis

The exact kernel from the retained failed C100 cohort maps instruction address
`ffffffff81eda95f` to `default_idle+15`. A separately booted, owned Firecracker
guest supplied the predecessor/successor symbols from `/proc/kallsyms`; kernel
and initrd hashes match the failed cohort. This resolves the earlier symbol
query's mismatched kernel provenance. It does not explain the readiness failure
or observe the failed guests executing that function over time.

A clean diagnostic daemon adds one fixed exit category and optional I/O port to
the existing first backend-return event. It logs no I/O payload. The guest
deadline, retry policy and handling of exits are unchanged. The source excludes
all three pending boot edits and derives from the accepted registration-mutation
daemon source. Its source and binary hashes are in `build-context.json`.

Two alternating C100 pairs completed all 400 attempts (200 per native engine),
with all 200 HyperMachine IDs matched to dispatch, readiness and first-exit
records. Every first return was `io_out` to decimal 3320 (`0xCF8`, PCI
CONFIG_ADDRESS; see `devices/pci_config_io.rs` in core). The full cohort retains
all attempts and its raw diagnostic log. Empty inventory, process/guest cleanup
and unchanged artifacts passed. The earlier scored failure was not reproduced.

| HyperMachine phase | Samples | Median ms | Mean ms | Maximum ms |
|---|---:|---:|---:|---:|
| VM dispatch queue | 200 | 2.46 | 3.98 | 48.26 |
| Owner thread setup | 200 | 0.09 | 1.61 | 43.71 |
| First backend call through return | 200 | 3098.63 | 3428.57 | 4986.78 |
| Agent connection wait | 200 | 7933.06 | 8103.32 | 9655.92 |

The large first-call interval ends at the first PCI configuration-address OUT.
It includes host scheduling and guest execution; it does not measure how much
CPU work occurred or isolate kernel decompression. The connection interval
includes guest boot and listener readiness. These measurements direct the next
investigation toward early guest execution and host scheduling before the first
PCI probe, without establishing a transport bottleneck or a reliability fix.

Both native engines used the same kernel/initrd, 1 vCPU and 1024 MiB per guest,
eight host CPUs in affinity, no added CPU worker and 15-second guest deadlines.
HyperMachine tracing changes timing. This cohort is diagnostic and excluded from
scored competitor comparisons; no optimization or performance win is established.
The accepted baseline daemon remains the previously verified binary.

Linux core tests passed 2294 with two ignored. Windows core all-target checking
passed. All eight diagnostic parser tests passed on each platform. Linux daemon
Clippy passed with the existing `too_many_arguments` exception. The first
compile used the wrong enum, and the initial release invocation named the wrong
package; both failed logs are preserved alongside corrected test/build logs.

Run `python -O verify.py` here to verify hashes, the linked failed cohort,
first-exit identities, all 400 attempts, and the recomputed analysis.

The runtime collection used `refresh-local-engines.py --first-exit --profiles
100 --pairs 2` with the archived `diagnose-concurrent-startup.py` wrapper and
the clean `/var/tmp/hm-first-exit/hv2-sandboxd`. Input paths and hashes are
retained in the matrix and cohort; the frozen tools accept explicit paths for
reproduction on another owned Linux host. No executable or private key is stored
in this archive.
