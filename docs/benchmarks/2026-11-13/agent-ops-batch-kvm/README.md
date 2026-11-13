# Batching guest-agent file operations, on KVM

Follow-up to `../agent-ops-timing-kvm/README.md`, which found the agent file path 8-9x slower than the shell's
`tar` at 5,000 files (repository unpack, template COPY) and about 17x for the checkpoint archive, at about
2-3 ms per operation because every operation opens its own vsock connection. The fix is the new additive
guest-agent operation `batch {ops}` -> `batch {results}` (`docs/GUEST_AGENT.md`, "Filesystem operations"; protocol
version still 3). Used by `unpack_by_agent` (`env_vars.rs`), `copy_by_agent` and `workdir_by_agent`
(`builds.rs`) and `build_archive` (`checkpoint_files.rs`). An agent without it drops the connection on the
request; the host sees `is_unsupported_op` and every caller falls back to the old one-operation-per-connection
code (debug line "... has no batch operation, using one op per connection").

## Design (as built)

- Allowed inside a batch: `stat`, `list_dir`, `read_file`, `read_link`, `make_dir`, `write_file` (with its
  `owner`), `make_symlink`, `set_attr`, `remove`. Anything else, or more than 512 operations, refuses the
  **whole batch** before anything runs (`failed`, naming the first offender). Whole-batch refusal was chosen over
  per-item refusal so a malformed batch has no partial effect.
- Operations run in order; a failing one does **not** stop the rest, its `failed {message}` is its own result
  entry with the same text the single operation gives. Callers report the first failure with the same label
  they used before, so error messages are unchanged. Not stop-on-first-error: reads gain nothing from it and
  the unpack/COPY step lists already put a directory before its files. Cost: after a failure the rest of that
  batch (at most 512 operations) has run; every caller gives up on the first failure anyway.
- Reply cap: the agent stops running further operations once the results' size reaches 2 MiB
  (`MAX_BATCH_REPLY_BYTES`), keeping the result that crossed it (so a reply is at most 2 MiB plus one result,
  under the 8 MiB frame). `results` is then shorter than `ops`; nothing is run and thrown away, so the host
  (`GuestAgent::batch`) resends the unrun tail with no risk of repeating an `append`.
- Request size and large writes: `write_file` carries its data inline as base64, as before. The host splits into
  requests of at most 512 operations and 4 MiB of JSON (`BATCH_REQUEST_BYTES`), and callers put only files up to
  256 KiB (`MAX_BATCHED_WRITE`) in a batch; a larger file is written with the existing chunked
  `write_file_as` between batches, so large files are unaffected.
