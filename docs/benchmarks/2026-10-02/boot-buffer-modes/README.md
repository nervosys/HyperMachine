# Same-binary boot-buffer counterfactual

This experiment compares owned and borrowed Linux/raw image buffers inside one
executable. It removes separate builds as a latency confound. The owned mode is
a counterfactual within the refactored layout implementation, not the original
accepted daemon. Default production behavior is unchanged.

All **1,664 scored restores** pass, with matched guest-state, clock/RNG, source
integrity, and cleanup checks. Borrowed mode has lower held PSS in all four outer
pairs. Its aggregate latency is worse in both profiles. The first 100-guest pair
has a large borrowed-mode tail alongside substantially slower Firecracker
controls; the second pair has better borrowed-mode means and P99. The results
support the fixture memory benefit but do not establish a causal slowdown,
latency non-regression, or a universal performance improvement.

| Profile | Owned HM mean / P99 (ms) | Borrowed HM mean / P99 (ms) | Owned / borrowed held PSS (MiB) | Firecracker control P99, owned / borrowed cohorts (ms) |
| --- | ---: | ---: | ---: | ---: |
| One guest | 41.664 / 58.027 | 48.314 / 119.378 | 69.221 / 56.701 | 60.445 / 123.622 |
| 100 guests | 1165.615 / 1737.004 | 2067.111 / 5533.443 | 349.855 / 324.422 | 1269.268 / 2419.393 |

The single-guest profile has 16 samples per mode/engine, more than the earlier
four-sample extension but still too few for a stable production P99 estimate.
Its observed P99 is the maximum sample. Each mode has 400 HyperMachine and 400
Firecracker attempts in the 100-guest profile. Samples and repeated inner batches
are not independent hosts. All results use shared WSL nested KVM with uncontrolled
background load, inherited eight-CPU affinity, one vCPU per guest, the recorded
guest inputs, and five-second memory holds outside latency timing. Preparation
is excluded. PSS excludes kernel memory and unmapped page cache.

Both modes use executable SHA-256
`448101e4f84ba984ba3d2846fa4c24ba7acf87a177fac97d0546745761633d2c`.
The host-only `HM_BOOT_IMAGE_MODE` variable selects owned or borrowed buffers.
The selection is cached once. A single startup message records the actual mode;
the coordinator verifies it before its first scored batch. There is no per-request
diagnostic logging, allocator probe, preload, or allocator tuning. The reported
activation record must match the requested mode and predate scoring. Separate
guest images and executables are not supplied for the two modes.

The experimental patch adds a pure explicit-mode layout helper. Its unit test
checks identical Linux wire bytes and validation errors, confirms both borrowed
image slices, and confirms owned initrd data has an independent buffer. Existing
raw, Linux, and Multiboot tests still pass. The full core library run passes 2,303
tests with two hardware tests initially ignored; both then pass explicitly on
local KVM after all scored comparisons stop. There is passing evidence for all
2,305 library tests across those invocations. Strict library Clippy passes;
the earlier all-target fixture limitation remains unproven and is not claimed
resolved by this experiment.

Linux and Windows analysis replays accept both actual profiles and reject
thirteen damaged contracts. These include missing or wrong activation, activation
after scoring, a different executable, missing source bindings/samples, bad guest
cleanup, diagnostic scoring, and unmatched clock/RNG restoration. Windows
regeneration produces the exact compiled Linux source hashes and patch bytes;
that is reproduction evidence, not a Windows runtime or performance test.

Reconstruct the accepted source using the Git-object and nine-overlay procedure
in the [borrowed-boot archive](../borrowed-boot/README.md), then apply its candidate
generator. Run `experiment-boot-buffer-modes.py` on that verified candidate using
the archived accepted and candidate source contexts and a new patch path. Build
the locked GNU/Linux release and record the binary/build binding. The mode driver
requires the same binary for both modes, injects the host-only setting only into
its owned daemon, and verifies activation before timing. Run two outer AB/BA
pairs with eight internal HM/FC pairs at concurrency one or two internal pairs
at concurrency 100. Analyze with `analyze-boot-buffer-modes.py` and replay the
damaged-contract checks with `check-boot-buffer-modes.py`.

The source generator validates all 550 input files and checks that unrelated
files remain unchanged. Only two more isolated core files change relative to the
borrowed candidate; its KVM file remains identical. The live workspace's
provisional core files are not read, built, changed, or staged. Source overlays,
prior candidate patch, and restoration helper remain in the prior archive;
this archive freezes the mode patch, contexts, tools, raw reports, tests, and logs
under `manifest.json`. Large executables and guest images are identified by hashes.

Default adoption remains deferred. The large tail also occurs inside one binary,
so separate code generation is not required for its appearance. The experiment
does not prove an allocator, scheduler, guest-clock, or readiness-protocol root
cause. Further diagnosis must measure the failing phases before choosing another
runtime change. The [remaining separate-binary profiles](../borrowed-boot-profiles/README.md)
and earlier favorable/unfavorable comparisons remain intact.
