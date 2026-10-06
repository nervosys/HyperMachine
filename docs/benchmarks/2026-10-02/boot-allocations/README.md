# Boot-buffer allocation diagnostic

Two fresh owned daemons reproduced the same temporary-allocation transition:
`LoadedBoot::highest_address()` materializes 14,795,027 bytes of boot regions.
Dropping those regions reduces GNU allocator used bytes by 14,794,912 and
increases free bytes by the same amount, with no change in arena or mapped
bytes. The allocator delta differs from payload size because of metadata,
small allocations, and concurrent activity.

| Observation | First daemon | Fresh repeat |
| --- | ---: | ---: |
| Highest-address region payload | 14,795,027 bytes | 14,795,027 bytes |
| Used bytes released after explicit drop | 14,794,912 bytes | 14,794,912 bytes |
| Arena size change after drop | 0 bytes | 0 bytes |
| Initrd clone, used-byte increase (each of two copies) | 1,858,688 bytes | 1,858,688 bytes |
| Kernel clone, used-byte increase (each of two copies) | 12,932,112 bytes | 12,932,112 bytes |

The first clone pair occurs while calculating the highest address. The second
occurs during boot loading. This identifies avoidable boot-buffer copying and
free allocator storage at a specific point in actual guest preparation. It
does **not** establish ownership of the full retained PSS gap, resident-memory
reclamation, or a performance improvement. There is no probe after the second
pair's destruction or after destruction of the original loaded images.

Both cohorts passed actual HyperMachine and Firecracker prepared guest-state
checks, including the restored file, live process, environment, and independent
child writes. Both had zero remaining HyperMachine sandboxes, no cleanup errors,
and an owned-daemon exit code of zero. Each cohort used one pair at concurrency
one; four restored guests passed in total. These are diagnostic runs, excluded
from performance rankings.

The build starts from the accepted 550-file source catalog recorded in
`accepted-source-context.json`, validated before instrumentation. Only isolated
copies of `boot/source.rs` and `boot/linux.rs` are changed by the generator;
unrelated sources are checked afterward. The original accepted core hashes
remain unchanged. The workspace's provisional core files are neither read nor
changed. No runtime optimization is adopted.

The GNU/Linux probe reads `mallinfo2()` in normal execution, with an opt-in
environment variable. Its C layout matches the host's `malloc.h`. Allocator
totals are process-wide, include concurrent work, and are not PSS. Logging itself
can affect allocation and timing. See the [GNU allocator statistics manual](https://sourceware.org/glibc/manual/latest/html_node/Statistics-of-Malloc.html).

Reproduce by copying the accepted clean source to a fresh directory without
`.git`, running `experiment-boot-allocations.py` with its accepted source catalog
and a new patch path, and building `hv2-sandboxd` with the frozen lockfile. Run
`diagnose-boot-allocations.py` twice with the recorded diagnostic binary,
Firecracker, kernel, and initrd, using distinct output paths. The driver enables
the probe only on its owned daemon and delegates guest lifecycle and cleanup to
the frozen prepared harness. Analyze both reports with
`analyze-boot-allocations.py`. `manifest.json` binds archived inputs and evidence;
large executables and guest images are identified by report hashes.

The next candidate should avoid materializing images solely to calculate layout,
preserve all Linux/Multiboot/raw validation behavior, and then undergo clean
uninstrumented correctness and matched memory/latency comparisons. This diagnostic
does not justify adopting the previously rejected trimming candidates.
