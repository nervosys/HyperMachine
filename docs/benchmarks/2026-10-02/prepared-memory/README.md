# Prepared guest mapping diagnostic

Two fresh owned-node cohorts each run three counterbalanced pairs of eight
prepared restores per engine. All 96 diagnostic restores pass the file,
live-process environment and child-write isolation checks from the
[prepared-startup benchmark](../prepared-engines/README.md). The daemon,
Firecracker, kernel and initrd are identical to that benchmark. All owned
guest records are removed, the daemons exit zero, Firecracker processes stop,
and binary/image/source hashes remain unchanged. No runtime code is changed.

The optional `--mapping-diagnostics` coordinator mode reads `/proc/PID/smaps`
for the empty HyperMachine daemon before each batch and for all held owned
processes after the five-second hold and normal memory read. It retains raw
smaps and parsed values. The new observations extend child lifetimes, so
these runs are diagnostic evidence and are excluded from benchmark rankings.
Source preparation, resource settings, execution checks and CPU affinity
otherwise follow the linked benchmark. Raw reports include any failure.

The table shows medians across the three batches for each engine. Guest-sized
means private file mappings exactly 1024 MiB in size: eight in the daemon and
one in each of eight Firecracker processes. Size and path provide candidate
identification, not proof of mapping ownership. All values are MiB of PSS
except the private dirty column. Independently calculated column medians need
not add up to the median total.

| Cohort | Engine | Total mapping PSS | Guest-sized PSS | Guest-sized private dirty | Other mapping PSS | Empty daemon PSS |
|---|---|---|---|---|---|---|
| First | HyperMachine | 90.085 | 46.274 | 16.891 | 44.264 | 43.462 |
| First | Firecracker | 40.242 | 33.972 | 16.387 | 6.262 | 0 |
| Repeat | HyperMachine | 90.393 | 46.546 | 18.465 | 44.613 | 43.704 |
| Repeat | Firecracker | 40.641 | 34.401 | 17.066 | 6.254 | 0 |

The non-guest-sized mapping gap is about 38 MiB in both cohorts, roughly
three quarters of the approximately 50 MiB total gap. Much of HyperMachine's
non-guest mapping PSS is present in the empty daemon. The first held observation
has 28.7 MiB in `[heap]`, 6.5 MiB in other anonymous mappings and 8.4 MiB in
other file mappings. These categories do not identify which subsystem owns
allocations or how much is reclaimable. The guest-sized mappings also differ
in shared residency and PSS; private dirty alone does not explain the gap.

This shifts the next investigation toward startup and retained host allocation
ownership, with the guest mapping difference kept as a separate question.
It does not prove snapshot page copying is unnecessary, establish a heap
reclamation benefit, or justify an allocator/runtime change. Prior cold-path
reclamation experiments remain separate evidence. Private dirty includes
restore writes and guest activity. Sequential procfs reads are not atomic,
and PSS excludes kernel memory and unmapped page cache. Host background load
remains uncontrolled; this is not a managed-service or fleet-density comparison.

From the repository root, verify the frozen hashes, raw interpretation,
profile, state checks, input provenance and cleanup:

```sh
python3 -O tools/verify-prepared-memory.py docs/benchmarks/2026-10-02/prepared-memory
python3 -O tools/check-prepared-memory.py docs/benchmarks/2026-10-02/prepared-memory/c8-report.json
```

Both Linux and Windows verify the archive with assertions disabled. Seven
malformed cases must fail: missing processes, duplicate PIDs, fabricated PSS,
missing raw mappings, wrong units, missing baseline and a scoring claim.

To reproduce on a Linux KVM host with the matching inputs in `manifest.json`:

```sh
python3 tools/bench-prepared-engines.py --mapping-diagnostics --hypermachine /path/hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --pairs 3 --concurrency 8 --output /new/path/report.json
python3 -O tools/analyze-prepared-memory.py /new/path/report.json --output /new/path/analysis.json
```

The coordinator requires fresh output paths, uses owned temporary directories,
and stops its owned processes on failure. Executables and snapshot memory
files are not committed. The frozen build context and compiled main bind
the accepted daemon; the three provisional working-tree core edits were excluded.
