# Plan for completing the boat.dev parity goal

Written 2026-10-28 from [BOAT_PARITY.md](BOAT_PARITY.md) and the evidence under
`docs/benchmarks/`. "Done" here means a claim has a real-KVM (or real-platform)
check with a kept report, and the docs say what was **not** verified. Tick a box
only when that is true. Items marked **OWNER** need an account, a key, hardware
or a decision that is not the engineer's to make.

## 1. Where it stands

Closed with kept evidence (see `docs/benchmarks/2026-10-23` to `2026-10-27`):

- [x] Docker in a sandbox: engine, bridge, published port through the node proxy,
      `docker build` (including a pulled base and a `RUN` that fetches over the
      guest's bridge), `docker compose` up / build / two projects, a volume's data
      across a checkpoint and fork, `--userns-remap`, a checkpoint and a fork in
      the middle of `docker build`.
- [x] `/dev/kvm` in a sandbox, and Docker together with it (a nested toy VM to an
      I/O exit and HLT).
- [x] A checkpoint of a sandbox with a disk, restored byte for byte: on ext4, XFS,
      Btrfs (reflinks shared on XFS and Btrfs) and on a `--disk-slot` node.
- [x] KVM backend sets the hypervisor bit in guest CPUID (a 6.6 host did not), so a
      nested guest now sees KVM, `kvm-clock` and its TSC frequency.

Open, in the order sections 2 to 6 take them:

- [ ] A real guest OS in the nested VM (reaches userspace).
- [ ] Docker leftovers, evidence quality, platform backends, owner items, wrap-up.

## 2. A real guest OS in the nested VM

State: a real kernel boots in 0.6 s on the host and does not reach userspace in a
sandbox's nested VM. Ruled out (`2026-10-27/nested-real-kernel-diag/README.md`):
PIT creation failing, exit-handling cost, the guest halting, first-touch memory
faults, a slow sandbox vCPU, and missing `kvm-clock` (fixed, did not cure it).
The nested guest is still about ten times slower in real time.

- [x] Re-run the nested boot on the current build and keep the baseline numbers
      (guest time reached in 60 s, userspace exits) as the comparison point.
      Done 2026-11-11, after rebuilding the lost WSL fixtures from scratch (kernel
      with `build-kernel.sh --docker --kvm`, tools image with `build-tools-image.sh`,
      `hv2-sandboxd` and `linux_boot_probe` release builds; needed `protoc` installed
      first). Result: 19.3 s of guest time and 15,641 userspace exits in 60 s wall,
      no userspace reached -- matches the 2026-10-27 baseline (17.7 s, 15,643 exits,
      no userspace) within the run-to-run spread that README already shows. The guest
      still sees KVM, `kvm-clock` and a real TSC frequency (the `advertise_hypervisor`
      fix still holds). **Unchanged: still slow, cause still unknown.** See
      `docs/benchmarks/2026-11-11/nested-boot-rerun/README.md`.
- [x] Memory-light test: a nested guest that does little page-table work, to see
      whether the slowness follows memory activity (nested paging under Hyper-V
      is the leading guess and is a guess). Done 2026-11-11: a 96 MiB variant
      reached *less* guest time in 60 s (16.6 s) than the 256 MiB baseline in the
      same run (25.8 s) -- the opposite of the guess's prediction, but the
      2026-10-27 README's own three-variant run showed the reversal in the other
      direction, and `pf_fixed` (nested page faults fixed) was flat for the
      baseline both times. One run each session, no controlled A/B: **the guess is
      neither supported nor refuted**, and the run-to-run spread is doing most of
      what looks like a signal. See
      `docs/benchmarks/2026-11-11/nested-memlight-and-counters/README.md`.
- [ ] Count where time goes: sandbox-side `perf kvm` or `trace-cmd`, and the
      per-VM KVM counters sampled every few seconds, not twice. Partly done
      2026-11-11 (same README as above): counters sampled at 20 s and 50 s again
      (still twice, not every few seconds) for both the baseline and the
      memory-light variant -- `halt_exits` stayed 0 throughout both, confirming
      2026-10-27's finding that the guest is not halted. `perf kvm` / `trace-cmd`
      inside the sandbox, and a tighter sampling interval, are still not done.
- [ ] Try the same nested boot on a host that is not Hyper-V-nested (a bare-metal
      Linux box or a cloud bare-metal instance) to separate "our KVM backend" from
      "the layer below". **OWNER** if hardware is needed.
- [ ] Decide from the evidence: either fix it, or record it as a limit of nested
      virtualization under WSL2/Hyper-V with the measurements, and mark the
      `BOAT_PARITY.md` row accordingly. Do not claim more than was shown.
- [ ] If it boots: a checkpoint and a fork while a nested VM is running, and the
      nested guest across a restore.

## 3a. Direction (owner, 2026-10-28)

HyperMachine is not meant to run Docker; the goal is to **port Docker's workflow to
HyperMachine itself** (build and run images, compose, with HyperMachine's own
sandboxes). Docker's engine inside a guest was checked (section 3 below) and stays as
evidence, but no more of it is to be built. The work is on the native path
(`hm sandbox vm build|run|compose`), whose gaps are in
[RUN_IMAGES.md](RUN_IMAGES.md):

- [x] Multi-stage builds and `COPY --from` a stage
      (`2026-10-28/hm-multistage-kvm`, 5 cases and the 6-case build check, 3 of 3 runs each).
- [x] Compose: networks between services, by service name, on a cluster node
      (`2026-10-28/compose-network-cluster-kvm`, 6 cases, 3 of 3 runs). Two nodes and UDP
      are NOT VERIFIED.
- [x] Standalone-node VM-to-VM networking, independent of `compose` (2026-11-10): a direct
      ask from another session (`aircom-b5`, porting Docker e2e tests with three VMs and a
      mid-test network disconnect/reconnect). `hm sandbox vm network create|list|inspect|
      rm|connect|disconnect` manages named networks on one node with no cluster or mTLS
      (`crates/hv2-sandboxd/src/{local_network,networks}.rs`); a sandbox's gateway resolves
      and dials other members through a `PrivateNetwork` that checks current membership
      live on every lookup, so a disconnect takes hold on the next one and a reconnect on
      the one after that, with nothing to restart -- the mid-test disconnect aircom-b5
      asked about. Dialing a peer goes over `AgentVM::forward_port`'s vsock channel (the
      published-machine-port primitive), never the peer's own virtio-net device. 9 unit
      tests on the registry's own bookkeeping (allocation, scoping, disconnect/reconnect,
      `remove` refusing a nonempty network); `hv2-sandboxd`'s existing 117 pass unchanged.
      **NOT VERIFIED**: nothing here has run against a live guest -- this repo has no Linux
      KVM host, and WHPX's own multi-vCPU boot panic (above) is a separate, still-open
      problem on the only host that could try. `compose` itself does not use this path yet
      (it still only gets cluster networking); DNS resolution through the gateway's
      intercept and a real two-guest TCP round trip are both unexercised.
