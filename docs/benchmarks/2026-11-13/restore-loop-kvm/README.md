# Creating a sandbox from a template snapshot, repeated: the "guest agent did not answer" flake, on KVM

Follows up the one failure in `../template-user-owner-kvm/restore-timeout-old-image-run0/`: a sandbox created from a
template snapshot on the old-agent image failed with "guest agent did not answer request 1 in time", and a rerun
passed. This directory measures how often that happens, and finds a cause for one family of it.

Driver (new): `tools/check-restore-loop-kvm.py`. It starts its own `hv2-sandboxd` (`--guest-kit` an empty directory,
so nothing is pulled), builds or uses a template, then creates sandboxes from it one after another (or N at a time),
running one `exec` in each. It records every create's latency, every failure with the daemon's own diagnostic, the
host load average, and, on a failure, the lines of the daemon log around it. Two modes:

- `--template base --count N`: N creates from the image's base template (a restore of the image's boot snapshot).
- `--template user --count K --cycles C`: C times over, start a daemon, build a template `USER user` from the base,
  then create K sandboxes from it. This is the path of the original failure (a restore of a snapshot the daemon
  itself had just taken). On a failure the driver also reads the snapshot files' headers (IOAPIC, PIC, LAPIC) and
  keeps the numbers in `report.json` under `snapshots`.

## Setup

- Host: Windows 11 with WSL2 (Debian), AMD, `/dev/kvm` through `kvm_amd`, 1 vCPU guests. Every run reports
  `platform_lines`: **`Tcg` is 0 in every report** (`Kvm` counts one platform line per VM started).
- Kernel `bzImage`: sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Images (initrd):
  - old agent (shell path): `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`
  - busybox + batch agent: `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485`
- Daemon at HEAD (0dcb4d1f), release build: `53d83fdf1994b4595451d3cc5f415898d7e2632ce02ea228ed23c37fe915a98e`.
- Daemon with the fix in `crates/hv2-core/src/backends/kvm.rs` (working tree), release build:
  `7f50153c7d075011027dabe2ff9458efebf957d4b73679cc0ede2fd8f9eb2e78`.
- Driver `tools/check-restore-loop-kvm.py`: the early runs carry `4f762ecc...` and `2901f6f8...` (each report's
  `input_sha256.driver`); the stress runs and the final file are `15c7d12d72f2f6535e839608923cfc5f71f6e9580fff34a3df18238f2817dae7`.
  Later versions only added recording (snapshot headers, setup errors, cycles); the creates they make are the same.
- Host load: the Windows side sat at 99-100% CPU from other sessions during all of this, which WSL's `loadavg` does
  not show. `loadavg_before`/`loadavg_after`/`loadavg_samples` in each report are WSL's: 0.5-2 in the unstressed
  runs, 9-56 in the stress runs. Absolute latencies are therefore noisy and mean little; the counts of failures do.
  The 4-parallel run ran 4 creates at once on a loaded host, which is why its latencies are the largest.
- Deadlines in the path: `AGENT_TIMEOUT` is 10 s per guest-agent request (`hv2-sandboxd/src/env_vars.rs`), the
  ready wait is 15 s (`ready_timeout`, `main.rs`).

## Results: unstressed, daemon at HEAD

| Run (dir) | Image | Template | Mode | Creates | Failures | create ms p50 / p95 / max |
| --- | --- | --- | --- | --- | --- | --- |
| `old-image-base-seq-500` | old agent | base | sequential | 500 | **0** | 457 / 2304 / 11417 |
| `old-image-user-seq-500` | old agent | `USER user` | sequential | 500 | **0** | 471 / 1635 / 5092 |
| `batch-image-base-seq-500` | busybox + batch | base | sequential | 500 | **0** | 393 / 1571 / 4323 |
| `old-image-base-par4-500` | old agent | base | 4 concurrent | 500 | **0** | 1537 / 11403 / 21104 |
| `batch-image-user-cycles` | busybox + batch | built `USER user`, 100 cycles x 3 | sequential | 300 | **0** | 148 / 378 / 770 |
| `old-image-user-cycles` | old agent | built `USER user`, 150 cycles x 3 | sequential | 447 | **0** | 269 / 569 / 1038 |
| `old-image-user-cycles-r2` | old agent | built `USER user`, 150 cycles x 3 | sequential | 447 | **0** | 120 / 279 / 413 |

3,194 creates, 0 failed. The slowest (21 s, the 4-concurrent run) is an outlier of the loaded host, and each of
those creates still got its answer inside its own deadline. But two things in the table are not clean:

- `old-image-user-cycles` finished 149 of its 150 cycles with creates (447 of 450). That cycle (73) produced no
  create and the older driver recorded no reason, so its cause is **unknown**.
- `old-image-user-cycles-r2` (the newer driver, so it did record the reason) had **one setup failure in 150
  cycles**: the template build itself failed, in cycle 66, with the same message as the original flake
  ("guest never became ready ... no answer from a guest agent on vsock port 1024 within 15s"), and its machine
  sample shows `IOAPIC IRR=0x20` with GSI5 unmasked. That is family 1 below, seen with no stress at all, on the very
  path (a restore of a snapshot the daemon just took) the original failure took.