- Unpack and COPY order is unchanged. COPY first stats every file and link path in one batch (what
  `clear_for_member` did one by one), then removes non-directories and writes in later batches. An archive that
  repeats a path (a COPY's never does) skips batching, because the up-front stats would be stale.
- Checkpoint archive: `build_archive` keeps its walk, caps (413 at the same limits, 504 deadline) and order.
  Before reading a file of at most 64 KiB or a symlink it prefetches that item and the next small files/links on
  the walk's stack (up to 256 reads, 4 MiB of file data per batch, 8 MiB held), then uses the prefetched
  answer only if it is a clean, whole result; anything else is read the old way, so errors and "vanished" and
  non-UTF-8 handling are the walk's own.

## Rust verification

- Windows (rustc 1.98.1), after the last edit: `cargo fmt --check` clean; `cargo clippy --all-targets -p
  hv2-guest-agent -p hv2-agent -p hv2-sandboxd -- -D warnings` clean; `cargo test` for the three crates: 15
  `test result: ok` lines, none failed (542, 15, 11, 5, 9, 10, 7, 9, 11, 3, 27, 0, 160 [2 ignored], 7, 0 passed).
- WSL (rustc 1.100.0, `RUSTFLAGS="-D warnings"`): clippy clean; `cargo test`, 15 `test result: ok` lines
  including the Linux-only agent tests (225 passed, 2 ignored in the sandboxd binary; 32 passed in the agent
  binary, which includes the three new agentd handler tests).
- New tests: serde round trip and wire shape, the batchable set, caps (`hv2-guest-agent`); agentd handler tests on
  a temp dir (mixed ops incl. a failure that does not stop the rest, refusal of exec/poll/mount_block/nested
  batch/over-cap with nothing run, reply cap stop and resend, 512 writers uncut); host client tests with a fake
  guest (order, failure entries, split at 512, stop-short continuation without repeats, large writes split under
  the frame limit, non-filesystem op refused locally, whole-batch refusal, old-agent hang-up is
  `is_unsupported_op`); call-site logic (unpack step order, COPY clear/own/mode steps, repeated paths, first-failure
  mapping, `build_archive` with a batching fake: identical archive, 484 reads in <= 6 batches and only the
  too-big file alone, failing/unsupported batch falls back, vanished file and non-UTF-8 link, caps still 413).

## Correctness on KVM (`correctness/`)

Daemon and images below. One run per image (no flake), all with 0 `Tcg` lines (every log: `Kvm` lines only).
"Agent without batch" is the previous agent image (busybox + the agent from before `batch`), which proves the
per-operation fallback; "old image" predates all file operations (shell fallback).

| Check | Image | Result | Log counts |
| --- | --- | --- | --- |
| `check-agent-ops-kvm.py` | shell-free, new agent | success, 7 cases | `using agent op` 40, shell 0 |
| `check-agent-ops-kvm.py` | busybox, new agent | success, 7 cases | agent op 44, shell 0, "no batch operation" 0 |
| `check-agent-ops-kvm.py` | old image | success, 7 cases | agent op 0, shell 46, "no batch operation" 1 |
| `check-agent-ops-kvm.py` | agent without batch | success, 7 cases | agent op 44, shell 0, "no batch operation" 3 |
| `check-build-noshell-kvm.py` | shell-free | success, 6 cases | COPY agent 5 / shell 3, WORKDIR agent 2 / shell 0 |
| `check-build-noshell-kvm.py` | busybox, new agent | success, 6 cases | same counts |
| `check-build-noshell-kvm.py` | agent without batch | success, 6 cases | same counts, "no batch operation" 7 |
| `check-checkpoint-files-kvm.py` | busybox new / old image / agent without batch | success, 7 cases each | (daemon at `info`: no debug lines) |

The batch path is shown to run, not just to pass: on the new-agent images the daemon logs have 0 "no batch
operation" lines, and the benchmark below takes seconds where the per-operation path took tens of seconds. The
archive's use of batching on KVM is shown only by that timing and by the byte-identical archive check in the
benchmark (the checkpoint check logs nothing at `info`).

## Speed (`bench/`, `bench-batch-rerun/`, `bench-no-batch-agent/`)

`tools/bench-agent-ops-kvm.py --sizes 100,1000,5000 --runs 3 --max-wait 60`, same driver and methodology as
`../agent-ops-timing-kvm/README.md` (1 vCPU, 1024 MB, two daemons side by side, alternating, `force: true` for
COPY, every archive compared byte for byte with the generated tree: 66 of 66 measurements complete in each
of the three runs, 0 errors). Shell image = the old image in all three. Medians in seconds (min-max in
`summary.md` of each directory); ratio = agent / shell.

Agent before = `agent-ops-timing-kvm` (old daemon, old-protocol-era busybox agent, a different day-part of the
host), after = the batch agent. **Note the shell column moves by up to 3x between sessions** (repo/5000 shell:
3.64 s before, 1.30, 0.97 and 0.82 s in the three runs here), so the host conditions differ and cross-session
ratios carry that noise; the same-session "agent without batch" column (new daemon + previous agent image: the
per-operation fallback, plus one failed batch attempt per group) is the cleaner before/after.

| path / files | before agent (timing README) | after A (`bench/`) | after B (`bench-batch-rerun/`) | per-op fallback, same session (`bench-no-batch-agent/`) |
| --- | --- | --- | --- | --- |
| repo 100 | 0.74 | 0.26 | 0.17 | 0.38 |
| repo 1000 | 6.02 | 0.55 | 0.36 | 1.77 |
| repo 5000 | 28.36 | 1.29 | 1.44 | 12.28 |
| archive 100 | 0.47 | 0.36 | 0.16 | 0.22 |
| archive 1000 | 5.23 | 0.28 | 0.20 | 1.07 |
| archive 5000 | 15.34 | 0.84 | 0.83 | 4.91 |
| copy 100 | 0.83 | 0.31 | 0.21 | 0.41 |
| copy 1000 | 6.81 | 0.42 | 0.31 | 2.86 |
| copy 5000 | 37.32 | 1.12 | 0.72 | 13.66 |

Agent / shell ratio by medians, before vs after:

| path / files | before | after A | after B | per-op fallback (same session) |
| --- | --- | --- | --- | --- |
| repo 1000 | 5.1x | 0.9x | 1.0x | 4.6x |
| repo 5000 | 7.8x | 1.0x | 1.5x | 15.0x |
| archive 1000 | 14.3x | 1.2x | 1.4x | 7.5x |
| archive 5000 | 16.7x | 2.1x | 2.2x | 17.9x |
| copy 1000 | 6.1x | 0.7x | 0.6x | 7.0x |
| copy 5000 | 8.3x | 1.0x | 1.0x | 22.3x |

Reading: at 5,000 files the batched agent path is about 22x (repo), 18x (archive) and 33x (COPY) faster than the
before figures, and 5.8-19x faster than the per-operation fallback measured in the same session; repo and COPY
are now level with the shell's `tar` (ratio 0.6-1.5x) and the archive is still about 2x the shell's time (0.83 s
against 0.38 s). At 100 files all paths are within a few tenths of a second and the ratios are noise. The
`ms/op (est.)` column of the summaries is the timing README's operation estimate divided into the new times and
no longer means anything (operations are no longer one round trip each); ignore it.

### Host load

- `bench/` (run A): the driver could not read the Windows load (`powershell.exe` was not on the login shell's
  PATH, so `windows_cpu_percent` is null in its `report.json`, and "quiet" there means nothing). My own
  `Get-CimInstance Win32_Processor` readings were 100 % five times before it started and 100 % three times
  after: **the host was not quiet** (a database server, other Claude sessions, as in the timing README).
  WSL loadavg 0.6-0.8.
- `bench-batch-rerun/` (B) and `bench-no-batch-agent/` (per-op): run with `HM_POWERSHELL` set; readings per
  batch, each after the 60 s wait that could not reach the 30 % threshold: B 70-100 % (median about 87), per-op
  15-100 % (median about 68). Both ran against load, the per-op run against a somewhat lighter one, which if
  anything flatters the per-operation numbers.

## Setup and hashes

- WSL2 Debian, `/dev/kvm` (`kvm_amd`, nested under Hyper-V); every daemon log: `Detected hypervisor platform:
  Kvm` lines (58 per benchmark daemon log), `Tcg` 0.
- Daemon: release `hv2-sandboxd`, rustc 1.100.0, working tree on HEAD `fe32ef5c` plus this change,
  sha256 `8aee91d7a98230a59471565a8bc1f8ae232623a0d73162516b24a54981987e07`. A test-only assertion fix and
  rustfmt of test code were applied to the tree while it was compiling; no non-test source changed after
  the compile started (the tests were then rerun, see above), but the binary was not rebuilt after.
- Static agent `hv2-guest-agentd` (`-C target-feature=+crt-static`, x86_64-unknown-linux-gnu, static-pie,
  stripped), sha256 `52f84b96fc42c0124593897b0655e34bb75b94792f33a3a920eaf4287e1ddd32`.
- Images (`tools/guest-image/build.sh`): busybox + new agent `busybox.cpio.gz` sha256
  `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485`; shell-free `noshell-tools.cpio.gz`
  (`--no-busybox`, agent as PID 1, busybox as `/bin/cat dd sha256sum grep ls`) sha256
  `f0e5ac68e1ae5e88ed2c3a14f281a7fbf87168a29e86834d07d19ce44695adbc`; agent without batch (the previous
  busybox image) `12577fb386d9649fbea4961478c4279812ab594e27fbad028e37c5342b837bd9`; old image
  `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`.
- Kernel `bzImage` sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Drivers (also in each `report.json`): `tools/bench-agent-ops-kvm.py`
  `cb6d9b393024f599c9f925784a8b00ae6bd020f7a86d8cfb15b23c1b92ed4106`, `tools/check-agent-ops-kvm.py`
  `98667e5a3f61178bcf0147b69b3a3f75279e4e02c780039cb97ad7dbd658858f`, `tools/check-build-noshell-kvm.py`
  `e3a23c3cf559258a7b477b9c24496aea78fca1688cca1760278fd4a0900e81b0`, `tools/check-checkpoint-files-kvm.py`
  `987fe6b60aab582b566b4794aa3295eac9a52c1dbf0ada9e8df814c4d0fa852e`. None of the drivers was edited.

## Findings and caveats

- An agent without `batch` costs one extra failed connection (and a ping, which `fs_op` makes to confirm the
  agent is alive) per unpack/COPY/workdir/archive group before falling back. The shell image's daemon log has 24
  "no batch operation" lines per benchmark and the shell column did not suffer visibly, but this cost was not
  measured separately. The archive walk asks once and stops.
- `workdir_by_agent` sends `make_dir` and `set_attr` as one two-operation batch; it saves one connection per
  WORKDIR with a non-root user and was not timed on its own.
- The 4-image, 10-run correctness matrix passes, but those checks run small trees. The big trees were exercised
  only by the benchmark (5,000 files: byte-identical archives, all three paths).

## NOT VERIFIED

- The old daemon was not rebuilt, so "before" is the earlier day's measurement on a different host state; the
  same-session comparison is against the previous agent image under the new daemon, which is the per-operation
  path plus the failed batch attempt, not byte-for-byte the old binary.
- A quiet host: Windows load was 100 % (A) and 70-100 % / 15-100 % (B, per-op); no number here is a clean
  benchmark, and the shell column's drift between sessions shows the noise.
- Trees beyond 5,000 files, deep trees, large files (above 256 KiB, which skip batching) and files between
  64 KiB and 4 MiB in the archive (read one at a time as before) were not benchmarked.
- The reply cap and request-size splitting were exercised by unit tests and the fake guest only, not provoked
  on a guest (no tree with a directory of megabytes of small-file data was built).
- The archive's use of the batch path on KVM is inferred from timing and a byte-identical result; no
  debug log line is emitted when it succeeds.
- Failure paths across a real batch (a read of a vanished file, a non-UTF-8 link, a refused batch) were not
  sent to a guest; unit tests only.
- Memory use of the daemon and the agent under batching was not measured.