- [x] Compose: bind mounts, `:ro` and options on volumes: DECLINED 2026-10-29 (owner delegated the
      call). The sandbox API has named volumes only; a host path would be a copy-in with no live
      link, and `:ro` needs a server change. Refused with a message; use a named volume.
- [x] Compose: health checks and `depends_on: condition: service_healthy`
      (`2026-10-28/compose-health-cluster-kvm`, 6 cases, 3 of 3 runs). Checked only during
      `up`.
- [x] Compose: `restart:` DECLINED beyond `"no"` 2026-10-29 (owner delegated the call): `"no"`/`false`
      is accepted (unit-tested), `always`/`on-failure`/`unless-stopped` stay refused. Reason: the node's restart supervisor
      (`machines.rs` `supervise`, `RestartPolicy` always/never) covers machines only; `POST /sandboxes`
      has no restart field and the v2 sandbox code has no restart logic. Needs an owner decision:
      a node change for sandboxes, or compose creating machines. `on-failure` also needs exit
      status recorded by the node. Checked by reading code, not run).
- [x] Build: `SHELL` (`2026-10-28/hm-build-shell-kvm`, 7-case build check, 3 of 3 runs).
- [x] Build: `RUN` here-documents (`2026-10-28/hm-build-heredoc-kvm`, 8-case build check,
      3 of 3 runs).
- [x] Build: `COPY <<EOF` (`2026-10-29/hm-build-copy-heredoc-kvm`, 10-case build check, 3 of 3
      runs). `ADD <<EOF` is still refused; an image without busybox is NOT verified.
- [x] Build: `ADD` of a local tar / tar.gz (`2026-10-29/hm-build-add-archive-kvm`, 9-case build
      check, 3 of 3 runs). bz2 and xz: `2026-10-29/hm-build-add-bz2-xz-kvm`, 12-case check, 3 of 3 runs. A tar-less
      image is NOT verified.
