# Borrowed boot-region candidate

The isolated candidate removes Linux kernel/initrd copies from highest-address
calculation and KVM boot loading. Linux layout construction borrows image slices
and owns the generated boot parameters and command line. The existing owned API
converts these regions to independent owned buffers. Raw images also borrow on
the new path. Multiboot retains its original owned implementation and zero-range
filtering. This is an implementation candidate, not an adopted runtime change.

Across six fresh-daemon outer pairs, held HyperMachine PSS is lower with the
candidate in every pair. Latency rankings are mixed: three pairs have lower
candidate means and five have lower candidate P99. The 100-guest repeat has a
worse aggregate candidate mean and P99. The evidence supports a memory benefit
in this fixture, not latency non-regression or a universal competitor win.

| Profile | Accepted HM mean / P99 (ms) | Candidate HM mean / P99 (ms) | Accepted / candidate held PSS (MiB) | Firecracker control P99, accepted / candidate cohorts (ms) |
| --- | ---: | ---: | ---: | ---: |
| Eight guests | 104.066 / 191.831 | 114.090 / 178.448 | 90.050 / 76.323 | 91.184 / 115.065 |
| 100 guests, first cohort | 2107.809 / 5391.941 | 1490.905 / 2414.546 | 330.630 / 322.057 | 1175.050 / 1351.141 |
| 100 guests, fresh repeat | 1181.035 / 1761.541 | 1250.153 / 1787.165 | 345.474 / 329.492 | 1619.245 / 1310.388 |

Memory is the median held process PSS of complete batches; latency is conditional
on successful guest creation, execution, and cleanup. PSS excludes kernel memory
and unmapped page cache. These results do not establish fleet density. Large
tail differences in the first 100-guest cohort and shifting Firecracker controls
remain visible; there is no causal attribution of the latency differences.
All runs use shared WSL nested KVM with uncontrolled host background load.

All **3,328 scored restores** pass: 832 each for accepted HyperMachine, candidate
HyperMachine, and the two sets of matched Firecracker controls. Each profile has
two baseline/candidate outer pairs in AB/BA order, with two internal HM/FC pairs
per fresh daemon. Every cohort verifies restored file contents, a live process,
its environment, independent child writes, clock/RNG restoration, unchanged
prepared sources, guest cleanup, and termination of owned processes. The helper
uses termination with a kill fallback; this is process cleanup evidence, not a
graceful-exit assertion. No allocation probe, preload, tuning, or diagnostic
logging is enabled in scored runs. Preparation is excluded from restore latency.

The candidate binary is
`10ac196b3268038818265f250b5f2932b288692d9436683d1bae23d0a1fb3cb2`;
the unchanged accepted binary is
`2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f`.
Reports also bind Firecracker 1.17.0, the kernel, initrd, benchmark driver, and
build context. The compiler is Rust 1.95.0. No executables or guest images are
included in this archive.

Validation:

- Final Linux boot-filtered suite: 146 passed. New tests check original-buffer
  pointers, actual command-line and boot-parameter bytes, initrd placement,
  validation errors, raw owned-buffer independence, and Multiboot BSS behavior.
- Full core library suite: 2,302 passed, two hardware tests initially ignored.
  Both ignored tests then pass explicitly on local KVM between the completed
  first 100-guest cohort and its fresh repeat, outside scored benchmarking;
  all 2,304 library tests have passing evidence across those invocations.
- Strict library Clippy passes with the existing argument-count allowance.
  The broader all-target invocation fails on 20 references to missing generated
  guest assembly binaries. That failure is retained; all-target lint is unproven.
- Linux and Windows analysis replays accept each real report and reject ten
  damaged contracts, including diagnostic scoring, wrong binary/source bindings,
  missing samples, bad cleanup, and unmatched clock/RNG maintenance.
- A fresh Windows reconstruction from Git objects and nine accepted overlays
  regenerates all three candidate source hashes and the exact Linux patch bytes.
  This verifies reproduction; it is not a Windows runtime or performance test.

The accepted source catalog contains 550 files. All but nine are recoverable
directly from the recorded Git commit; the remaining accepted versions are in
`accepted-overlays/`, each checked against the catalog. The three protected core
files match the Git commit. The live workspace's provisional versions are neither
read nor modified. Only three isolated core files change in the candidate, and
unrelated source hashes are checked before and after generation.

To reconstruct the source, run `restore-borrowed-boot-source.py` with an existing
repository, `accepted-source-context.json`, and a new output directory. It uses
Git objects plus verified overlays and refuses existing output. Then run
`experiment-borrowed-boot.py` on that directory with the same source context and
a new patch path. Build `hv2-sandboxd` on GNU/Linux with the locked release profile
and record the binary/build binding. Run `bench-borrowed-boot.py` with the accepted
and candidate executables, recorded guest inputs, two outer pairs, and the chosen
concurrency. Use distinct output directories for repeats. Analyze with
`analyze-borrowed-boot.py` and replay damaged-report checks with
`check-borrowed-boot.py`. `manifest.json` binds the archived evidence and helpers.

The first 145-test source variant and its patch/context are retained separately;
the final variant adds the explicit Multiboot regression test before the scored
binary is built. The cross-platform generator uses explicit UTF-8 and byte writes
to preserve the exact tested candidate on both operating systems.

Default adoption remains deferred. Further work must establish latency behavior
across the remaining profiles and integrate the validated change without
overwriting the user's provisional core edits. This candidate is not evidence
that HyperMachine beats Firecracker or managed sandbox products across the board.