So the answer to "how often": 0 of 3,194 creates failed, but restoring a snapshot **does** go wrong without any
stress, about 1 setup in 150 cycles x 3 restores on this host, and the creates fail in about the same proportion
whenever the template that was built is itself deaf (see below, where it is counted).

## Results: stressed (48 busy loops in WSL, load average up to 56)

Stress was the only way to make this common enough to study. 120 cycles x 2 creates each:

| Run (dir) | Daemon | Creates (ok / run) | Failed creates | Setup failures (template build) | Templates deaf |
| --- | --- | --- | --- | --- | --- |
| `old-image-user-cycles-stress48` | HEAD | 215 / 224 | 9 | 8 | 12 of 120 cycles |
| `old-image-user-cycles-stress48-fixed` | with the fix | 237 / 240 | 3 | 0 | 0 of 120 |

("Deaf" is a template whose snapshot restores into a guest that never answers: the 8 setup failures, where the
build's own restore of the base failed, plus the 4 cycles whose `USER user` template then failed every create.)

Unstressed, with the fix (`old-image-user-cycles-fixed`): 450 creates in 150 cycles, 0 failures, 0 setup
failures (create ms p50 243 / p95 428 / max 717).

## Cause, family 1: an interrupt latched on a masked IOAPIC pin is lost

Every deaf case in the HEAD stress run (`snapshots` in its `report.json`) has `ioapic_irr: 0x20` in the snapshot
that was restored (GSI5, the virtio-vsock device's line), and the fixed run's snapshots have `0x0`. The failure's own
diagnostic shows the same: `IOAPIC ... IRR=0x20 ... GSI5 ... MASKED=0 ... REMOTE_IRR=0`, `PIC_MASTER IRR=0x32`, 0 exits
in the guest, a guest that waits for an interrupt and never gets it.

The mechanism, in the KVM in-kernel IOAPIC:

1. A raise (level 1) of an edge-triggered pin that is **masked** sets that pin's bit in the IRR and delivers
   nothing. Unmasking does not replay it. While the bit is set, every later raise is coalesced into it (no new edge).
   Only a release (level 0) clears it.
2. The virtio-mmio line is level-style in our model: `raise` asserts it and holds it until the guest's
   InterruptACK. `restore_state` ends with `raise(VRING)` so a restored device that has a used buffer pending
   interrupts the guest.
3. `restore_machine` (irqchip: IOAPIC, PIC) runs first, then the vCPUs, then the devices. If the snapshot was taken
   while the guest still had GSI5 masked (a guest that is early in its boot, or one that masks it around its driver
   probe), the device's raise lands on a masked pin: the IRR bit latches, the guest later unmasks the pin and the
   interrupt never arrives. The snapshot taken from that state carries the latched bit, so every restore of it is
   deaf too, which is why a bad template fails each of its creates.

The fix (`irq_line` in `backends/kvm.rs`): a raise first releases the pin, then asserts it (`irq_line_raw`), so a
stale latched bit is cleared and the assert is a fresh edge. Doc comment added there.

Tests: `raising_an_edge_pin_after_a_masked_raise_still_delivers` (ignored; needs `/dev/kvm`, runs as root). It
raises GSI5 while its entry is masked (the reset state), runs a few guest instructions that unmask and vector the
entry (0x20, edge), raises again, and asserts the vector reached the local APIC's IRR:

- before the fix: 3 of 3 runs fail, `after the next raise: request bits 0x20 ... pending 0`
- after the fix: 3 of 3 pass, `request bits 0x0 ... pending 1`

Results of the product-change checks are in the section "Checks of the product change" below.

## Family 2: device delivery while paused tore the snapshot

The family-1 fix under the same stress still failed 3 creates of 240 (cycles 2, 61, 91). Their snapshots have
`ioapic_irr: 0x0` (so not family 1), the guest shows "0 exits ... 7-9 in all", and the log of cycle 61 shows
vsock packets still being delivered to the guest **between** "VM paused" and "snapshotted", then, on the restore,
`vsock: packet addressed to CID 0` warnings, and no answer.

Cause, from the code (`crates/hv2-core/src/vm.rs`): `VM::pause` parked only the vCPUs. The vsock delivery thread
(started by `attach_vsock`/`attach_vsock_pci`) calls `notify_vsock` whenever the device queues a host-side packet,
with no check of the VM's state: it pops the receive ring, writes the packet into guest memory, `add_used`, sets
`interrupt_status` and raises the line. `notify_net`/`notify_net_pci` are the same. `snapshot_inner` captures the
vCPUs, then a memory pass, then device state, then the irqchip, then writes the pages, so a packet delivered in
between leaves memory, queue indices and interrupt state describing different moments. The torn snapshot itself
was not reproduced in isolation; the link to the 3 failures is by mechanism and by the result below.

Fix: `pause` now holds host-side device delivery (`devices_held`, plus a `delivery_gate` that every
`notify_vsock`/`notify_net`/`notify_net_pci` holds for its whole publish-and-signal, so `pause` waits out one in
flight). Held packets stay in the device's host-side queue; `resume` delivers them (`flush_held_deliveries`).
`standby` does not hold, since traffic is what ends it.

Test: `vm::tests::a_paused_vm_holds_host_packets_and_resume_delivers_them` (real delivery thread, rx ring in guest
RAM): while held, the used ring is untouched, the queue position stays (0,0) and the packet stays pending; after
`resume` it is delivered (used idx 1, position (1,1)). With the hold disabled (the gate always granted) it fails at
the first assertion; with it, it passes.

Results with both fixes (daemon `672801943ae283ce6c25b582333264526b41006efbf92c42e8246e7af6677e70`), 120 cycles x 2
creates, `--template user`:

| Run (dir) | Image | Stress | Failed creates | Setup failures |
| --- | --- | --- | --- | --- |
| `old-image-user-cycles-stress48-quiesce` | old agent | 48 busy loops | **0 / 240** | 0 |
| `old-image-user-cycles-stress48-quiesce-r2` | old agent | 48 busy loops | **0 / 240** | 0 |
| `batch-image-user-cycles-stress48-quiesce` | busybox + batch | 48 busy loops | **0 / 240** | 0 |
| `batch-image-user-cycles-quiesce` | busybox + batch | none | **0 / 240** | 0 |

0 of 720 stressed creates failed, where the family-1 fix alone failed 3 of 240 (about 9 expected in 720; the
chance of 0 at that rate is about 1 in 8,000). One stressed run's daemon log is kept (`daemon-log.txt`); the others
kept none (`--keep-log failures`, and nothing failed).

Regressions with the same daemon, both images (`quiesce-regressions/`): `check-checkpoint-files-kvm.py`,
`check-disk-checkpoint-kvm.py`, `check-disk-fork-kvm.py`, `check-disk-pause-kvm.py` all pass. `check-disk-pause`
first failed on both images on a stale assertion (it expected a checkpoint of a sandbox with a disk to be refused;
that is supported now, as in `check-block-disk-kvm.py`), kept as `disk-pause-old-stale-assertion.json`; every pause
case in it had passed. The check now asserts only the snapshot refusal, and passes.

## Not reproduced

- The original `restore-timeout-old-image-run0` failure itself: I cannot say which family it was. Its diagnostic
  had few vCPU exits, which fits either family.
- Failures from load alone: the unstressed runs, including 4 creates at once, never crossed the 10 s per-request or
  15 s ready deadlines in a way that failed a create (largest 21 s, with per-request deadlines each met).

## NOT VERIFIED

- That restores can no longer fail at all: 0 of 720 stressed and 240 unstressed creates bounds the rate on this
  host, it does not prove zero. That the 3 family-2 failures were the torn snapshot is shown by mechanism and by
  their disappearing, not by catching one.
- The device hold on KVM with network devices attached (`notify_net`/`notify_net_pci`): the runs here use vsock
  only. Standby on KVM after the change (its logic was reviewed, and standby does not hold).
- Snapshots taken before either fix: a template already holding a latched bit or a torn queue is not repaired.
- Cycle 73 of `old-image-user-cycles` (a cycle with no creates and no recorded reason).
- Anything on hardware other than this AMD WSL2 host, on a real (non-nested) Linux host, or with more than 1 vCPU.
- The original failure's family (see above).
- The stress numbers as a rate: 48 busy loops is an arbitrary load; they show the fix removes family 1, not how
  often either family occurs in production.
- macOS (HVF) and Windows (WHPX): the fix is in the KVM backend only; their IRQ delivery is not touched.

## Checks of the product change

- Windows (1.98.1): `cargo fmt --check` clean; `cargo clippy -p hv2-core --all-targets -- -D warnings` clean;
  `cargo test -p hv2-core --lib`: 2452 passed, 0 failed.
- WSL (`cargo +1.100.0`, `RUSTFLAGS="-D warnings"`): clippy `-p hv2-core --all-targets` finished with no
  warnings from hv2-core's code (the manifest-lint notices it prints are pre-existing and not denied);
  `cargo test -p hv2-core --lib`: 2396 passed, 0 failed, 3 ignored.
- The new KVM test run as root: 3 of 3 passed after the fix (request bits `0x20` latched by the first raise,
  `0x0` and the vector pending after the second); before the fix 3 of 3 failed.
- Not run: the other crates' tests (only `kvm.rs` in hv2-core changed).
- Family-2 change (`vm.rs`): Windows (1.98.1) `cargo fmt --check -p hv2-core` clean, clippy `-p hv2-core
  --all-targets -D warnings` clean, `cargo test -p hv2-core --lib` 2453 passed; WSL (`cargo +1.100.0`,
  `-D warnings`) clippy clean, `cargo test -p hv2-core --lib` 2397 passed, 3 ignored; the family-1 KVM test still
  passes as root.