- [x] Build: `FROM` an earlier stage (`2026-10-29/hm-from-stage-kvm`, multistage check 7 cases,
      3 of 3 runs). `ENV`/`WORKDIR`/`CMD` are inherited, and `USER`/`ENTRYPOINT` too
      (`2026-10-29/hm-from-stage-user-kvm`, 8 cases, 3 of 3 runs).
- [x] Build: `RUN --mount` bind, tmpfs, cache (`2026-10-29/hm-build-run-mount-kvm`, 11-case build
      check, 3 of 3 runs). Copy-in design: no real mount, a cache does not persist, an existing bind
      target is removed afterwards; secret, ssh and `--network` are refused.
- [ ] Build: `ADD` of a URL (left refused, `RUN curl` does the same: build `RUN` steps have a network
      only when the node runs with `--network` (`builds.rs:583`), no hm check has fetched a URL from
      a build, and a guest cannot reach a host server at 10.0.2.2, so it could not be verified on
      KVM), `ONBUILD` (a base image's triggers would need the node to expose the image config).
- [ ] `hm sandbox vm run`: `-p` done (`2026-10-28/hm-run-publish-cluster-kvm`, 3 of 3);
      `-i` (stdin to EOF, `2026-10-29/hm-run-stdin-kvm`, 3 of 4 runs); `-v` named volumes
      (`2026-10-29/hm-run-volume-kvm`, 3 of 3); `--network PROJECT` (joins a compose
      project's network, `2026-10-29/hm-run-network-cluster-kvm`, 3 of 3); `-it` not done (the exec
      API has no tty).
- [x] A moved tag: DECLINED 2026-10-29 (owner delegated the call). `docker run` does not re-pull a
      moved tag either; the way to refresh is to build under a new name. Tried 2026-10-29: a client-only
      `run --pull` (delete the template, pull again) failed 3 of 3 runs on KVM, because the node
      refuses to delete a template built from an image (409 "only snapshots are deleted"), and
      a repeat `POST /templates` for the same name is a 409 "exists". Not shipped; it needs a
      node change (replace an image-built template, in a cluster too). A digest check also needs
      a manifest HEAD in `oci.rs` and the stored digest exposed by `GET /templates`.

## 3. Docker leftovers (Docker's engine in a guest: done; no more to be built)

- [x] **The one unexplained failure.** The published-port case failed once in 10
      runs with no error captured. Ran the full check 20 times with logs kept
      (`2026-10-28/docker-flake-chase`): the published-port case did not recur, but
      a different case failed once (a fork restored mid-build never answered on
      vsock, 1 in 20; 1 in 30 with the earlier fork runs). Cause not known.
- [ ] **Fork restore that never resumes** (run 11 above): a plain fork of a busy or idle
      sandbox did not reproduce it (0 in 560 forks, `2026-10-28/fork-busy-kvm`), but one
      cold boot of 14 stalled with both vCPUs halted and a timer armed. Cause unknown;
      next, catch one with the per-VM KVM counters sampled during the stall.
- [x] Published port reached from outside the node: a client in another network
      namespace over a veth pair, 3 of 3 runs (`2026-10-28/docker-netns-proxy-kvm`).
      A second machine is NOT VERIFIED.
- [ ] The default kernel: decide whether the Docker kernel becomes a shipped
      option or stays an opt-in build (`HV2_KERNEL`); document the choice.
- [ ] `docker compose build` (BuildKit) checkpointed mid-step, and a build that is
      pulling layers when checkpointed or forked.
- [x] A volume bigger than a few bytes, and a volume on a bind mount: 128 MiB volume
      and 64 MiB bind-mounted file, checkpoint, restore and fork, 3 of 3 runs
      (`2026-10-28/docker-big-data-checkpoint-kvm`). Data written during a checkpoint
      is NOT VERIFIED.
- [x] Remapped daemon (`--userns-remap`) together with compose, across a checkpoint,
      restore and fork: 3 of 3 runs (`2026-10-28/docker-userns-remap-compose-kvm`).
      A `build` under the remap is NOT VERIFIED.

## 4. Evidence quality (repeat what was run once)

Several results are one run. Each needs three runs in a row, kept.

- [x] Reflink checks: XFS, Btrfs, ext4 control, 3 of 3 runs each, extents shared on
      XFS and Btrfs every time (`2026-10-28/disk-checkpoint-repeat-kvm`).
- [x] Disk checkpoint on a `--disk-slot` node: 2 of 3 runs passed (run 1 got an empty
      `cat /tmp/count` after a restore); with the read retried, 3 of 3
      (`2026-10-28/disk-checkpoint-repeat-kvm/slot-retry`). Race in the check is likely,
      not proven. `--disk-slot` on a reflink filesystem is still NOT VERIFIED.
- [ ] XFS or Btrfs on a real block device, not a loop device; a disk much larger
      than 1 GiB.
- [x] `docs/BOAT_PARITY.md`: re-read against the 2026-10-21 to 2026-10-28 evidence
      (2026-10-28). Four stale statements fixed: "no Docker" in the tools image
      (twice) and "cannot be checkpointed at such a size". Rows outside that date
      range were not re-read: NOT VERIFIED.

## 5. Platform backends

- [x] **WHPX SMP** (done on one Windows host; see CHANGELOG `[Unreleased]`). The
      2026-10-21 attempt idled forever; the emulated-xAPIC route now works: Linux
      boots with 2, 4 and 8 vCPUs, reads and writes a PCI data disk and mounts a
      real Ubuntu root from a PCI disk with 2, and a 2-, 4- or 8-vCPU guest
      restores from a snapshot into a fresh VM. NOT VERIFIED: more than 8 vCPUs;
      interrupts in flight at snapshot; level-triggered I/O APIC lines; booting
      the Ubuntu root past the mount; KVM and HVF multi-vCPU snapshots (HVF
      refuses); any of it in CI (no kernel fixture there).
      Level lines, surveyed 2026-11-08 by reading the code and running a 2-vCPU
      Ubuntu guest (`/proc/interrupts`): the disk's INTx is programmed by the
      guest as EDGE (`10-edge virtio0`, 92 interrupts), so the I/O APIC's level
      path is not on the disk's path. Only the SCI (IRQ 9) is level, with 0
      interrupts, and nothing drives it. In the code, `IoApic::eoi` is called
      only from tests and `set_irq` lowers only the PIC, so a level line that
      a guest does program would stop after its first interrupt (remote IRR
      never cleared); a level line asserted while masked is not redelivered on
      unmask. Not fixed and not exercised by any guest here; a different guest
      kernel or an ACPI-routed PCI pin could reach it.
- [x] **WHPX: second VM in one process** (`0xC0370008`) was a callback kept alive
      by the first VM; it is weak now (CHANGELOG), and the fork tests start a
      second WHPX VM in the same process. NOT VERIFIED: the Botnix report's exact
      configuration (framebuffer, net device, other memory sizes).
- [x] **WHPX guest CPUID** reports the hypervisor-present bit clear (Botnix
      measurement). What each backend does (read from the code, 2026-10-28): KVM sets
      leaf 1 ECX bit 31 (`kvm.rs`, `advertise_hypervisor`) and takes 0x40000000 from
      KVM's supported set; WHPX configures no CPUID at all (`whpx.rs` sets only the
      processor count, no CpuidResultList, no CPUID exit handler), so it gets WHP's
      defaults; HVF has no CPUID emulation. `tsc_early_khz` and `tsc=reliable` are
      injected on WHPX and HVF only (`vm.rs:1081`). Done 2026-11-09, decided by the owner's "You decide": WHPX now exits on CPUID
      leaf 1 only, returns WHP's own result with ECX bit 31 added, steps past the
      instruction and re-enters (`whpx.rs`: the CPUID exit list `[1]` and the
      `X64CpuidExit` extended exit, which the list needs; the exit list alone did
      nothing). Measured on this host: the guest's `/proc/cpuinfo` lists `hypervisor`
      0 times before and once per vCPU after (1 and 2 vCPUs; the new assertion in the
      two root-disk tests fails on the old code). The 1-vCPU and 2-vCPU guests still
      calibrate the TSC the same way (PIT fails, PMTIMER reference, same MHz), the
      17 WHPX Linux tests pass before and after (94 s vs 104-120 s, 3 runs), and the
      mount median is 1.21 s with the change and 1.23 s without (40 interleaved runs
      each). Slow runs occur on both sides, unexplained: 3 of 92 with the change
      took 9.5, 16.5 and 120 s (the last is the boot deadline, a hang); 1 of 84
      without took 120 s. A boot hang of roughly 1 in 80 exists either way; whether
      the change alters its rate is NOT known. One hang's cause was found: a guest
      kernel panic, `IO-APIC + timer doesn't work!`, at 0.001 s on the 2-vCPU guest
      (1 of 142 runs idle; 9 of 60 under 24 busy host threads). `no_timer_check` does not cure it (7 of 28 boots still
      panicked with it on the command line; the 0-of-60 test-path result is not
      trustworthy). Open; cause unknown. Raising the vCPU threads' priority (HIGHEST) did not cure it either:
      8 of 40 boots panicked with it, 5 of 40 without, interleaved under the same 24 busy threads, so
      host scheduling of the vCPU thread alone does not explain it. Two other runs mounted in 7.0 and 9.4 s with no panic, unexplained.
      A candidate mechanism, read from the code 2026-11-11, not yet tested: on WHPX
      (and HVF) the PIT, PIC and I/O APIC are emulated in userspace, unlike KVM's
      in-kernel irqchip (`KVM_CREATE_IRQCHIP`/`kvm_create_pit2`), which never takes
      this path and has never shown the panic. `TimerDevice::tick` raises IRQ 0 on
      the `Pic8259` (`devices/timer.rs:466`); its line observer remaps IRQ 0 to GSI
      2 and calls `IoApic::raise_irq` (`vm.rs:1289-1291`); the I/O APIC's interrupt
      callback, when it decides to deliver, does not inject the interrupt directly --
      it does `runtime.spawn(async move { backend.request_lapic_interrupt(...).await
      })` (`vm.rs:1264-1275`), handing the actual WHP call
      (`WhpxBackend::request_lapic_interrupt`, `whpx.rs:415-429`, itself a fast
      direct call once it runs) to whatever tokio worker thread is free next. Under
      24 busy host threads, that hop queues behind other tokio work with no priority
      of its own; raising only the vCPU thread's priority (the fix already tried)
      would not touch this queue.
      Fixed and measured 2026-11-11: a new `HypervisorBackend::request_lapic_interrupt_sync`
      (default `None`) lets a backend answer without the trait's required `async`;
      WHPX implements it as a direct call to `WhpxVm::request_interrupt` (a plain FFI
      call, `whpx.rs:1065`), and the I/O APIC callback in `vm.rs` calls it first, only
      falling back to `spawn` when it answers `None` (a backend with no fast path).
      Three runs of 40 two-vCPU boots each (`apic=verbose`, the same cmdline that
      showed the panic), 24 busy host threads, `--test-threads=1`: 0 of 120 panicked
      with this fix, against 9 of 60, 7 of 28 (`no_timer_check`) and 8 of 40 (vCPU
      priority) before it. Two more batches of 40, same conditions, run 2026-11-11:
      0 of 80 panicked, bringing the total to 0 of 200. One of those two batches had
      one more unrelated hang (no panic string in the console), the second such hang
      across these runs. A further batch of 80, run 2026-11-11 with full console
      output captured on any non-pass to diagnose a repeat, came back 0 of 80
      panicked and no hang -- running total 0 of 280 panics, hang still unrepeated
      and uncaptured, still not claimed fixed. The 17 WHPX Linux tests (both
      fixture runs) and all 2448 `hv2-core` unit tests still pass. NOT VERIFIED:
      whether the earlier "0 of 60" false negative (noted above) means 280 more
      trustworthy than that one was -- a longer run is the honest next check, not
      claimed here; HVF (no sync override exists for it, since it never had one for
      the async path either); any effect in CI (no kernel fixture there); the
      no-panic-string hang's own cause (it has not recurred under capture).
      NOT VERIFIED:
      leaves 0x40000000+ (the vendor signature; the kernel logs no "Hypervisor
      detected" line, so Linux treats it as no known hypervisor); a Windows guest;
      any effect in CI (no kernel fixture); more than 2 vCPUs with the bit; HVF.
- [ ] **HVF (macOS), IRQ 12.** Needs a Mac. **OWNER** hardware.
- [ ] **Intel nested KVM (VMX).** Needs an Intel host. **OWNER** hardware.
- [ ] **vCPU reservation** (priority and affinity for vCPU threads, requested by
      Botnix): affinity already exists as `VMConfig::vcpu_affinity` but nothing sets
      it; no priority call exists anywhere. **OWNER** decision first: build it as a
      setting that is off by default, with a flag, and a way for a guest to tell
      it is in effect. Do not build it unasked: it changes scheduling on a shared
      host.
- [ ] Steal-time interface and a LAPIC timer under WHPX: the owner decides the
      effort; both are new work, and the latter is an unsolved problem.

## 6. Needs the owner

- [ ] **Publish the clients** (Python to PyPI, TypeScript to npm; needs a PyPI publisher, an npm scope and token, and a GitHub environment) and the tools image
      (179 MiB: where it is hosted is the owner's call). Do not tag a release
      without the owner's say.
- [ ] **A coding agent that needs a model key:** supply a key in an environment the
      owner controls, then check that the agent runs in a sandbox end to end.
- [ ] **CI secrets:** the repo has no Actions secrets, so the Slack notice and the
      Codecov upload fail quietly. Add them or remove the steps.
- [ ] Merging through a PR needs the owner when a change is not pushed directly.
- [ ] Hardware access for the HVF and Intel items above.

## 7. How every item is closed

Every box above is closed the same way:

- [ ] A check script under `tools/` that fails when the claim is false.
- [ ] Three runs in a row on the real platform, reports kept under
      `docs/benchmarks/<date>/<name>/` with a README that lists what was **not**
      verified. A failed run stays in the README with its count; it is not dropped.
- [ ] Docs and `CHANGELOG.md` updated, and the `BOAT_PARITY.md` row corrected.
- [ ] Before a push: `tools/check-no-local-paths.py`, `tools/find-unread-controls.py`,
      `tools/find-unreachable-states.py`. Then wait for all five workflows (CI,
      Security, Coverage, Deploy, Benchmarks) to be green on the previous commit
      before pushing the next: a push cancels the older runs.
- [ ] Anything not run is written down as NOT VERIFIED.

## 8. Done when

- [ ] Every row of `BOAT_PARITY.md` either says "done, with this evidence" or says
      exactly what is missing and why.
- [ ] Section 2 is resolved one way or the other, with measurements.
- [ ] The owner items in section 6 are done, or are explicitly declined.
- [ ] A final read of `BOAT_PARITY.md` against `docs/benchmarks/` finds no
      statement without evidence.

## 9. Suggested order

1. Section 3's flake chase (cheap, unblocks trusting every Docker number).
2. Section 4's repeats (cheap, strengthens what is already claimed).
3. Section 2 (the largest unknown; time-box it, and stop at "documented limit" if
   the evidence points below our layer).
4. Section 5's WHPX items that are bugs (second VM, CPUID bit), then the owner's
   choice on the vCPU reservation.
5. Section 6 as the owner provides what is needed.

## Status of the remaining items (2026-11-09)

- **Multi-vCPU WHPX boot panic under load** (`IO-APIC + timer doesn't work!`):
  fixed 2026-11-11, not yet confirmed closed. Delivering the I/O APIC's
  interrupt callback synchronously, instead of through a spawned async task
  that queues behind other tokio work under load, took the panic rate from
  9 of 60 (and 7 of 28, 8 of 40 with two other fixes that didn't work) to 0 of
  200 across five runs. See section 5 for the mechanism and what is not yet
  verified (a longer run; HVF; CI). Still in the v1.2.0 release notes as a
  known issue until more runs confirm it.
- **CI fixture for the WHPX Linux tests:** the recipe now exists and has been run
  (not in CI): `tools/guest-image/build-kernel.sh`, `build.sh`, and a new
  `build-root-disk-fixture.sh`, all built in WSL, make all 17 of 17
  `linux_whpx_boot.rs` tests pass genuinely (serial runs, `HV2_REQUIRE_GUEST=1`; two
  runs, since the 2 root-disk tests need `HV2_TEST_INITRD` unset). See
  `docs/benchmarks/2026-11-10/whpx-boot-fixture-windows/README.md`. Still OPEN:
  wiring it into CI (GitHub-hosted Windows runners likely lack WHPX/nested-virt,
  not tested) and hosting the built GPL kernel/initrd/disk image are both the
  owner's call, deliberately not done here.
- **Nested-guest re-run (section 2):** done 2026-11-11. Rebuilt the WSL
  fixtures (kernel, tools image, release builds) and re-ran the probe: 19.3 s
  guest time and 15,641 exits in 60 s, no userspace -- matches the 2026-10-27
  baseline (17.7 s, 15,643 exits) within its own run-to-run spread. The
  2026-10-27 evidence (about 10 times slower, no userspace after 270 s, no
  variant helped) stands and is now confirmed current. See
  `docs/benchmarks/2026-11-11/nested-boot-rerun/README.md`. The rest of
  section 2 (memory-light test, counting where time goes, a non-nested host)
  is still open.
- **OWNER items, not mine to close:** publishing the clients, the CI secrets (Slack,
  Codecov), HVF and Intel hardware, and the decision on hosting a fixture.
- **Level-triggered I/O APIC lines:** OPEN, documented above; no guest reaches it.
