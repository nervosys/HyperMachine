# Agent file operations against the shell's `tar`: timing on KVM

Question: since node-initiated guest steps moved from `/bin/sh` to guest-agent operations, one vsock round
trip is made per file operation (several per file) where the shell path ran one `tar`. How much slower is
that on a large tree? Measured with `tools/bench-agent-ops-kvm.py` (new, reusable), release daemon, KVM.

## Result in one paragraph

On a 5,000-file tree (1 KiB files, about 50 per directory, nested two levels, 3 symlinks) the agent path is
roughly **8-9x slower than the shell path by medians** for the repository unpack and the template COPY, and
**about 17x slower** for the checkpoint archive: about 28 s, 37 s and 15 s against 3.6 s, 4.5 s and 0.9 s.
Each agent operation costs about **2-3 ms** (estimated: repository unpack about 2.7 ms/op, COPY about
2.4 ms/op, archive 3-5 ms/op), so time grows linearly with file count: about 5.5 ms per file for the unpack,
7 ms per file for COPY, 3 ms per file for the archive. Nothing failed or timed out at 5,000 files (slowest
archive 25.8 s against the 180 s deadline, no 413). Every completed result was byte-identical to the
generated tree. At 100 files the difference is under a second and buried in noise. Batching the operations is
worth doing if trees of thousands of files are expected; at hundreds of files it is not needed. **The host
was not quiet** (see below): treat the shell column's maxima as noise and the minima as the cleaner numbers.

## Setup

- WSL2 Debian, `/dev/kvm` (`kvm_amd`, nested under Hyper-V), 1 vCPU and 1024 MB per guest (both images
  identically), `--capacity 4`. Platform lines in the daemon logs: Kvm 57 and 58, Tcg 0 and 0.
- Daemon: release `hv2-sandboxd`, sha256 `0cf628784805b80eb385c1e693b4ec5488b0a1b994f8d3ccce9748326a2e4fb3`.
  Built 2026-11-13 10:49; no workspace source file is newer than the binary, and HEAD is `3b27beb7`. Not
  rebuilt for this task. Same binary as `../build-noshell-kvm/README.md`.
- Kernel `bzImage` sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Agent image (busybox plus the new agent) `busybox.cpio.gz` sha256
  `12577fb386d9649fbea4961478c4279812ab594e27fbad028e37c5342b837bd9`. Its daemon log: 64 `using agent op`
  lines, 0 `falling back to shell`.
- Shell image (busybox plus the agent from before the file operations) `initrd.cpio.gz` sha256
  `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`. Its daemon log: 0 `using agent op`, 66
  `falling back to shell`.
- Driver `tools/bench-agent-ops-kvm.py` sha256 `cb6d9b393024f599c9f925784a8b00ae6bd020f7a86d8cfb15b23c1b92ed4106`
  (also in `report.json`; the file in the tree may have been edited since, check the hash). Command:
  `--sizes 100,1000,5000 --runs 3 --max-wait 60`.
- Two daemons (one per image) run side by side and the measurements alternate between them (the order swaps
  each run and each size), one VM at a time.
- Tree: N files of 1 KiB (mode 0644, every tenth 0755), 50 per directory in `dNNN/sK/`, plus 3 symlinks (a
  file, a directory, a relative one).
- Paths: repo = `POST /sandboxes` with an environment whose Git repository (loopback HTTP remote, `git gc`ed,
  tree under `data/`) is unpacked to `/srv/app`; archive = `GET .../checkpoints/one/archive?path=/srv/app/data`
  of that sandbox (the checkpoint, 0.02-0.9 s, is timed separately in `report.json` and excluded); copy = a
  template build `fromTemplate: base` with one COPY step of an N-file upload, `"force": true`, timed from
  `POST .../builds/{id}` to status ready, the tree then read back through the archive route.
- Baselines: a one-file repository and a one-file COPY, once per image per run, subtracted for the "net"
  columns. No baseline for the archive.
- Completeness: every archive (of the unpacked repository, and the read-back of each built template) is
  compared with the generated tree: same names, file bytes, modes, directory modes, symlink targets, no extras.
  53 of the 54 measured cells passed; the other is the flake below.

## Host load readings (Windows CPU, WSL loadavg)

Windows `LoadPercentage` was **100 % at every one of the 12 batch readings** (`report.json` `host_load`),
caused by other processes on the host (a database server, two long-running compute jobs, other Claude
sessions) that this task does not own and did not stop. Each batch waited 60 s for it to drop and then ran
anyway; the rule "wait if over 30 %" could not be met. WSL 1-minute loadavg ranged 1.4 to 23. Consequently the
numbers are noisy: the shell path shows outliers of 8-18 s where a typical run is 1-4 s.

## Results (3 runs each; seconds, median [min-max])

Gross = the timed request; net = minus the median baseline of the same image; ms/op = agent net time
(archive: gross) divided by the estimated number of agent operations. Ratios and nets at 100 files are
meaningless (differences of tenths of a second, one negative).

