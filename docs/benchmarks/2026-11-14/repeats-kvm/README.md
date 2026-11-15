# Seven "one run" checks, repeated three times on KVM (2026-11-14)

Each check below had one earlier run. Each was run three times in a row with
the current binaries, its original driver under `tools/`, in its own output
directory (`<name>/run1..run3/report.json`; daemon logs are not kept). Every
run's daemon log was read for `platform: Tcg` (none) and for warnings,
errors and panics (none). Daemons logged `Detected hypervisor platform: Kvm`
(4 to 13 times a run). The teams check's node runs at `RUST_LOG=warn`, so
its logs carry no platform line; `/dev/kvm` was present and loaded
(`kvm_amd`) and the same daemon on the same host logged `Kvm` in every other
check.

Host: WSL2 with nested KVM, 46 GB free memory at the start of each run.
Release builds, run one at a time.

## Inputs

| Input | SHA-256 |
| --- | --- |
| `hv2-sandboxd` (release) | `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70` |
| `hv2-control-plane` (release, teams only) | `70fd013042b6bdd1578e32ce91322c711f83418ececf7408a0ad3a9975b682b7` |
| `hm` (release; not used by these drivers, they speak HTTP) | `400d81cdcaffb983b86dd7ee1aea222a3299edead66e5c8d98d5cdf59617f123` |
| kernel `bzImage` (not the earlier runs' `bzImage-known-uart-irq`) | `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297` |
| initrd `busybox.cpio.gz` (all seven checks) | `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485` |

Drivers:

| Driver | SHA-256 |
| --- | --- |
| `check-sandbox-sizes-kvm.py` (changed, see below) | `cbe6b3118e44700aa481647b73223d3d8afa265b1cdb9a34b714e4caaf035294` |
| `check-sandbox-resize-kvm.py` | `24b0d5851296368a9793edcfac8a288a2b53ceba6311b1aa2dd21cb19b56f1fd` |
| `check-auto-checkpoint-kvm.py` | `02aededf3ce00d6eaa5afb7959935f689078cee47661629af9de674e191483e8` |
| `check-disk-fork-kvm.py` | `d5846ba0ba1e628336b77cd5ae8bf96928ff0a155577ea9bbf0906553215d2de` |
| `check-sized-checkpoint-kvm.py` | `e5bda52d7d532130071a75a2d6964c6e00446d58e652a5335f7ac3ed1f12165b` |
| `check-machine-network-kvm.py` | `4f1689494224ddb964866bb7ee253771bfaf4fb44bda0d71b90ce6d284eccbfb` |
| `check-teams-kvm.py` | `d7d901ce18c7c2372fa1ea72fac60df680460026e3428ba9775e9f6406c218f7` |

The earlier runs used other kernels and initrds (the guest-output-drain
initrd for four of them, 1fcc60fa..., which is not on this host's
`/var/tmp/hm-*` fixtures). These runs therefore also test the current kernel
and the busybox initrd with the current guest agent, which is a change of
guest, not only a repeat.

## Results

| Check (BOAT_PARITY row) | Cases | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- | --- |
| `sandbox-sizes` (Sizes) | 6 | 6/6 | 6/6 | 6/6 |
| `sandbox-resize` (Changing size later) | 7 | 7/7 | 7/7 | 7/7 |
| `auto-checkpoint` (Automatic snapshots) | 6 | 6/6 | 6/6 | 6/6 |
| `disk-fork` (Fork) | 7 | 7/7 | 7/7 | 7/7 |
| `sized-checkpoint` (Named snapshots) | 5 | 5/5 | 5/5 | 5/5 |
| `machine-network` (What a sandbox may reach) | 5 | 5/5 | 5/5 | 5/5 |
| `teams` (Teams) | 5 | 5/5 | 5/5 | 5/5 |

Every report has `"success": true` and every case in it `"success": true`.
Values that were stable or varied:

- **auto-checkpoint:** the first checkpoint came 61.2 s after start in all
  three runs. Counter before the roll back, straight after it, and two
  seconds later: 630/599/618, 631/598/618, 631/600/619. The sandbox with no
  interval ran 311.5, 311.7 and 311.5 s with no checkpoint.
- **sandbox-sizes:** the 2 vCPU, 1024 MiB sandbox saw 2 processors and
  989,864 kB in all three runs; start 0.323, 0.271 and 0.251 s (a
  stopwatch, not a benchmark).
- **disk-fork:** two forks each time, `Dirty: 4088 kB` in the source at the
  fork; the fork request took 0.319, 0.464 and 0.284 s (release build).
- **teams:** blue's event list held 2 events and red's 4 in all three runs.

## Failures kept, and driver changes

1. **`sandbox-sizes`, first attempt, case 3 failed (not counted as one of the
   three).** The driver asserted that a snapshot of a sized sandbox is
   refused with 409 "size of its own". The daemon now answers 201
   (`{'names': ['sized-snap:default'], 'snapshotID': 'sized-snap:default'}`):
   since 2026-10-21 a sized sandbox can be snapshotted and checkpointed (see
   `sized-checkpoint-kvm`). That is the intended change in the product, and
   the driver was out of date. Cases 1 and 2 passed in that attempt. Driver
   fix (driver only): the two assertions now expect 200/201, and the case's
   detail records `"snapshot_and_checkpoint": "accepted"` rather than the
   refusal message. The three runs above are on the fixed driver. The
   failed attempt's `report.json` was overwritten by the next attempt, so
   only this description is kept.
   Consequence: the old README's step 3 ("A named snapshot and a checkpoint
   of it are refused with 409") no longer describes the daemon.
2. **`teams`, three attempts, could not start (not product failures).** The
   driver starts `redis-server` from a fixed `PATH`
   (`/usr/local/bin:/usr/bin:/bin`) and this host has no installed Redis
   (an unpacked copy lives elsewhere and needs `LD_LIBRARY_PATH`), so each
   attempt died with `FileNotFoundError: 'redis-server'` before any case ran.
   Not a driver change: a two-line wrapper script was placed at
   `/usr/local/bin/redis-server` for the duration of the three counted
   runs (it set `LD_LIBRARY_PATH` and exec'd the unpacked binary) and
   removed afterwards. The Redis was 8.0.2 from that unpacked package.
3. No other run failed. No run was dropped.

## NOT VERIFIED

- The earlier reports' claims that depended on the old guest (kernel
  `bzImage-known-uart-irq`, the guest-output-drain initrd) were not
  reproduced on that guest, only on the current one.
- Anything the original READMEs listed as not shown stays so: large sizes,
  a size changed on a fork, a cluster, webhook delivery, more than one
  node, `--auto-checkpoint-every`, a sandbox with a disk checkpointed
  automatically, a name-based `allowOut`, DNS, TLS and the Internet for a
  machine, a filesystem with reflinks, a large or full disk.
- `sandbox-resize` still checks only a file on a disk and an environment
  variable as "kept"; volumes and the network are not checked.
- `sandbox-sizes` and `sandbox-resize` times are one stopwatch sample per
  run, not benchmarks.
- These are repeats, not new coverage: three runs on one host, with each
  daemon and guest fresh in every run, but the same driver and fixtures.
- Teams: the platform was not logged for that check's node (see top).
