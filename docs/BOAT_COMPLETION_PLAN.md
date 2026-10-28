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

- [ ] Re-run the nested boot on the current build and keep the baseline numbers
      (guest time reached in 60 s, userspace exits) as the comparison point.
- [ ] Memory-light test: a nested guest that does little page-table work, to see
      whether the slowness follows memory activity (nested paging under Hyper-V
      is the leading guess and is a guess).
- [ ] Count where time goes: sandbox-side `perf kvm` or `trace-cmd`, and the
      per-VM KVM counters sampled every few seconds, not twice.
- [ ] Try the same nested boot on a host that is not Hyper-V-nested (a bare-metal
      Linux box or a cloud bare-metal instance) to separate "our KVM backend" from
      "the layer below". **OWNER** if hardware is needed.
- [ ] Decide from the evidence: either fix it, or record it as a limit of nested
      virtualization under WSL2/Hyper-V with the measurements, and mark the
      `BOAT_PARITY.md` row accordingly. Do not claim more than was shown.
- [ ] If it boots: a checkpoint and a fork while a nested VM is running, and the
      nested guest across a restore.

## 3. Docker leftovers

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
- [ ] A volume bigger than a few bytes, and a volume on a bind mount.
- [ ] Remapped daemon (`--userns-remap`) together with compose and build, across a
      checkpoint.

## 4. Evidence quality (repeat what was run once)

Several results are one run. Each needs three runs in a row, kept.

- [ ] Reflink checks: XFS, Btrfs, ext4 control (one run each today).
- [ ] Disk checkpoint on a `--disk-slot` node, and `--disk-slot` on a reflink
      filesystem.
- [ ] XFS or Btrfs on a real block device, not a loop device; a disk much larger
      than 1 GiB.
- [x] `docs/BOAT_PARITY.md`: re-read against the 2026-10-21 to 2026-10-28 evidence
      (2026-10-28). Four stale statements fixed: "no Docker" in the tools image
      (twice) and "cannot be checkpointed at such a size". Rows outside that date
      range were not re-read: NOT VERIFIED.

## 5. Platform backends

- [ ] **WHPX SMP.** The 2026-10-21 attempt (hypervisor-emulated xAPIC) idled
      forever and was reverted. Re-scope before retrying: why no tick, and whether
      a software LAPIC timer is the other route.
- [ ] **WHPX: second VM in one process** fails at map with `0xC0370008` (reported
      by Botnix; cause unknown, not reproduced). Reproduce on this host if WHPX is
      enabled; log retry count and elapsed time at the failure site; test with and
      without framebuffer and net device, and at different memory sizes.
- [ ] **WHPX guest CPUID** reports the hypervisor-present bit clear (Botnix
      measurement). Decide whether to set it and add the `0x40000000` signature
      leaf; check what each backend does.
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