### repo

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.74 [0.70-1.63] | 0.37 [0.19-1.86] | 2.0x | 0.08 | -0.32 | -0.2x | 0.4 | 3/3 | 3/3 |
| 1000 | 6.02 [4.42-19.40] | 1.18 [0.83-8.25] | 5.1x | 5.36 | 0.49 | 10.8x | 2.6 | 3/3 | 3/3 |
| 5000 | 28.36 [20.20-31.58] | 3.64 [3.03-17.80] | 7.8x | 27.70 | 2.95 | 9.4x | 2.7 | 3/3 | 3/3 |

### archive

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.47 [0.29-0.54] | 0.23 [0.11-0.99] | 2.1x | n/a | n/a | n/a | 4.4 | 3/3 | 3/3 |
| 1000 | 5.23 [4.51-11.08] | 0.36 [0.36-3.31] | 14.3x | n/a | n/a | n/a | 5.1 | 3/3 | 3/3 |
| 5000 | 15.34 [10.31-25.75] | 0.92 [0.84-2.77] | 16.7x | n/a | n/a | n/a | 3.0 | 3/3 | 3/3 |

### copy

| files | agent s median [min-max] | shell s median [min-max] | ratio gross | agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 | 0.83 [0.51-1.15] | 0.62 [0.41-1.56] | 1.3x | 0.14 | 0.10 | 1.4x | 0.4 | 2/3 | 3/3 |
| 1000 | 6.81 [6.23-7.66] | 1.12 [0.94-11.89] | 6.1x | 6.12 | 0.61 | 10.0x | 2.0 | 3/3 | 3/3 |
| 5000 | 37.32 [30.21-51.36] | 4.50 [1.89-5.72] | 8.3x | 36.63 | 3.99 | 9.2x | 2.4 | 3/3 | 3/3 |

Errors, copy/100 agent: ["sandbox from the template answered 503: {'code': 503, 'message': 'guest never became ready: Timeout: no answer from a guest agent on vsock port 1024 within 15s (0 refused). The guest may not be running, may have no vsock driver, or may not be running hv2-guest-agentd; the guest wrote nothing to its console; VM Running; vCPU 0: 0 exits (0 "]


### Per-operation cost and operation counts

The daemon logs no per-operation line, so operations are estimated from the code (`env_vars.rs`
`unpack_by_agent`, `builds.rs` `copy_by_agent`, `checkpoint_files.rs` `build_archive`):

- repo: 2 per directory (`make_dir`, `set_attr`), 2 per file (`write_file`, `set_attr`), 1 per symlink: about
  10,200 at 5,000 files, about 2,050 at 1,000.
- copy: 2 per directory, about 3 per file and per symlink (stat/clear, `write_file`, `set_attr`): about 15,200
  at 5,000 files.
- archive: 1 listing per directory plus 1 read per file or link: about 5,200 at 5,000 files.

That gives about 2.7 ms (repo), 2.4 ms (copy) and 3-5 ms (archive) per operation at 1,000-5,000 files; each is a
vsock round trip including the agent's work. The shell path costs under 1 ms per 100 files. The host also spends
time cloning and packing the repository (0.14 s, 0.22 s, 0.48 s at 100, 1,000, 5,000 files,
`host_clone_seconds`), common to both paths.

## Findings and caveats

- No failure, timeout or 413 at 5,000 files on any path. The slowest agent archive at 5,000 files took 25.8 s
  (limit 180 s); at this per-op cost the deadline would be reached at very roughly 35,000 files (an
  extrapolation, not measured). `MAX_ARCHIVE_ENTRIES` is 200,000.
- One cell lost to an unrelated flake: the sandbox made from the 100-file agent COPY template (the read-back
  step, not the build) answered 503 "guest never became ready: Timeout: no answer from a guest agent on vsock
  port 1024 within 15s". The build itself was timed but the run counts as incomplete (2/3 in the copy table).
- A first complete run (not kept) hit the template build cache on runs 2 and 3 (identical upload and steps),
  making COPY look instant. The driver now sends `"force": true`, and the logs here hold 0 "Steps unchanged"
  lines. Anyone repeating COPY timings must defeat that cache.
- Noise: Windows at 100 % throughout, nested virtualization (KVM under Hyper-V), 1 vCPU. The agent path, being
  chatty, suffers more in absolute terms (repo, 1,000 files: 4.4 s to 19.4 s). Medians are the better summary
  and minima the best case; the order of magnitude (about one decade at thousands of files) held in all three
  runs. A quiet host would sharpen the ratios.
- Per-op figures are estimates from reading the code, not measured. They depend on file size (small here).
- Not measured: the shell-free image (it has no shell path to compare), trees beyond 5,000 files, deep trees.

## Files

`report.json` (every measurement, host loads, input hashes, log line counts), `summary.md` (the tables above),
`daemon-agent.log` and `daemon-shell.log` (about 140 KB each, untrimmed).
