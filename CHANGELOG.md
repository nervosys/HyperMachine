# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Private VM networks: a sandbox can reach another by its compose service name
  (`web.<project>.hv2.internal`) when both have the same owner, the service label and the project
  tag match, and exactly one sandbox matches. `hm sandbox vm compose up` now joins each service to
  the private network named after the project and sets a `search <project>.hv2.internal` line in the
  guest, so a service reaches a sibling by name (`nc server 8080`). On a node without private
  networks it says so and carries on. Checked on KVM on one cluster node (6 cases, 3 of 3 runs);
  two nodes and UDP are NOT VERIFIED.
- `hm sandbox vm build` builds multi-stage Dockerfiles: `FROM x AS name`, `COPY --from=name|N` of a
  file, a directory or a glob, chains of stages. Each earlier stage is built as a hidden template and
  its files are read out of a sandbox of it and uploaded as a context; no daemon change. Refused by
  name: `FROM` an earlier stage and `COPY --from=IMAGE`. 5 cases, 3 of 3 runs, and the 6-case build
  check 3 of 3 (`docs/benchmarks/2026-10-28/hm-multistage-kvm/`). Failure cleanup of the hidden
  templates is not verified.
- A compose project under `--userns-remap` survives a checkpoint, a restore and a fork (both containers
  running, the volume's data intact): 3 of 3 runs; the first version of the check failed 4 times on a
  mistake in the check itself (`docs/benchmarks/2026-10-28/docker-userns-remap-compose-kvm/`).
- A 128 MiB Docker volume and a 64 MiB bind-mounted file are byte for byte as they were at a checkpoint,
  and a fork has them: a new case in `check-docker-sandbox-kvm.py`, 3 of 3 runs
  (`docs/benchmarks/2026-10-28/docker-big-data-checkpoint-kvm/`).
- Disk checkpoints, three runs in a row each: XFS, Btrfs and ext4 10 of 10 every time (extents shared on XFS
  and Btrfs); a `--disk-slot` node 2 of 3, the failed run an empty read of the guest's counter after a restore,
  likely a race in the check (`docs/benchmarks/2026-10-28/disk-checkpoint-repeat-kvm/`).
- `tools/check-fork-busy-kvm.py`: 560 of 560 forks of a busy or idle sandbox answered; the mid-build fork
  failure did not reproduce without Docker. One cold boot in 14 stalled (both vCPUs halted, timer armed),
  cause unknown (`docs/benchmarks/2026-10-28/fork-busy-kvm/`).
- A published port reached through the node proxy from a second network namespace (veth pair), with
  the namespace's own loopback as a control: `check-docker-sandbox-kvm.py --netns`, 3 of 3 runs
  (`docs/benchmarks/2026-10-28/docker-netns-proxy-kvm/`). One kernel, not a second machine.
- 20 runs of the 18-case Docker check with logs kept: 19 passed; run 11 failed in the mid-build
  fork case (the restored fork never answered on vsock), cause unknown
  (`docs/benchmarks/2026-10-28/docker-flake-chase/`).
- Disk checkpoints on Btrfs (loop-mounted): 10 of 10 cases, and the disk copy shares extents, as on XFS.
  One run (`docs/benchmarks/2026-10-25/disk-checkpoint-reflink-kvm/btrfs/`).
- A fork in the middle of `docker build`: the build finishes in the fork and the parent. 18 cases,
  3 clean runs (9 of 10 runs clean); one run failed in the existing published-port case, cause unknown
  (`docs/benchmarks/2026-10-26/docker-build-fork-kvm/`).
- A checkpoint in the middle of `docker build` with a two-container compose project running: after
  the restore the build finishes by itself and the project is back. 17 cases, 3 runs in a row
  (`docs/benchmarks/2026-10-26/docker-build-checkpoint-kvm/`).
- `--userns-remap` in a sandbox: a container's root maps to uid 100000, and a file it writes is owned
  by that uid on the guest. 16 cases, 3 runs in a row
  (`docs/benchmarks/2026-10-25/docker-userns-remap-kvm/`).
- A `RUN` step that needs the network during `docker build` in a sandbox: it fetched from a
  server on the guest's bridge. 15 cases, 3 runs in a row
  (`docs/benchmarks/2026-10-25/docker-build-network-kvm/`).
- A Docker named volume across a checkpoint and a fork, on real KVM: the data is as it
  was at the checkpoint after a restore, and a fork has it. 14 cases, 3 runs in a row
  (`docs/benchmarks/2026-10-25/docker-volume-checkpoint-kvm/`).
- Disk checkpoints on a `--disk-slot` node: `tools/check-disk-checkpoint-kvm.py --daemon-arg=--disk-slot`,
  10 of 10 cases, one run (`docs/benchmarks/2026-10-25/disk-checkpoint-slot-kvm/`).
- Disk checkpoints on a filesystem with reflinks: `tools/check-disk-checkpoint-kvm.py --work-dir`
  now records whether the checkpoint copy shares extents with the disk. On a
  loop-mounted XFS (`reflink=1`) it did and 10 of 10 cases passed; on ext4 it
  did not and 10 of 10 passed (`docs/benchmarks/2026-10-25/disk-checkpoint-reflink-kvm/`).
  One run each. NOT VERIFIED: Btrfs, a real block device, `--disk-slot` nodes.
- `/dev/kvm` in a sandbox: `build-kernel.sh --kvm` (`tools/guest-image/kvm.fragment`).
  Checked on real KVM on an AMD host, 2 cases, 3 runs in a row
  (`tools/check-kvm-sandbox-kvm.py`, `docs/benchmarks/2026-10-23/kvm-sandbox-kvm/`):
  a program in the guest creates a VM and runs a vCPU to an I/O exit and HLT.
  NOT VERIFIED: Intel hosts, a real guest inside the sandbox, checkpoint or fork with a VM running.
- Docker together with `/dev/kvm` in a sandbox: `check-docker-sandbox-kvm.py --nested`
  runs a nested VM to an I/O exit and HLT from the guest and from a container given
  `--device /dev/kvm`, on a `--docker --kvm` kernel. 14 cases, 3 runs in a row on KVM
  (`docs/benchmarks/2026-10-24/docker-kvm-sandbox-kvm/`). NOT VERIFIED: Intel hosts,
  a real guest OS in the nested VM, a checkpoint with a nested VM running.
- Docker's engine in a sandbox: `build-tools-image.sh --docker docker-X.tgz` and
  `build-kernel.sh --docker` (`tools/guest-image/docker.fragment`: memory cgroups,
  overlayfs, veth, bridge, user namespaces, NAT, iptables raw). The guest uses
  legacy iptables and a `runc-nopivot` runtime, since the root is an initramfs.
  `--compose BIN` adds `docker compose` (the release binary, as a CLI plugin).
  Checked on real KVM, 13 cases, 3 runs in a row (`tools/check-docker-sandbox-kvm.py`,
  `docs/benchmarks/2026-10-23/docker-sandbox-kvm/`), including `docker build` from
  `scratch` and from a pulled `busybox`, a published port reached through the node's
  port proxy, a checkpoint and fork that carry a running container, and a two-service
  `docker compose up` with a volume that outlives `down`, and a checkpoint and fork
  that carry the running project, and `docker compose build` (BuildKit runs `runc` from PATH, so the no-pivot wrapper is now installed as `runc` too), and two projects side by side.
- `build-tools-image.sh --systemd` builds a sandbox template whose first process is
  systemd (Ubuntu 24.04, `tools/guest-image/init-systemd`; `build.sh` takes `--init`).
  Checked on real KVM, 5 cases, 3 runs in a row (`tools/check-systemd-sandbox-kvm.py`,
  `docs/benchmarks/2026-10-23/systemd-sandbox-kvm/`). NOT VERIFIED: Docker,
  `/dev/kvm` in the guest, OCI-built images, other distributions, several nodes.
- A team can share a snapshot with another team: `PUT`/`DELETE
  /templates/{id}/shares/{team}` and `GET /templates/{id}/shares` on the control
  plane. The team shared with lists it and starts sandboxes from it; it cannot
  read, extend, revoke or delete the share (404). Revoking stops new starts at
  once. Nothing is scrubbed from a shared snapshot, so the request must carry
  `acknowledgeContents: true`. The list is kept in the snapshot store and
  survives a node restart. Checked on real KVM through a control plane with three
  teams, 6 cases, 3 runs in a row (`tools/check-snapshot-share-kvm.py`,
  `docs/benchmarks/2026-10-23/snapshot-share-kvm/`). NOT VERIFIED: several
  nodes (the share is sent to each and the store is followed every 2 seconds), a
  node that was down when a share was revoked, and any removal of secrets.
- A machine sends webhooks: `machine.lifecycle.created`, `.started`,
  `.stopped` and `.deleted`, signed as a sandbox's are, to a webhook that names
  them in `events` (one with no list keeps getting sandbox events only) and
  only to the machine's own team's. The payload has `machine_name` where a
  sandbox's has `sandbox_id`; `GET /events/sandboxes` does not list them.
  Checked on real KVM through a control plane with two teams and a loopback
  receiver, 5 cases, 3 runs in a row (`tools/check-machine-webhooks-kvm.py`,
  `docs/benchmarks/2026-10-23/machine-webhooks-kvm/`). NOT VERIFIED: a guest
  that stops by itself or a node restart (no event), snapshots (no event), a
  failing receiver for a machine event, several nodes.
- Compose `ports:` checked on real KVM through a real control plane and
  `hv2-native-gateway` (`tools/check-compose-ports-cluster-kvm.py`,
  `docs/benchmarks/2026-10-23/compose-ports-cluster-kvm/`): `up` reserves the
  container port and prints the public one, a TCP client of it reaches the
  service's listener in the guest, another principal cannot see the
  reservation, `down` closes the port, and a key with no `principal_id` fails
  naming the port and leaves no sandbox. 6 cases, 3 runs in a row. Found on the
  way: a team-scoped key is refused (403) the template build `compose` makes for
  an `image:` service. NOT VERIFIED: UDP/`both` through compose, several nodes,
  more than one published port.
- A sandbox that holds a data disk can be checkpointed by name (it was
  refused with 409). The node copies the disk's image beside the memory file
  while the guest is paused, so the two are of one moment. Restoring makes a
  new disk from the copy (`NAME-restored-NNNNNNNN`), starts the replacement on
  it, and only then swaps, letting go the disk the sandbox held, unchanged;
  a restore that fails deletes the new disk and the sandbox keeps what it had.
  Automatic checkpoints and templates from a sandbox with a disk stay
  refused. Checked on real KVM, 10 cases (`tools/check-disk-checkpoint-kvm.py`,
  `docs/benchmarks/2026-10-23/disk-checkpoint-kvm/`), including a restored
  sandbox checkpointed again and a 1 GiB disk with 400 MiB of data restored
  byte for byte. NOT VERIFIED: a filesystem with reflinks (WSL's ext4 has
  none, so the copies were full), `--disk-slot` nodes, and disks much beyond
  1 GiB. A filesystem with reflinks is now checked: see the entry above.
- Checked on real KVM with Ubuntu 24.04's cloud image (a GPT disk with four
  partitions): a machine's snapshot is listed and archived from its root
  partition without one being named, the archive equal file for file to the
  disk mounted through a loop device, and a partition that is not the root
  filesystem refused. In the same guest, `/dev/kvm` works: a program created
  a VM and ran a toy real-mode program under nested virtualization (AMD host,
  WSL2); sandboxes, whose template kernel has no KVM, have none
  (`tools/check-machine-snapshot-ubuntu-kvm.py`,
  `docs/benchmarks/2026-10-23/machine-ubuntu-snapshot-kvm/`). NOT VERIFIED:
  an Intel host, a larger nested guest.
- `tools/check-team-telemetry-kvm.py --otelcol PATH` runs the team telemetry
  check with the real OpenTelemetry Collector (contrib 0.163.2, bearer-token
  auth) as red's destination: 7 cases pass on KVM, metrics read back from the
  collector, logs and traces accepted
  (`docs/benchmarks/2026-10-23/team-telemetry-otelcol-kvm/`). Usage in the
  push stays a rolling 24 hours: a running total would read the whole,
  never-trimmed ledger on every push.
- Webhook deliveries carry `hv2-signature` (`v1=` HMAC-SHA256 over
  `<timestamp>.<body>`) and `hv2-signature-timestamp`, beside the unchanged
  `e2b-signature` that E2B's verifiers expect. Rotating `signatureSecret`
  keeps the old secret signing too for `overlapSeconds` (a day by default, 0
  for none, a week at most): the header then holds two `v1=` values. See
  `docs/WEBHOOKS.md`. 3 unit tests, one a loopback delivery after rotations,
  one against RFC 4231's vector. NOT VERIFIED: another vendor's verifier,
  and a webhook stored by an older node is read as having no old secret.
- `GET /machines/{name}/snapshots/{id}/archive?path=&partition=` downloads a
  directory of a machine's disk snapshot (`/` for all) as a gzipped tar,
  without booting it: `debugfs rdump` into a scratch directory of the
  node's own, then `tar | gzip`, then the scratch is removed. Links are
  kept; capped at 64 MiB of archive, 256 MiB or 500,000 entries read (413)
  and 3 minutes (504). Routed through the control plane too. Checked on real
  KVM (4 cases, `tools/check-machine-snapshot-archive-kvm.py`) and 2 unit
  tests on a `mkfs.ext4 -d` image. NOT VERIFIED: a partitioned disk image or
  Ubuntu's cloud image (the fixtures are gone), a filesystem that was not
  cleanly unmounted, the archive over 64 MiB, and a non-root node's
  ownership on KVM (a non-root unit-test run passes). The route through a
  control plane and Redis is checked, for its team only
  (`docs/benchmarks/2026-10-23/machine-archive-cluster-kvm/`).
- `--team-git-credential TEAM=PREFIX=VAR` (repeatable): a team's sandboxes'
  own `git` is given the team's credential by the gateway, before the
  node's `--git-credential-in-sandbox` ones; other teams' sandboxes and ones
  with no team never get it, and two teams may hold different tokens for one
  prefix. Held only on the host. Needs a clustered node and a cluster token
  (attribution), and `--network`. Checked on real KVM, 4 cases
  (`tools/check-team-git-kvm.py`, `docs/benchmarks/2026-10-22/team-git-kvm/`)
  and a unit test. Also checked through a real control plane and Redis, the
  team coming from the API key (3 cases,
  `tools/check-team-git-cluster-kvm.py`,
  `docs/benchmarks/2026-10-23/team-git-cluster-kvm/`). NOT VERIFIED: a real
  GitHub or GitLab, and a server-side environment clone (it does not use the
  team's credential).
- A sandbox's own `git` can use the node's `--git-credential` without the
  guest holding it: with `--git-credential-in-sandbox`, the gateway sets
  `Authorization: Basic` on a guest's HTTPS request whose scheme, host, port
  and path prefix are the credential's URL, replacing any the guest wrote
  (new `hv2_net::host_credentials`, `Gateway::set_host_credentials`,
  `mitm::intercept_with`). A look-alike host, another port, a path outside
  the prefix and one that climbs out with `..` or an encoded dot or slash
  get none. It applies to every sandbox on the node, not per team. Checked
  on real KVM with the host's git and curl in the guest, 6 cases
  (`tools/check-git-in-sandbox-kvm.py`,
  `docs/benchmarks/2026-10-22/git-in-sandbox-kvm/`), and 7 unit tests.
  NOT VERIFIED: a real GitHub or GitLab (the server is a Python dumb-protocol
  one), `git push`, the smart protocol, HTTP/2, a credential per team or per
  sandbox, and a Debian-based template (the guest was busybox plus copied
  binaries).
- A sandbox of a size of its own can now be snapshotted into a template
  (`POST /sandboxes/{id}/snapshots`); it was refused. The snapshot keeps the
  size beside its base, in the store record too (`cpuCount`, `memoryMB`,
  absent on older records), and a sandbox started from it is that size; the
  node advertises it at that size for placement. Asking for another size on
  that create is refused, and a template build on such a snapshot is
  refused. Checked on real KVM with the sized-checkpoint script (case 5) and
  a unit test of the record. NOT VERIFIED: a store shared between nodes
  (another node reading the new fields), a control plane placing by the
  advertised size, and a snapshot above 1 GiB.
- A sandbox of a size of its own (`cpuCount`, `memoryMB`, or resized) can now
  be checkpointed, by hand or automatically; it was refused. Its memory is
  written whole, as a pause writes it. A checkpoint records the size it was
  taken at and a restore is at that size, so a sandbox resized after a
  checkpoint is restored to the checkpoint's size and its record updated (it
  was restored at the sandbox's size at the time, whatever the checkpoint
  held); the checkpoint file reader boots at that size too. Checked on real
  KVM in WSL2, 4 cases (`tools/check-sized-checkpoint-kvm.py`,
  `docs/benchmarks/2026-10-21/sized-checkpoint-kvm/`). Still refused: a
  sandbox with a data disk, and making a sized sandbox a template. NOT
  VERIFIED: a checkpoint of a sandbox restored from a sized template, memory
  sizes beyond 1 GiB, and a checkpoint index surviving a node restart (it
  never did).
- A team's telemetry destination ending in `/v1/metrics` is now also sent
  the team's sandbox events (created, paused, resumed, deleted, expired, lost,
  forked) as OTLP logs, to `/v1/logs` under the same operator prefix and
  headers: oldest first, at most 500 a push, once each (a cursor on time plus
  the ids at that millisecond), again after a refused push, and nothing from
  before the destination or the control plane began. Tested against a
  loopback collector (`team_telemetry::tests`, 4 new tests: the path swap, the
  cursor, per-team filtering with oldest-first order and no repeat, retry
  after a 503). NOT VERIFIED: against a real OpenTelemetry Collector or
  backend, a Redis-backed store's event tail across control planes, a team
  with more than 500 new events in one interval under load, and traces or
  command output (not built).
- A team's telemetry destination ending in `/v1/metrics` is also sent a
  span to `/v1/traces` for each sandbox that has ended (created or forked to
  deleted, expired or lost; pauses and resumes as span events; ERROR for a
  loss), once each, kept on a cursor apart from the logs' so one signal's
  failure neither repeats nor drops the other. Tested against a loopback
  collector (3 new `team_telemetry` tests). NOT VERIFIED: a real
  OpenTelemetry Collector or tracing backend, spans inside a sandbox, and a
  sandbox whose start has left the event tail (not sent).
- `GET /sandboxes/{id}/checkpoints/{name}/archive?path=` downloads a
  directory of a sandbox's checkpoint (`/` for everything) as a gzipped tar,
  built in the same reader guest as `/files` and `/file`, so the sandbox and
  the checkpoint are untouched. Modes, owners and symlinks are kept; for `/`
  the kernel's `/proc`, `/sys`, `/dev` and `/run` are left out; at most 64 MiB
  compressed (413). The control plane forwards the route. Verified on KVM
  (WSL2) in a busybox guest: [`tools/check-checkpoint-files-kvm.py`](tools/check-checkpoint-files-kvm.py),
  7 cases, [evidence](docs/benchmarks/2026-10-21/checkpoint-archive-kvm/README.md).
  NOT VERIFIED: GNU tar in the distribution template, an archive over 64 MiB,
  the route through a running control plane, and a machine snapshot's archive
  (not built).
- A Linux guest on a non-KVM backend is told its TSC frequency
  (`tsc_early_khz=`, measured once on the host over 100 ms) unless the command
  line already sets `tsc_early_khz` or `tsc=`. Before, the guest timed its TSC
  against the PIT through port-I/O exits, failed ("Unable to calibrate against
  PIT") whenever the exits were slow, fell back to the tick as its only clock and
  ran at about half speed (a 30 s busy loop took 65 s of host time). Verified on
  WHPX with a Debian 13 cloud kernel: `Switched to clocksource tsc-early`, and
  the root mounts at guest 5.03 s after 6.76 s of host time including VM
  setup. NOT VERIFIED: a long-running guest's drift against the host (the
  frequency is a 100 ms measurement), HVF, and a host whose TSC is not invariant.
  `tsc=reliable` is appended with it (unless `tsc=` is set): the watchdog's
  only reference is the tick, which loses interrupts here, and it marked the
  TSC unstable 1.7 s into boot. Measured by the Botnix session on Debian 13
  with both flags: guest time within 10 ms of host time over a 30 s busy loop
  and seven `sleep 10` (9.998 to 10.022 s); with the frequency alone, 5% slow.
  The cost: nothing checks the TSC, so the host's must be invariant.
- `VM::attach_data_pci`: a third, writable virtio-blk disk on PCI (slot 7, BAR
  0xd0070000, IRQ 7), `/dev/vdc` after the root and seed disks, for a guest to
  hand results back through a raw image the host reads once the guest has
  synced and the VM is stopped. Verified on WHPX with a busybox guest that
  writes a computed line and syncs (`HV2_TEST_DATA`; probe: `HV2_DATA`). NOT
  VERIFIED: on KVM or HVF, with a Debian root disk, and a guest killed before
  `sync` (its page cache is lost, by design of the guest, not the device).
- A WHPX guest can have more than 3 GiB of RAM: the partition maps the ranges of
  `memory::ram_ranges` (RAM below the 0xC0000000 device hole, the rest from
  4 GiB) from one host buffer, as the KVM backend does, and host-side reads and
  writes follow the same map. Debian 13 boots with 4, 8 and 16 GiB (e820 shows
  the two ranges; `HV2_TEST_MEMORY_MIB` in `tests/linux_whpx_boot.rs`). Before,
  one block was mapped at GPA 0 across the PCI window. Boot images are placed
  below the hole; one vCPU only; HVF NOT verified.
- A WHPX guest calibrates its TSC and runs on the `tsc` clocksource: port 0x61
  (PIT channel 2's gate and output, the speaker bit, the refresh toggle) now
  exists, so the kernel's calibration against channel 2 measures real time
  ("tsc: Using PIT calibration value", "Switched to clocksource tsc"). The port
  sits inside the i8042's range, so the keyboard device carries it. Debian
  13 reaches its login prompt in under 20 s of host time. Verified on WHPX only;
  KVM has an in-kernel PIT and port 0x61 and is untouched. HVF NOT verified.
- A second, read-only disk reaches a WHPX guest: `attach_seed_pci` (existing) adds
  a virtio-blk device at PCI slot 6, `/dev/vdb` beside a root disk, and the
  guest reads it (`tar -xOf` a tar stream with no filesystem prints the file;
  `HV2_TEST_SEED` in `tests/linux_whpx_boot.rs`). Debian's kernel reports
  `[vdb]` with the right size next to the root `[vda]`. One extra disk only;
  writable and further disks are NOT supported, and nothing was verified on HVF.
- A root filesystem on a virtio-blk disk over PCI mounts on WHPX
  (`a_root_filesystem_mounts_from_a_virtio_blk_disk_on_pci`, `HV2_TEST_DISK`:
  Debian 13's 6.12 kernel and initramfs mount `/dev/vda1` read-only and start
  systemd in about 1.2 s). Three things were missing, each alone enough to stall
  it: an MMIO exit now decodes the instruction behind it (`backends/x86_mmio.rs`:
  `mov`/`movzx`/`movsx`/`movsxd` loads and stores, length computed because WHP
  reports none) so a load gets the device's answer and RIP steps over the
  instruction (before, the access carried the instruction's first bytes as data
  and the guest re-executed it); the device model now shares the partition's RAM
  (`guest_memory_host_addr`), so a virtio device reads the rings the guest
  wrote; and the probe stops attaching a second `Machine` whose PCI root is not
  the VM's. Not handled: `rep movs` and read-modify-write forms (refused with
  their bytes), RAM above the PCI window (guest RAM is one block at GPA 0), and
  a guest clock that runs slow (see the next entry).
- A WHPX guest's clock runs about 2x slow instead of about 15x: on Windows 11
  a timer-resolution request can be ignored for a process with no window, which
  left every wait at about 15.6 ms, so the process now says (power-throttling
  class, `IGNORE_TIMER_RESOLUTION` clear) that it must be honoured, and a
  halted vCPU wakes when a line is raised instead of waiting out its poll.
  Measured on WHPX: `sleep 1` in the guest takes 2.2 s of host time (it took
  14.5 s). The guest still falls back to the jiffies clocksource because TSC
  calibration against the PIT fails, and ticks raised while one is waiting are
  merged, which is the remaining slowness.
- A WHPX guest that goes idle is woken again. `interrupts_deliverable` ignored
  an interruption already queued in the vCPU's single pending-interruption
  slot (injected on an earlier entry, the run cancelled before the guest took
  it), so the next injection overwrote it. The lost vector's 8259 in-service
  bit stayed set and masked that line for good; when it was IRQ 0 the guest's
  timer stopped and `sleep 1` never returned. Verified on WHPX by
  `tests/linux_whpx_boot.rs` (`HV2_TEST_INITRD_SHELL`: `sleep 1` then an
  echo; fails without the change). NOT fixed: guest time runs about 12x slow
  (TSC calibration against the PIT fails, so the clocksource is `jiffies`, and
  ticks arrive at the host's coarse timer rate), so `sleep 1` takes about 12 s.
- Device interrupts reach a WHPX guest. Port-I/O devices (the UART's IRQ 4
  transmit interrupt, and anything else behind `IoOutcome.interrupt` or the
  interrupt sink) were driven through `set_irq_line`, which does nothing on a
  backend without an in-kernel controller, so a guest's userspace console
  output never arrived (kernel printk polls and was unaffected). They now raise
  the userspace 8259 there. Verified on WHPX with an initramfs that prints from
  `/init` and runs a shell (`HV2_TEST_INITRD_MARKER` in `tests/linux_whpx_boot.rs`;
  fails without the change). KVM is untouched (its tests pass); IRQ delivery on
  HVF is NOT verified.
- A Linux kernel boots to userspace on WHPX (`tests/linux_whpx_boot.rs`, 1 vCPU,
  1 GiB, `nolapic noapic`; KVM and 30 hv2-core test binaries unaffected). WHPX
  now enters Linux through the 32-bit boot protocol the loader lays the images
  out for (it used the 64-bit entry, which faulted on the first instruction),
  and a pending PIC interrupt now cancels a running vCPU so a guest spinning on
  a timer tick gets it. Block root, network, SMP and RAM above 3 GiB on WHPX
  are not done.
- `hm sandbox vm compose` accepts named volumes: a top-level `volumes:` declares
  them, a service mounts one with `name:/path`. They are created as
  `PROJECT-NAME`, mounted over 9P, and survive `down`. Bind mounts, `:ro` and
  volume options are refused. Checked on KVM (8 of 8 cases, 3 of 3 runs).
- `hm sandbox vm compose logs [SERVICE] [--tail N]` prints the tail of each
  service's command output (`/tmp/hm-compose.log`). Checked on KVM (3 of 3 runs).
- `hm sandbox vm compose` accepts `ports:` (short syntax): each container port
  becomes a public-port reservation, printed as `service: container port N/tcp
  is public port M`. The gateway chooses the public number, so a requested host
  port is not honoured; ranges and the long syntax are refused. Parsing is unit
  tested; the compose-to-gateway path is NOT verified end to end.
- `VM::send_pointer(dx, dy, buttons)` moves the guest's PS/2 mouse. The i8042
  now has an auxiliary port: `0xA7`/`0xA8`/`0xA9`, `0xD4` forwarding to the
  mouse, status bit 5 on mouse bytes, and IRQ 12 when the guest sets bit 1 of
  its command byte. `dy` is positive up (PS/2); a move over 255 on an axis is
  split into packets, and a move that does not fit the 16-byte output buffer
  is refused whole. Refused too until the guest enables reporting (`0xF4`).
  Verified by a polling guest on WHPX and KVM (`tests/pointer_delivery.rs`, 6 of
  6 KVM runs). IRQ 12 delivery to a guest is NOT verified on WHPX/HVF.
- `VM::send_serial(&[u8])` sends bytes to the guest's COM1 as if typed at its
  serial console; the guest reads them from port 0x3F8 when line-status bit 0
  is set, and its replies come back through `console_output`. A host may leave
  at most 4096 bytes unread (it grew without bound before); more is refused and
  none of the call is queued. A guest polling COM1 got `abc
` echoed as `bcd
`
  and 600 more bytes in order, none lost, on WHPX and KVM; the test fails
  without the limit. The receive interrupt (IRQ 4) is not delivered on WHPX or
  HVF, so a guest there polls.
- A VM can have a linear framebuffer: `VM::attach_framebuffer(width, height,
  gpa)` before provisioning maps host memory into the guest outside RAM
  (`XRGB8888`, rows packed), so a guest draws with plain stores and no VM exits,
  and `VM::screendump()` returns the raw bytes with width, height, pitch and the
  channel shifts. The guest is told through the Multiboot information structure
  (flag bit 12, fields at offsets 88-115); a kernel not booted by Multiboot is
  refused. A guest read the address and size from the info, drew the first and
  last pixels, and the host read them back exactly, on WHPX and on KVM; the test
  fails when the buffer is mapped at the wrong address. Not done: a graphics mode
  the kernel's header asks for is ignored (the size is the caller's), the
  framebuffer is not in snapshots, and a frame read while the guest draws can be
  half done. `MultibootInfo` gained a `framebuffer` field.
- `VM::send_key(&[u8])` sends PS/2 set-1 scancodes (a press is `0x1E`, its
  release `0x9E`, extended keys carry `0xE0`) to the guest's keyboard
  controller; up to 16 bytes at once, all or nothing. On WHPX and HVF the
  keyboard now raises IRQ 1 on the VM's interrupt controller (before, it only
  tried the hypervisor's, which does nothing there), and raises it again as the
  guest drains each byte: a guest that reads one byte per interrupt used to see
  the first byte of a make/break sequence and never the rest. A halted guest
  receives six bytes in 3 to 7 ms on WHPX, and the test fails without the
  re-raise. On KVM the interrupt goes through the queued sink, which cannot
  re-raise exactly (a stale pulse read an empty buffer, a seventh IRQ 1 for six
  bytes), so it is not done there; six bytes arrived intact in 6 of 6 runs, but
  a coalesced pulse could lose one. No REST route or CLI command yet.
- `hm sandbox vm build -t NAME [CONTEXT]` builds a Dockerfile (FROM, ARG, ENV,
  WORKDIR, COPY, RUN, USER, ENTRYPOINT, CMD) into a template with no Docker,
  uploading the context with `.dockerignore` honoured, and
  `hm sandbox vm run --template NAME` runs it with its ENTRYPOINT and CMD. The
  build API accepts `ENTRYPOINT` and `CMD` steps and records them in the
  template. Checked on KVM (6 cases, 3 runs); multi-stage builds are refused.
- `hm sandbox vm run` and `build` take `--registry-user` and
  `--registry-password-env VAR` to pull from a private registry; the password
  comes from the environment and is never echoed or logged. Checked on KVM
  against a Basic-auth registry on localhost (6 cases, 3 runs).
- `hm sandbox vm compose -f FILE [-p NAME] up|down` starts the services of a
  compose file (`image` or `build`, `command`, `environment`, `depends_on`),
  each in a VM of its own, detached and in dependency order, and removes them.
  No `ports`, `volumes`, networks between services or health checks yet (the
  first two are refused). Checked on KVM (6 cases, 3 runs). Adds
  `serde_yaml_ng`; its `unsafe-libyaml` is unmaintained and is accepted in
  `deny.toml` with a reason.

### Security
- A sandboxed Linux program could write through a read-only mount. It ran as
  root in its user namespace with that namespace's capabilities, and
  `mount -o remount,rw,bind` on a read-only grant, or a read-only mount of an
  isolated root, succeeded. Where the caller could write the underlying path,
  so then could the program. Every capability is now dropped before the
  program starts, the bounding set included.

### Fixed
- A keystroke on WHPX or HVF could be followed by an interrupt for an empty
  keyboard buffer (a stray `0x00` read from port 0x60, about one
  `key_delivery` run in fifteen). The i8042 raised IRQ 1 on the userspace PIC
  and also through the interrupt sink, whose queued pulse reached the same PIC
  later, sometimes after the guest had read the byte. It now raises on the PIC
  alone when it has one. 0 failures in 200 runs afterwards; KVM, which has no
  userspace PIC, is unchanged and was not run.
- PS/2 mouse movement packets now set byte 0 bit 3, which a driver uses to
  resynchronise; they never did.
- A guest on WHPX or HVF had no interval timer. The legacy machine attached
  COM1, the RTC and the keyboard controller but no PIT, so a kernel's writes to
  ports 0x40-0x43 hit an unhandled port and its IRQ 0 never came; one that did
  STI and HLT to wait for a tick waited forever. A PIT is now attached off KVM
  (KVM keeps its in-kernel one). The PIT also ignored the divisor a guest
  programmed and ticked at a fixed 18.2 Hz; it now follows it (divisor 11932 is
  about 100 Hz), re-timing at once when the counter is written. A pending
  vector is injected only when the guest's interrupt flag is set and there is no
  interrupt shadow, and WHPX asks Windows for 1 ms timer resolution so a 10 ms
  tick is not rounded to 15.6 ms. A 32-bit guest that programs the 8259 and PIT
  and halts counts 20 ticks in about 204 ms on WHPX and 201 ms on KVM, and the
  test fails without the change. Mode 3 (square wave) still fires at half its
  rate; mode 2, which Linux and Botnix use, is exact.
- A Multiboot guest was told it had 127 MiB above 1 MiB whatever the VM's
  memory size, so a guest that trusted `mem_upper` or the memory map would use
  memory that does not exist. The map and `mem_upper` now come from the VM's
  size. Found by running a custom kernel (64 MiB VM, 130048 KiB reported).
- On Windows (WHPX), setting a partition's processor count failed with
  `0x80370302` (`WHV_E_UNKNOWN_PROPERTY`): the property code was 0x1000, and
  the SDK's is 0x1fff. 0x1001, named `ProcessorRoot`, is `ProcessorFeatures`.
  Found by running a guest on this host; a test now pins these codes to the SDK
  header. Nothing past this call has been run on WHPX.
- More WHPX definitions differed from the Windows SDK, found by comparing every
  constant in `whpx_ffi.rs` with `WinHvPlatformDefs.h`: the MSR register names
  (EFER, STAR, LSTAR and the rest are 0x2001..0x200b, not 0x29..0x33), the
  pending-interruption, interrupt-state and deliverability registers (0x8000_0000
  and up, and one 64-bit word of bits each, not structs of 4-byte flags), and
  `WHV_UINT128` (16-aligned). The register value union was 24 bytes wide, not 16,
  so a register array had the wrong stride. Tests pin all of it to the SDK.
- WHPX crashed the Windows CI test jobs with `STATUS_STACK_BUFFER_OVERRUN`:
  `WHV_RUN_VP_EXIT_CONTEXT` was 184 bytes where the SDK's is 224, so
  `WHvRunVirtualProcessor` wrote past the caller's stack buffer. It and four
  other structs (`WHV_VP_EXIT_CONTEXT`, the memory-access and I/O-port
  contexts, `WHV_X64_VP_EXECUTION_STATE`) now have the SDK's sizes, checked by a
  test. The crash only showed once the partition property above was fixed,
  because until then no test got as far as running a guest.
- WHPX guests could not make progress: a port `IN` or `OUT` exit left RIP on the
  instruction, so the guest repeated it for ever. `run` now steps over it
  (string and REP forms are still not emulated). Boot segments had no D or G
  bit, which ran a 32-bit entry as 16-bit code; and the long-mode check read the
  wrong attribute bit. The Multiboot and Linux boot paths no longer reset the
  segments they had just set.
- Dropping a WHPX backend without `shutdown` deleted the partition before its
  vCPUs, so each vCPU called `WHvDeleteVirtualProcessor` on a dead handle and
  the process crashed in about half of the single-threaded test runs (5 in 8).
  A VM's memory is now freed after its partition, and mapping it is retried for
  up to half a second when the hypervisor still holds a recycled block, which
  failed in 5 of 20 parallel runs. After both, on the final tree: 8 of 8
  serial and 20 of 20 parallel runs of the 60 WHPX unit tests pass on this host. Several of those
  tests asserted nothing before, and three were wrong (they expected 64-bit
  mode with a 32-bit CS, or counted injections that never happened) and are
  corrected; two ran a guest that never exits and now use `kick`.
- Completing a port `IN` on WHPX rewrote every segment register as a real-mode
  segment, so a protected- or long-mode guest lost CS.L, D and G and the next
  instruction ran as 16-bit code (a triple fault). Found by running a custom
  kernel's boot; a test that runs a 32-bit guest through `IN`, `OUT` and `HLT`
  failed with the old code (CS attributes 0x409B became 0x9B) and passes now.
  `IN AL` and `IN AX` also zeroed the rest of RAX; they now merge as hardware
  does.
- A command still running when its sandbox paused stayed `running` in the
  history for good, though the docs said it was recorded as ended. Pausing
  now records it as `lost` ("the sandbox paused while it ran"), with what an
  envd command had printed, and a later completion does not rewrite it.
  Checked on KVM with an envd and an exec command.
- `config.example.toml` no longer describes an `[observability]` section that
  nothing reads, and the disk module's header now says what a sandbox with a
  disk can do (pause, fork, reboot) and cannot (checkpoint, start from a
  snapshot), as the code does.
- A machine run that printed faster than it was polled lost output without
  saying so. The guest agent holds at most 1 MiB of a stream between polls
  and dropped the oldest beyond that uncounted, so a run of 9 MiB reported
  1 MiB written and nothing dropped. It also turned a character split across
  two polls into U+FFFD. The agent now reports what it dropped
  (`stdout_dropped` and `stderr_dropped` on a poll, 0 from an older agent),
  keeps a half-written character for the next poll, and cuts between
  characters. The node counts the drops and polls again at once while a
  program floods.
- A machine with Claude Code installed at first boot could lose it for a
  while. The first time `claude` ran, its updater reinstalled its own npm
  package, and a prompt or conversation started during that window failed
  with "claude: not found". The updater is now off in the machine's Claude
  Code settings and login environment, and in every Claude Code turn the
  node starts.
- A guest's hardware clock gave the wrong date, for two reasons. The
  emulated RTC had its data-mode bit the wrong way round, so a guest that
  left it clear, as every PC does, was handed binary numbers and read them as
  BCD: Linux took 2026 for 2020. And it worked the date out as days over 365
  with thirty-day months, which by October 2026 was seventeen days ahead. A
  guest running this project's agent has its clock set by the agent and
  showed neither; a guest booted from a disk image has only the RTC, and
  Ubuntu refused its package archive as signed in the future. The registers
  are now BCD unless the guest asks for binary, and the date is the
  Gregorian calendar's.
- `POST /machines` refuses a processor count it cannot give. More than 32
  was stored and failed at the machine's first boot, and a machine from a
  disk image was given one processor whatever it asked for, without being
  told. `hm sandbox vm machine create --cpus` takes 1 to 32.
- Two messages with a run of spaces in the middle of a sentence, left by a
  lost line continuation: the reason the machine model gives for its PCI
  configuration device, and a block device's attach log line.
- `reboot` hung MMIO guests. On the hardware-reduced ACPI platform, Linux
  reboots through EFI and, with no EFI, spins in a BIOS jump. Guests now boot
  with `reboot=k`, and the VM stops on the i8042 reset pulse. Sandbox guests
  also boot with `panic=1`.

### Added
- `hm sandbox vm run IMAGE [-e N=V] [--keep] [COMMAND...]` runs a container
  image in a VM of its own, like `docker run` with no daemon: the node pulls
  the image, builds a template once (named from the reference), and each run
  is a fresh sandbox that is deleted afterwards. The image's `ENTRYPOINT`,
  `CMD`, `ENV`, `WORKDIR` and `USER` are honoured, and the exit code is the
  command's. The node now reads `Entrypoint`, `Cmd`, `WorkingDir` and `User`
  from an image's config (it read only `Env`), and `POST /sandboxes/{id}/exec`
  and the guest agent's exec take an optional `user`. Checked on KVM against
  `alpine`, `curlimages/curl` and `golang`. See [RUN_IMAGES.md](docs/RUN_IMAGES.md).
- A `Tools image` workflow builds the tools-image template on a GitHub runner,
  checks what is in it, and keeps it with a SHA-256 as an artifact; a
  `tools-image-v*` tag attaches it to a release. It does not boot the image,
  but the one it built passed the KVM checks when downloaded and run.
  See [TOOLS_IMAGE.md](docs/TOOLS_IMAGE.md#building-it-in-ci).
- A node can clone a private Git remote for an environment. `--git-credential
  PREFIX=ENV` names an environment variable holding `user:token`, sent as
  Basic only to remotes under `PREFIX`. It goes to `git` in the process's
  environment, not its arguments or the clone's config, and never into the
  guest or the store; a failed clone's message has it removed. See
  [ENVIRONMENTS.md](docs/ENVIRONMENTS.md#private-git-remotes).
- A FIPS build (`--features …/fips`): every TLS endpoint and HTTPS client in
  the control plane, node daemon and `hm` runs on AWS-LC's FIPS 140-3 validated
  module, chosen in one place (the new `hv2-tls` crate). `--fips` / `HV2_FIPS=1`
  refuses to start a build that is not it. See `docs/FIPS.md` for what is and is
  not covered.
- Machines from a disk image: `POST /machines` with `image` copies a raw image
  from the node's `--image-dir` and boots it by `--firmware`, so a stock cloud
  image runs with its own bootloader and kernel. Such a machine has no guest
  agent; `GET /machines/{name}/console?tail=N` reads its serial console and
  `POST` types at it. `hm sandbox vm machine` gains `create --image`,
  `console --tail` and `type`.
- Firmware boot in the VMM, on KVM: `BootSource::Pvh` enters a firmware image by
  the PVH boot protocol, and `VM::attach_block_pci` puts a disk on the PCI bus
  where firmware and a stock kernel look for one. An unmodified CirrOS cloud
  image boots to its login prompt. Not yet available to machines. See
  `docs/FIRMWARE_BOOT.md`.
- `hm sandbox exec`: one versioned JSON request in, one JSON response out, for a
  caller that is a program (`hv2_sandbox::request`). The schema is
  `docs/schemas/sandbox-request-v1.schema.json`.
- Containment on macOS: a sandboxed program asked for no network, for
  confinement to its granted paths, or for denied paths runs under a sandbox
  profile applied by `sandbox-exec`. The backend reported resource limits
  only before. See `docs/SANDBOXES.md` for what that interface is.
- UI isolation for sandboxed programs on Windows: `SandboxSpec::isolate_ui`,
  `--isolate-ui` and `isolateUi` in a request keep a workload from the
  clipboard, other programs' windows, system and display settings, and logging
  off, through the job object's user-interface restrictions. A new control,
  `UI isolation`; Linux and macOS refuse it.
- A machine booted from a disk image has ACPI tables: the processors it was
  created with (`cpuCount`, to 32, where it had one whatever was asked),
  interrupt routing for its PCI devices through the I/O APIC, and power
  registers. `poweroff` in such a guest now stops the machine, and it is left
  stopped whatever its restart policy. `VM::powered_off` says a guest turned
  itself off, as distinct from restarting. Ubuntu 24.04's cloud image runs as
  such a machine: its own 6.8 kernel on four processors, systemd started in
  full.
- Telemetry a team sets for itself: `PUT`, `GET` and `DELETE /telemetry`
  give a team a collector, and the control plane pushes that team's own
  numbers to it over OTLP/HTTP (sandboxes running and paused, their vCPUs and
  memory, and the last 24 hours of usage). The operator lists the collectors
  teams may use with the control plane's `--team-otlp-endpoint`; with none, no
  team telemetry is sent. A destination is checked when stored and at every
  push, follows no redirect, and takes no credentials in its URL. New API key
  scope `telemetry`. See `docs/TELEMETRY.md`.
- A sandbox's command history masks secrets before it keeps a command line:
  `NAME=VALUE` for secret-looking names, `--password`/`--token` values,
  `Bearer`/`Basic` credentials and credentials in URLs. `"commandHistory":
  false` on a create, inherited by forks, or `--no-command-history` on a
  node, keeps none. Output is masked separately, when read.
- A reverse port on a guest whose agent is too old to listen now says so:
  the 502 tells the caller to rebuild the guest image with a current agent,
  instead of reporting only that the connection closed. The sandbox is
  unaffected.
- A sandbox's command history masks secrets in what a command printed, when
  it is read: PEM private key blocks, `NAME=VALUE` / `name: value` /
  `"name": "value"` for secret-looking names (`env`, `.env`, JSON, YAML),
  the credential after `Bearer`/`Basic` in a header, URL credentials, and
  well-known tokens by shape (AWS, GitHub, GitLab, Slack, Stripe, Google, npm,
  `sk-`, JWT). The stream is kept whole and its offsets are unchanged. It
  over-masks, and cannot know every secret. See `docs/COMMAND_HISTORY.md`.
- V8's Web Tooling Benchmark ran in a 4 vCPU, 8 GiB sandbox from the tools
  image on KVM (`tools/bench-node-tooling-kvm.py`): median 12.56 runs/s over 12
  runs, 9.33 to 15.26, on a host at 42 to 100% CPU. A rough placement against
  a public leaderboard, not a comparison; see
  `docs/benchmarks/2026-10-16/node-web-tooling-kvm/README.md`.
- A stopped machine now closes the reverse connections waiting on it and
  forgets them. They used to stay open and counted against its 64 until the
  node restarted, so a machine stopped and started with 64 waiting refused the
  next (429). Checked on KVM: 0 of 64 closed before, 64 of 64 after, three runs.
- The control plane forwards a machine's reverse port,
  `GET /machines/{name}/ports/{hostPort}/reverse`, which it answered with 404
  before: the machine is found as for its other routes (another team's is a
  404), the node is sent the machine's team and the cluster token and not the
  caller's team header, and the node's stream is carried through. Checked on
  KVM through a control plane, three runs in a row, and by a unit test against
  a fake node.
- The guest `init` raises the in-memory root's size cap from the kernel's
  default (about 44% of memory here) to 90% after boot. A guest image large
  enough to fill the default, such as the one below, could write nowhere but
  `/tmp` ("No space left on device"); small images are unaffected, and the
  sized-templates check still passes with the new `init`.
- A sandbox template with the development tools in it:
  `tools/guest-image/build-tools-image.sh` builds Ubuntu 24.04 with git,
  Python 3, Node.js, npm, gcc, make, curl and jq from the distribution's
  archive (debootstrap and apt in a chroot, no Docker), and it ran on KVM:
  a C build, a Python program, `node --test` and a git commit offline, and
  with the network `curl`, `git clone`, `npm install` and `apt install`; a
  fork kept the files. It needs 1536 MiB of guest memory: at 1024 MiB `npm`
  was missing from the unpacked image. Not shipped; see `docs/TOOLS_IMAGE.md`.
- The Python and TypeScript SDKs are packaged for PyPI and npm and tested as
  installed (`tools/check-sdk-packages.sh`): the licence texts are in each
  package, the Python licence is a PEP 639 expression (needing setuptools 77),
  and the npm package is public. `.github/workflows/sdk-packages.yml` builds and
  tests them on every change and, for an `sdk-vX.Y.Z` tag approved in the
  `sdk-release` environment, publishes to PyPI by trusted publishing and to npm
  with provenance. Nothing is published yet; see `docs/RELEASING_SDKS.md`.
- `--sized-template-disk-gb N` (default 16) bounds the disk a node's sized
  templates take together. Each is a snapshot as large as its guest's
  memory, and four large sizes could fill a node's temporary directory; a
  size that would go past the bound is not built, and its sandboxes boot.
- Usage kept for good: as each lifecycle event is published, the cluster
  store folds it into a usage ledger of running stretches, kept in Redis with
  no limit (a lone node: in memory, the last 100,000). `GET /usage` reads the
  ledger, and the event tail only for before the ledger began, so usage no
  longer stops at the last 10,000 events. See `docs/USAGE.md`.
- An environment's Git repositories: a repository with `"kind": "git"` is
  cloned by the node from an `http` or `https` Git remote the operator allows
  with `--git-remote`, with `git` on its `PATH`. The clone takes only the web
  protocols, follows no redirects, runs no hooks or submodules and reads no
  system or global Git configuration; it is packed and placed in the guest as
  a Lit clone is. A Git repository takes no credentials. See
  `docs/ENVIRONMENTS.md`.
- A port of the caller's reached from inside a machine or a sandbox, as
  `ssh -R` does: `hm sandbox vm reverse ID --port P --to HOST:PORT`, and
  `hm sandbox vm machine reverse NAME ...` for a machine, over
  `GET /sandboxes/{id}/ports/{port}/reverse` (or `/machines/{name}/ports/{port}/reverse`)
  upgraded to `hv2-reverse/1`.
  A program in the guest connects to its `127.0.0.1:P`, with no network
  card needed. The guest agent has a new `Listen` operation for it, so a
  guest needs this release's agent. See `docs/REVERSE_PORTS.md`.
- A sandbox's command history: every command run through `exec` or envd's
  `process.Process/Start` (E2B's `commands.run`) is kept with its command
  line, user, PID, how it ended and the newest 64 KiB of each stream, at
  `GET /sandboxes/{id}/commands` and `.../commands/{n}`
  (`hm sandbox vm history`). The newest 64 are kept, through a pause and,
  with a snapshot store, onto the node that resumes it. `hv2-api`'s
  `EnvdProcess` takes a `ProcessObserver` for this. See
  `docs/COMMAND_HISTORY.md`.
- A guest's processes listed from outside: `GET /sandboxes/{id}/processes`
  and `GET /machines/{name}/processes` (`?all=true` for kernel threads),
  `hm sandbox vm ps` and `hm sandbox vm machine ps`, and `processes()` in
  both SDKs. Every process the guest has, with its parent, user, state,
  command line, threads, memory, CPU time and age, read from its `/proc`
  through the guest agent. See `docs/PROCESSES.md`.
- A machine's runs, and their output, are kept across a node restart, in
  the machine's directory under `runs/`. A run that was going when the node
  stopped comes back as `lost`, saying so, with its output to the node's
  last poll. The output files are cut back to what is kept, 4 MiB a
  stream, once they are twice that.
- A sandbox's checkpoint read from outside, without touching the sandbox:
  `GET /sandboxes/{id}/checkpoints/{name}/files?path=` lists a directory in
  it and `.../file?path=` reads a file, up to 64 MiB
  (`hm sandbox vm checkpoint ls|cat`). Each request restores the checkpoint
  into a guest of its own, with no network or volumes, stops the
  checkpoint's programs, reads, and stops the guest. See
  `docs/CHECKPOINTS.md`.
- A sandbox's size changed after it was created:
  - **Resize:** `POST /sandboxes/{id}/resize` with `cpuCount` and
    `memoryMB` (`hm sandbox vm resize`). It is a reboot at the new size:
    the sandbox keeps its ID, token, network, volumes, disk and variables,
    and loses what was in its memory.
  - **Resume:** `resume` takes the same fields, to bring a paused sandbox
    back at another size, booted rather than restored
    (`hm sandbox vm resume --cpus --memory-mb`).
  - **Fork:** `fork` takes the same fields (`hm sandbox vm fork --cpus
    --memory-mb`). Forks of another size boot at it, with a copy of the
    source's disk and its volumes, network and variables, but not its
    memory. The source is paused only to copy a disk.
  - **Afterwards:** usage counts each size for the time spent at it, and a
    resize is reported as `sandbox-resized`.
  - **A fast start at any size:** the node builds a template again at each
    size a sandbox asks for, and restores later sandboxes at that size from
    it, whether created, resized, forked or resumed at it. Only the first
    boots. `--template-size CPUSxMB` builds one at start, and
    `--sized-templates N` (default 4) bounds how many are kept. See
    `docs/SANDBOX_SIZES.md`.
- A desktop for a machine, and computer use on it: `desktop` on
  `POST /machines`, for a machine from an image with `guestAgent`.
  - **What is installed:** at first boot, a virtual display with a window
    manager and a terminal, and noVNC on guest port 6080 to watch it in a
    browser.
  - **Through the API:** `/machines/{name}/desktop`, `.../screenshot` (PNG)
    and `.../actions`, which moves, clicks, drags, types, presses keys or
    scrolls. `hm sandbox vm machine desktop`, and `desktop` in both SDKs.
  - **For the agents:** each coding agent installed is given the same
    actions as an MCP server named `computer`.
- Conversations with a machine's coding agents:
  `/machines/{name}/conversations`, `hm sandbox vm machine conversation`,
  and `conversations` in both SDKs.
  - **Turns:** each message is a turn, which is a prompt, and the agent's
    session is carried from one turn to the next.
  - **Steering:** a message to Claude Code while its turn runs goes into
    that turn. The turn reads messages as stream-json on its standard input,
    and ends once each message has been answered. Codex takes one turn at a
    time, and a message while it runs is refused.
  - **Transcript:** what was said, in order, read from the agent's events.
  - **Moving to another agent:** `from` starts a new conversation, with
    either agent, from what was said in another.
- Prompts: `resume` works for `codex`, as `codex exec resume`. It was
  refused.
- Coding agents installed in a machine: `agents: ["claude", "codex"]` on
  `POST /machines` with `image` and `guestAgent` (`hm sandbox vm machine
  create --agent claude --agent codex`, `agents` in both SDKs). The node adds
  their install to the cloud-init vendor data that installs its guest agent:
  Node and npm from the distribution's packages if the image has no `npm`,
  then Claude Code and Codex from npm, once, at first boot. A failure is
  logged to `/var/log/hv2-agents.log` and leaves the machine reachable;
  `/var/lib/hv2/agents-ready` says when they are there. An agent not
  offered, or agents without the guest agent, are refused.
- A sandbox's own size: `cpuCount` and `memoryMB` on a create (`hm sandbox vm
  create --cpus --memory-mb`). One its template is not is booted at that
  size rather than restored, kept through pause and resume, inherited by a
  fork, and counted in usage at that size. Such a sandbox is written whole
  when paused or forked, and refused, as one with a disk is, for
  checkpoints and named snapshots. Refused, with nothing made, for a size
  the host cannot give.
- A login in front of a machine's port: the control plane's proxy routes
  `{port}-{machineID}` to the node that has the machine, asking nodes only
  for an ID of a machine's shape, so a port published with `proxy` is
  reached through it behind the web access login, granted by `machineID`.
- Lit repositories in environments: an environment's `repositories`
  (`remote`, `path`, `branch`, `username`, `password`) are cloned by the
  node with the `lit` binary `--lit` names, only from remotes allowed with
  `--lit-remote`, and unpacked into the guest, `.lit/` and all, owned by the
  template's user, before the create answers. Passwords are never returned.
  See docs/ENVIRONMENTS.md.
- A TypeScript client, `sdk/typescript` (`@nervosys/hypermachine`), with no
  dependencies and types in `index.d.ts`: what the Python client has, in
  camelCase. Its tests and a strict `tsc` of its declarations run in CI;
  checked against a node on KVM.
- A Python client, `sdk/python` (`hypermachine`), with no dependencies:
  machines, runs, prompts, files, ports, snapshots, usage and environments,
  refusals raised as `ApiError`. Tested against an HTTP fixture on every CI
  run, and against a node on KVM.
- A machine snapshot's files, read without booting it:
  `GET /machines/{name}/snapshots/{id}/files?path=` lists a directory and
  `.../file?path=` reads one file, from an ext filesystem on the whole disk
  or in a partition (`partition=N`, else the largest ext one). Paths are
  walked by inode, so no name is ever part of a `debugfs` request.
  `hm sandbox vm machine snapshot ls|cat`.
- A machine's published port can be routed through the node's sandbox
  proxy too: `"proxy": true` on `POST /machines/{name}/ports` (`hm ...
  publish --proxy`) makes it reachable at `{guestPort}-{machineID}.{domain}`
  and by E2B's routing headers, over the proxy's TLS. Off unless asked.
- Environments: `PUT /environments/{name}` keeps a team's named set of
  variables and files; a sandbox created with `"environment": NAME` starts
  with them, its own `envVars` over the environment's and the files written
  into its guest before the create answers. They are put back after a
  reboot and carried into a fork. Listing returns names and sizes only.
  Served by a node and a control plane from the cluster store. See
  docs/ENVIRONMENTS.md.
- Snapshots of a machine's disk: `POST /machines/{name}/snapshots` copies a
  stopped machine's disk (sparse, and sharing blocks where the filesystem
  can); `.../snapshots/{id}/restore` puts it back with its disk size,
  replacing the disk in one rename; `.../snapshots/{id}/disk` downloads it
  raw; list, get and delete. `hm sandbox vm machine snapshot take|list|
  restore|download|delete`; the control plane forwards them.
- Usage: `GET /usage?from=&to=&by=sandbox|team|template|owner` adds up how
  long the caller's sandboxes ran, with vCPU-seconds and memory
  GiB-seconds, from the lifecycle events: time paused is not counted, a
  sandbox still running counts to the moment asked, and the answer says how
  far back the events go. Each start and stop event carries a ledger entry
  (`hm_usage`) for it, kept out of what E2B clients and webhooks receive.
  Served by a node and by the control plane; `hm sandbox vm usage`. See
  docs/USAGE.md.
- OTLP export from a node: `hv2-sandboxd --otlp-endpoint URL` (or
  OpenTelemetry's `OTEL_EXPORTER_OTLP_*` variables) pushes the metrics it
  serves at `/metrics` to an OpenTelemetry collector over OTLP/HTTP with
  JSON, every `--otlp-interval` seconds, with `--otlp-header` for a token.
  Pushes are counted in `hv2_node_otlp_pushes_total`. Any other OTLP
  protocol is refused at start. See docs/TELEMETRY.md.
- Published ports for machines: `POST /machines/{name}/ports` gives a port
  of the guest one of the node's, and connections to it are carried in
  through the guest agent, with no NIC needed. The node gives out ports from
  `--machine-ports` (20000-29999) on `--machine-port-address` (loopback
  unless said). Kept with the machine across stops and node restarts;
  `GET` lists them and `DELETE .../ports/{hostPort}` takes one back.
  `hm sandbox vm machine publish`, `ports` and `unpublish`.
- Resizing a machine: `PATCH /machines/{name}` with any of `cpuCount`,
  `memoryMB` and `diskGiB` changes a stopped machine's size, which it has at
  its next start. A disk only grows; a template machine's filesystem is
  checked and grown with it, and an image's partitions are left to the
  guest. `hm sandbox vm machine resize`, and the control plane forwards it.
- Files for machines: `PUT /machines/{name}/files?path=` writes the request's
  body to a path in the guest, making the directories above it, for a guest
  `user` when one is given; `GET` answers with a file's bytes. Up to 64 MiB
  each way. The control plane forwards both, and `hm sandbox vm machine
  upload` / `download` use them.
- Prompts for machines: `POST /machines/{name}/prompts` runs a coding agent
  that is in the machine (`claude`, `codex`, or any by its own `command`) on
  a prompt, as a run. Its events are read from any offset
  (`GET .../prompts/{id}?after=N`), it is interrupted
  (`.../interrupt`), and `GET .../prompts` lists them. The prompt reaches the
  agent in its environment and is never part of a shell line; `env`, where
  its key goes, is not kept. `hm sandbox vm machine prompt` follows one to
  its end, and `machine run` / `machine runs` start and read runs.
- The guest agent starts programs with SIGINT, SIGQUIT, SIGTERM and SIGHUP
  at their defaults. The sandbox guest's init starts the agent with `&`,
  which leaves SIGINT and SIGQUIT ignored in it and in everything it
  starts: an interrupt sent to a program was accepted and did nothing, and
  a shell could not trap it. A guest image has the fix once it is rebuilt
  with this agent.
- Runs for machines: `POST /machines/{name}/runs` starts a command and
  answers at once; its output is kept on the node and read from any offset
  (`GET .../runs/{id}?stdout=N&stderr=N`), it can be written to
  (`.../stdin`) and signalled (`.../signal`), and what it printed is still
  there after it ends. `GET .../runs` lists them. The control plane forwards
  all five. A run's environment is not kept.
- A guest agent in a machine from a disk image: `guestAgent` on
  `POST /machines` (`hm sandbox vm machine create --guest-agent`), on a node
  started with `--guest-agent FILE`, installs the agent through the image's
  cloud-init and gives the machine a vsock channel, so `exec` works in a
  distribution's image as it does in a template's. Nothing in the image is
  changed beforehand.
- cloud-init for a machine from a disk image: `userData` on `POST /machines`
  and `hm sandbox vm machine create --user-data FILE` reach the image's
  cloud-init on a seed disk the node writes itself, with the machine's name
  as its hostname. A distribution's cloud image needs no editing to be
  logged in to. The data is not returned with the machine's record.
- Automatic checkpoints: a sandbox created with `autoCheckpoint` (seconds;
  `hm sandbox vm create --auto-checkpoint`), or any sandbox on a node run
  with `--auto-checkpoint-every`, is checkpointed on that interval without
  being asked, memory included. One is kept, named `auto`, each replacing
  the last only once it is whole, and it is rolled back to like any other.
  None is taken when nothing has happened in the sandbox since the last.
- A network between none and the host's for a sandboxed process:
  `NetworkPolicy::Proxy { port }` and `hm sandbox run --net proxy:PORT` give a
  program one port on the host's loopback, where the caller's proxy listens,
  and nothing else, so the proxy decides which hosts are reached. Linux relays
  the port out of the program's empty network namespace; macOS allows it in
  the sandbox profile. A new control, `network through a proxy`; Windows and
  the microVM backend refuse it.
- Allowed hosts for a sandboxed process: `hm sandbox run --allow-host HOST`
  and `network.allow` in a request list the only hosts a program may reach,
  by name, `*.` wildcard, address or range. `hm` runs a proxy for the run
  (`hv2_net::forward_proxy`) that asks the egress policy the microVM gateway
  uses, keeps the program to that proxy's port, and sets the proxy variables.
  Reserved addresses stay closed whatever is listed. Linux and macOS.
- Denied paths on Windows: a denied path stops inheriting the entries that let
  an AppContainer in for the length of the run, and inherits again afterwards.
  Path denial was Linux-only.
- A Node client for the process sandbox, in `sdk/node`: `run(request)` sends
  one request to `hm sandbox exec` and resolves with the result, or rejects
  with why the run did not happen. No dependencies; not on npm.
- Denied paths for sandboxed programs on Linux: `PathGrants::denied`, `--deny
  PATH` on `hm sandbox run` and `filesystem.denied` in a request close a path
  and everything under it, on the host's filesystem or under a grant. A new
  control, `path denial`, reports where it holds; Windows and macOS refuse it.
- "Only these paths" for sandboxed programs: `SandboxSpec::confine_paths`,
  `--confine-paths` on `hm sandbox run` and `filesystem.confine` in a request
  make the granted paths all of the caller's filesystem a workload reaches,
  with no root to build. Linux roots it in an empty directory holding the
  grants; Windows uses an AppContainer that keeps the network when asked. A new
  control, `path confinement`, reports where it holds. A read-write grant
  inside an isolated root is now a writable mount on Linux.
- Path grants for sandboxed programs: `SandboxSpec::grants`, and `--ro PATH` /
  `--rw PATH` on `hm sandbox run`, open a path to a workload whose containment
  would hide it. On Windows that is an access-control entry for the run's
  container, removed when it ends.
- The Windows process sandbox enforces "no network": a workload asked for none
  is started in an AppContainer with no capabilities, and cannot open a socket,
  loopback included. Such a workload also sees less of the filesystem, so
  `hm sandbox run`, which denies the network by default, is stricter on Windows
  than it was; `--net host` is the old behaviour. See `docs/SANDBOXES.md`.
- A machine made from a disk image can have a network: a NIC on the PCI bus
  behind the egress gateway, which now answers DHCP so a stock guest configures
  itself. The gateway no longer answers ARP for the guest's own address, which
  a DHCP client reads as a conflict.
- `--disk-slot`: template guests boot with a placeholder disk, so a sandbox that
  asks for a disk is restored from the template (or taken from the warm pool)
  and given the real one, where it used to cold-boot. Its pause and fork then
  write only what changed. See `docs/DISKS.md`.
- A sandbox holding a block disk can be forked: each fork gets a copy of the
  disk made while the source is paused, so it matches the memory the fork is
  restored from. Up to 8 forks a request. See `docs/DISKS.md`.
- A sandbox holding a block disk can be paused and resumed, by request, by
  `autoPause` or when idle. The disk stays claimed while it is paused, and only
  the node holding the disk resumes it. See `docs/DISKS.md`.
- A warm pool: `--warm-pool N` keeps N sandboxes of the base template restored
  and in standby, and a plain create takes one. `GET /pool` reports it. See
  `docs/WARM_POOL.md`.
- Standby for sandboxes: `POST /sandboxes/{id}/standby` (and `hm sandbox vm
  standby`) stops a sandbox's vCPUs and keeps its memory; the next command,
  file or port request resumes it without being asked. `--idle-standby-after`
  does it to idle sandboxes. See `docs/STANDBY.md`.
- `hm sandbox vm machine`: create, list, inspect, start, stop, restart, delete,
  exec (with the guest command's exit code), console and network decisions,
  against a node or the control plane.
- Machines through the control plane: it serves `/machines`, places a new
  machine on the node with the most room (or the one `nodeID` names) and finds
  an existing one by asking the nodes. A new `machines` API key scope gates
  them, and a machine belongs to its creating key's team, as a volume does.
- Machine networking: a machine created with `network` has one NIC behind the
  node's egress gateway, with a sandbox's `allowOut`/`denyOut` rules, kept with
  the machine and applied at every boot. `GET /machines/{name}/network/decisions`
  shows what was allowed and refused. Egress only; nothing can connect in.
- Machines: long-lived VMs that boot from their own persistent ext4 root disk
  (made from any template), with no lifetime cap. They keep what they write
  across stop and start, guest reboots and daemon restarts (`autostart`), and
  restart by policy when the guest stops by itself. `/machines` on the node,
  `--machine-dir`. See `docs/MACHINES.md`.
- Single sign-on through any OpenID Connect provider (`--sso-*`). Members
  listed by verified email sign in at `/auth/login` with code + PKCE. They get
  a `__Host-` session cookie, or send the token as a bearer token, and act
  with their scopes, role, principal and team. ID tokens are checked with
  RS256 or ES256 through IronCrypto. Cookie-authenticated changes must come
  from the control plane's own origin, and nobody is anonymous once SSO is on.
  `hm sandbox vm login` signs the CLI in through a loopback listener and keeps
  the session per endpoint. With `--sso-guest-urls`, private guest URLs need a
  signed-in member who may view the sandbox. The guest receives their verified
  email in `X-HyperMachine-User` and never the credential. See `docs/SSO.md`.
- Teams. An API key policy may name a `team_id`. Each sandbox is then recorded
  in its creator's team, and a key reaches every sandbox in its team by role,
  and none outside it, on every sandbox route, the inventory, metrics and
  per-sandbox events. Resources still shared by every team (image
  template builds and cluster events) are refused to team keys until they are
  partitioned. Administrators stay global. See `docs/TEAMS.md`.
- Snapshots per team. A snapshot belongs to its sandbox's team, which alone
  lists it, starts from it and deletes it. Operator templates stay usable by
  every team. Image builds remain administrator-only.
- Volumes per team. A volume belongs to its creator's team, its ID is derived
  from team and name, the volume API shows a team only its own, and
  `volumeMounts` resolves names within the sandbox's team.
- Events and webhooks per team. Each event carries its sandbox's team, and a
  team reads only its own. A team's webhook receives only its team's events,
  and another team's webhook is not found.
- Sandboxes reboot in place. A guest that runs `reboot`, panics or triple-faults
  comes back within about a second, with the same ID, token, URL, network policy,
  volumes, disk and `envVars`. There is also `POST /sandboxes/{id}/reboot`. A
  sandbox rebooted five times in 60 s is ended as lost. See `docs/REBOOT.md`.
- Movable block disks: a file-backed virtio-blk device (`VM::attach_block`), and
  node-level `POST/GET/DELETE /disks` with `"diskMount"` on sandbox create. Each
  disk is ext4-formatted and held by one sandbox at a time, then released when
  that sandbox ends. Also `hm vm disk …` and `hm vm create --disk NAME:/path`.
  Verified on real KVM, where data written in one sandbox was read in the next;
  see `docs/DISKS.md` for the limits.

### Benchmark evidence
- Sample failed-boot architectural state on the vCPU execution owner after a
  kick, with a bounded response wait. Add a real-KVM halted/spinning regression
  and a smaller cold-boot diagnostic probe; this does not fix startup stalls.
- Add a checksum-pinned Firecracker installer and alternating same-host cold
  sandbox comparison with exact guest workload verification, failure retention,
  cleanup checks and source provenance. Record two cohorts exposing HyperMachine
  startup failures and slower median readiness; no performance win is claimed.
- Gate local engine benchmark failure accounting in CI.
- Add opt-in KVM boot exit tracing on the vCPU owner thread and retain node
  log tails at each failed benchmark sample. Mark tracing reports as diagnostics
  because register sampling and log output perturb performance.

### Security
- Reject control-plane credentials configured as both a legacy admin key and
  a scoped key. Embedded instances enforce the scoped key's permissions and
  expiry, preventing legacy admin access from overriding those restrictions.
- Restored guests receive host time sampled after their agent connection is
  established, avoiding stale timestamps captured before worker scheduling
  and connection waits. Host clock conversion failures are reported rather
  than sending zero or truncating the timestamp; RNG reseeding still must
  succeed.
- **The API's `/agentic` auth exemption no longer covers writes**
  (`hv2-api`). With API keys on, the default exempted every path under
  `/agentic`, and `POST /agentic/plans/execute` starts and stops the
  server's real VMs, so anyone who could reach the port could drive them.
  Excluded paths now apply to GET, HEAD and OPTIONS only. Behaviour change:
  `POST /agentic/plans/validate` and `POST /agentic/templates` need a key.
- **A tenant's `allowOut` no longer reaches the host's or the cluster's
  network** (`hv2-net`, `hv2-sandboxd`, Helm chart). `allowOut` CIDRs were
  checked before reserved addresses, so a sandbox created with
  `allowOut: ["0.0.0.0/0"]` could reach private and link-local ranges. In a
  cluster that included the unauthenticated Valkey store holding every
  sandbox's envd access token, and the cloud metadata address. Reproduced on
  real KVM guests. Now:
  - a reserved range opens only where the operator grants it, with
    `--tenant-reserved-cidr` or `node.tenantReservedCidrs`;
  - the chart's store has a generated password;
  - node pods get an egress NetworkPolicy.

  Behaviour changes: `allowOut` rules that relied on reaching private ranges
  need an operator grant, and an `auth.existingSecret` needs a
  `store-password` key. `tools/e2e-egress.sh` checks all of it on real guests.

### Added
- `hm sandbox vm mcp` serves 12 real remote sandbox lifecycle, execution and
  checkpoint tools over MCP 2025-11-25 stdio. It validates arguments before
  HTTP requests, bounds input frames, omits structured sandbox access tokens,
  reports guest and partial-fork failures, and keeps tracing on stderr.
- Protected control-plane API access tracing with request correlation,
  credential categories, configured-key fingerprints, route templates and
  response status/timing. Logs omit raw credentials and request contents;
  durable audit storage and bearer/proxy-route coverage are not provided.
- Sandbox Helm chart policy-Secret mounting and optional policy-only API
  authentication, with configuration validation and rotation instructions.
  Disabling legacy admin authentication requires a scoped-key policy Secret.
- Control-plane `--api-keys-file` provisions hashed, expiring credentials with
  admin, inventory, sandbox, template, volume and event capability scopes.
  Inventory access excludes credential-bearing detail responses. Expiration
  is enforced per API request; policies load at startup and apply to the single
  configured team. Existing admin and independently issued bearer credentials
  keep their existing lifetimes.
- Cluster template listings report node-advertised snapshot availability and
  guest resources, with unknown metadata for legacy nodes and per-node details
  for heterogeneous fleets. Checkpoint operations now forward through the
  authenticated control plane. VM CLI files can route through shared proxies
  with `--envd-host`.
- `hm sandbox vm files` uploads and downloads binary files through authenticated
  envd, with a separate sandbox access token, a 512 MiB limit, atomic downloads
  and no overwrite of existing local files. Requires a reachable envd endpoint.
- `hv2-sandboxd --require-template` refuses startup when any configured snapshot
  template cannot be prepared, before listening or joining the cluster. It
  conflicts with `--no-template`; the default cold-boot fallback is unchanged.
- **`hm sandbox vm`** (`hm-cli`): a client for sandboxd lifecycle, guest
  commands and checkpoints. `benchmark` measures creation through checked
  guest output, with concurrency, raw samples, tail latencies, failure
  accounting and an optional P99 gate. Commands retain guest exit codes;
  benchmark failures and cleanup errors fail the run.
- **`hm jobs`, a durable job queue** (`hv2-jobs`, `hm-cli`): programs
  queued in a shared directory and run by workers under the process sandbox,
  with labels (e.g. `gpu`), leases that requeue a lost worker's job,
  graceful stops, `HM_JOB_ID`, kept logs and exit codes, and a REST mirror
  (`hm jobs serve`, `/api/v1/jobs`). No VM, daemon or database. Claims are
  exclusive file creations, because `rename` is not exclusive on Windows.
  See `docs/JOBS.md`; checked by `tools/e2e-jobs.sh` on Windows and Linux.
- **`hm sandbox run`** (`hm-cli`): run a host program under the process
  sandbox from the command line, with limits, network and filesystem policy,
  an enforcement report, live output and the program's exit code. No VM.
- **Streaming and cancellation in `hv2-sandbox`**: `Sandbox::run_with` takes
  a `RunIo` with an output sink, called as each chunk arrives, and a cancel
  flag that kills the whole workload. `run` is unchanged, and the trait's
  default `run_with` keeps other backends working.
- **Checkpoints** (`hv2-sandboxd`): save a running sandbox and roll it
  back in place -- memory and filesystem, under the same ID, token and URL
  (`/sandboxes/{id}/checkpoints`, `.../{name}/restore`, list, delete; ten
  per sandbox). A restore boots the checkpoint before stopping the current
  guest, so one that fails changes nothing. Checked on KVM guests by
  `tools/e2e-checkpoints.sh`.
- **Idle pause** (`hv2-sandboxd`): `idleTimeout` on create, or
  `--idle-pause-after` for a node, pauses a sandbox to disk once it is
  unused; with `autoResume` the next request wakes it. Unused means no proxy
  traffic, no `/exec`, and a guest CPU under 5% for the whole window, so
  unwatched work is never frozen. `/exec` now counts as activity, which
  also keeps node-full eviction from pausing a sandbox mid-command. Checked
  on KVM guests by `tools/e2e-idle-pause.sh`.
- **Sandbox `envVars`** (`hv2-sandboxd`), as E2B's `NewSandbox` has them:
  variables every command in the sandbox sees, over its template's `ENV`.
  Kept in the guest, so a pause, a resume on another node, a fork and a
  snapshot carry them; never returned by the API. Checked on real KVM
  guests by `tools/e2e-sandbox-env.sh`. `docs/PLATFORM_PARITY.md` tracks
  HyperMachine against boxd and exe.dev.
- **vTPM command dispatcher** (`hv2-core`). `VirtualTpm::execute` takes a
  TPM 2.0 command buffer and returns the response: Startup, Shutdown,
  SelfTest, GetRandom, GetCapability, PCR_Read and PCR_Extend, password
  sessions only, everything else `TPM_RC_COMMAND_CODE`. It is the interface a
  device model will forward to; there is no device model yet, so no guest
  reaches it. Along the way:
  - `GetRandom` was a counter from a fixed seed, the same bytes in every
    vTPM. It is IronCrypto's HMAC_DRBG, seeded from the OS.
  - `Startup(Clear)` now resets the PCRs, and a second `Startup` is refused,
    so a rebooted guest does not measure on top of its last boot.
  - `TpmResponseCode::BadPcr` and `NvDefined` had the wrong values (they
    decoded as `TPM_RC_BINDING` and `TPM_RC_NV_AUTHORIZATION`).
- **Tamper-evident audit logs** (`hv2-core`, `hv2-agent`, `hv2-api`).
  `hv2_core::security::AuditChain` writes one JSON object per line, each
  carrying an HMAC-SHA256 over its sequence number, timestamp, source, event
  and the previous record's MAC. Editing, deleting, reordering or splicing a
  record breaks the chain at that line. The MCP tool-call log and the HTTP
  audit middleware both append to it when configured (`audit_chain` /
  `chain`); `AuditChain::from_env()` opens one from `HV2_AUDIT_CHAIN` and
  `HV2_AUDIT_KEY_FILE`; and `cargo run -p hv2-core --example verify_audit_log`
  checks a file. Truncation at the tail needs a copy held elsewhere: point a
  SIEM collector at the file. Off by default.
- **`hv2 serve` writes the audit chain** (`hv2-cli`). Setting
  `HV2_AUDIT_CHAIN` and `HV2_AUDIT_KEY_FILE` opens it, turns HTTP audit
  logging on, and makes a chain that cannot be opened, or no longer
  verifies, stop the server from starting.
- **A command runs inside a guest through the published API** (`hv2-agent`).
  Not the boot probe, which drives the vsock device by hand because it was
  written alongside it, but `AgentVM::ping_guest` and `exec_in_guest` -- the
  surface an embedder actually has:

      ping          : agent 1.1.0
      exec status   : exit=Some(0) signal=None timed_out=false
      exec stdout   : Linux (none) 6.6.52 ... x86_64 GNU/Linux

  `examples/guest_exec_probe.rs` is that run, and exits non-zero when the guest
  does not answer, so it is a check rather than only a thing to read.
- **`LoadedBoot::append_cmdline` and `LoadedBoot::cmdline`** (`hv2-core`): add
  an argument to a kernel command line that was written before the device
  existed, and report what a guest will actually be booted with rather than
  what the caller typed.
- **`VsockDevice::connect_ephemeral`** (`hv2-core`): a connection from a host
  port nothing else is using.
- **A command crosses the guest channel and is answered** (`hv2-core`,
  `hv2-guest-agent`). The host publishes a framed request into the guest's
  receive queue, `hv2-guest-agentd` inside a Linux guest reads it, and its reply
  comes back over the transmit queue:

      vsock reply : 73 bytes -- {"id":1,"version":1,"result":
                    {"kind":"pong","agent_version":"1.1.0"}}

  This is the experiment the guest channel existed for. The two halves of the
  feature -- a device proven against hand-built virtqueues, and an agent proven
  against the host kernel's own `AF_VSOCK` -- have now been proven against each
  other.
- **The host and the guest agent complete a request and a reply over vsock**
  (`hv2-core`, `hv2-guest-agent`). With the operation numbers corrected, a run
  against a Linux guest shows the whole exchange: the host publishes a 45-byte
  framed request, the guest answers with a credit update -- which is a guest
  saying it consumed the data -- and then a 73-byte reply from
  `hv2-guest-agentd`. The two halves of this feature have now spoken.
- **`hv2-guest-agentd` says when it accepts and how much it reads.** Silence
  was ambiguous in the way that cost the most time here: a host that gets no
  answer could not tell an agent that never accepted from one that accepted and
  is waiting on a request that never arrived, and those are failures in
  different halves of the transport.
- **A VM with a kernel to boot now gets the devices it needs to reach
  userspace** (`hv2-core`). `VM::provision` attaches `Machine::legacy_pc()`
  when a boot source is configured. Until now the only caller of that model was
  the boot probe, registering by hand -- so a VM created through a host had no
  console, no CMOS and no keyboard controller. That is not a degraded guest but
  one that cannot reach userspace at all: its output goes nowhere and it spins
  on the first absent port it polls. No agent can run in a guest that never
  gets that far, whatever the guest channel does.

  `Machine::attach_absent` is what provisioning uses: it registers only what
  the manager does not already have, so a caller who installed their own
  machine model keeps it rather than being refused for having done the thing
  this is a default for. A device occupying the same ports under a different
  name is still a conflict, and still reported -- silently declining to map a
  port range is how a guest ends up talking to something other than what the
  caller thinks it is.
- **A vsock connection is established between host and guest** (`hv2-core`).
  The host opens a connection, the packet reaches the guest through the receive
  queue, Linux's vsock stack answers over the transmit queue, and the
  connection reports `Established`. Data moves in both directions across the
  virtqueues for the first time.
- **A guest channel can be asked for through the tool surface** (`hv2-agent`).
  `vm.create` takes an optional `guest_cid`; `LocalVmHost` attaches the channel
  before launching and merges `virtio_mmio.device=` into the guest's command
  line first, because virtio-mmio has no enumeration and the address has to be
  on the command line before `VM::new` freezes it. After attaching it checks
  the rendered argument against what the device reports and fails the start on
  a disagreement -- a guest that probes the wrong address finds nothing and
  says nothing, so a warning would be useless.
- **A host can publish to the guest and signal it** (`hv2-core`).
  `VM::notify_vsock` moves packets the host queued into the receive buffers a
  driver posted and then raises the used-queue interrupt -- two steps that have
  to happen together: publishing without signalling leaves data in a ring the
  guest has no reason to read, and signalling without publishing wakes it to
  find nothing. `VsockDevice::deliver_pending` exposes the first half, which
  previously ran only when the *guest* kicked a queue -- the wrong trigger for
  a host-initiated message, since the guest kicks when it posts buffers and
  then waits.
  `VirtioMmioTransport` also routes its interrupt through the VM's
  `InterruptSink` now. It raised only on the userspace `Pic8259`, which a guest
  with an in-kernel irqchip never reads, so every virtio interrupt this device
  raised went nowhere.
- **A Linux guest driver binds to the vsock device** (`hv2-core`). The two
  halves of this feature had never spoken: the device was proven against tests
  that lay out virtqueues by hand, the guest agent over the host kernel's own
  socket. A guest now enumerates it and completes the handshake:

      /sys/bus/virtio/devices/virtio0/device  ->  0x0013   (VIRTIO_ID_VSOCK)
      /sys/bus/virtio/devices/virtio0/status  ->  0x0000000f
      /sys/bus/virtio/devices/virtio0/driver  ->  vmw_vsock_virtio_transport

  `0x0f` is ACKNOWLEDGE | DRIVER | DRIVER_OK | FEATURES_OK -- full feature
  negotiation, by Linux's real driver, against this device. Reaching it needed
  a kernel built with `CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`, which the WSL
  kernel does not set, so `virtio_mmio.device=` enumerated nothing before.
- **`Machine::legacy_pc()`** (`hv2-core`): the legacy PC device set in one
  call, replacing three hand-registrations a caller had to get right in order.
  Each entry records what breaks without it, because each was found the same
  way -- a guest hung, and the port it was spinning on named the device.
- **A boot regression test** (`hv2-core`). It asserts the e820 bounds
  *computed from the VM's configured memory*, so a table describing memory the
  guest does not have fails, and it skips -- naming what is missing -- where
  there is no `/dev/kvm` or no kernel image, rather than passing vacuously on a
  host that cannot execute a guest.
- **Userspace output reaches the console** (`hv2-core`). A program running as
  PID 1 inside the guest writes to `/dev/console` and the bytes come back out
  of `VM::console_output`. That completes the path: kernel boots, initramfs
  unpacks, init runs, writes through the tty layer, the 8250 driver takes a
  transmit interrupt, and the device hands the byte to the host.
- **egui, eframe and egui_extras moved 0.31 → 0.36** (`hm-gui`), which is what
  dependabot PRs #65, #67 and #69 each needed and none could do alone -- the
  three move as one ecosystem and CI failed on every one of them separately.
- **Device interrupts have a path to the guest** (`hv2-core`).
  `HypervisorBackend::set_irq_line` asserts and releases a line through the
  interrupt controller, which for KVM means `KVM_IRQ_LINE` on a
  `KvmVm::irq_line` that had been written and never called. It is deliberately
  not `inject_interrupt`: that hands a vector straight to the vCPU, bypassing
  the controller's masking and priority, and is wrong whenever an in-kernel
  irqchip exists. `Device::pending_interrupt` lets a device report the line it
  is asserting, polled after every access because that is when the condition
  changes, and `SerialDevice` implements the 16550's two sources. A backend
  that cannot drive a line says so rather than accepting the call and dropping
  it.
- **A guest reaches userspace** (`hv2-core`). The kernel boots, unpacks an
  initramfs, and executes a statically linked binary as PID 1 inside the guest.
  The proof is the kernel's own: an init that returns 42 produces
  `Attempted to kill init! exitcode=0x00002a00`, which only happens if that
  binary ran. Before it, the full boot -- RCU, the scheduler, SLUB, ftrace, the
  8250 driver finding COM1 at `0x3f8 (irq = 4) is a 16550A`, every filesystem
  registered, `Freeing unused kernel image (initmem) memory`. Userspace
  *output* does not come back yet; see below.
- **A Linux kernel boots** (`hv2-core`). It decompresses itself, enters the
  kernel proper, reads the `e820` map this loader writes, sets up its zones and
  prints ~20 lines of kernel log through `SerialDevice` to `VM::console_output`
  -- the memory ranges it reports back (`0x1000-0x9efff` and
  `0x100000-0x7fffffff`) are exactly the two entries handed to it. It does not
  reach userspace: it currently stops polling the DMA controller, which is the
  next legacy device to bring up. `examples/linux_boot_probe.rs` runs it and
  says how far it got.
- **Single-step tracing** (`hv2-core`). `VM::single_step_trace` steps a guest
  one instruction at a time and reports where it went, which is the only way to
  see either of the two ways a guest goes quiet: a triple fault arrives as
  `VmExit::Shutdown` and KVM resets the vCPU on AMD before returning it, so the
  registers afterwards describe the reset vector and not the fault; and a guest
  spinning in a tight loop never exits at all. Addresses are recorded *before*
  each step for that reason, and only a bounded tail is kept, because a guest
  can execute millions of instructions before it fails and the interesting part
  is always the end. It found the setup-header truncation below in one run,
  after an afternoon of reasoning had not.
- **Context as an environment** (`hv2-context`, `hv2-agent`). An agent's history
  is normally kept by putting it back in the prompt, which forces the decision
  about what matters to be made at *write* time: when a tool returns 40 MB,
  something has to choose what to keep before anyone knows what the next
  question will be, and whatever it discards is gone. The new crate implements
  the alternative from Scroll (arXiv:2608.21690) -- keep the history outside the
  context, as something the agent queries -- built on one invariant: **eviction
  changes the view and never the record**. `EventLog` is append-only and offers
  no operation that edits or removes an event, not as discipline but as absent
  API; every event has a `Seq` that never changes and never repeats. Payloads
  over 8 KiB move to a `PayloadStore` behind a handle, and are still indexed and
  searchable by their whole content rather than by the preview the log keeps.
  `SearchIndex` ranks with BM25 and returns addresses and previews, never
  content. `WorkingView::evict` persists before it selects -- so nothing can
  leave before it is addressable -- then protects the active turn and the recent
  tail, folds unprotected tool payloads down to their addresses, and only then
  evicts, leaving a `Headline` in a tiered `EvictionIndex` that keeps recent
  history detailed, coarsens distant history, and carries the exact span at
  every tier. `ContextRuntime` is somewhere to compute over what was retrieved
  under `hv2-sandbox`, so the answer comes back instead of the data. Exposed as
  seven MCP tools -- `context.search`, `expand`, `record`, `exec`, `compact`,
  `view`, `status` -- dispatched against a `ContextHost` the way `sandbox.*`
  dispatches against a `SandboxHost`; with none installed they refuse, because a
  record that accepts every write and loses it is worse than none. `context.exec`
  needs `HostExec` as well as the new `ContextMemory` capability: being able to
  read the record is not a reason to run code on the machine holding it. The
  runtime is a confined process with a durable workspace and not a resident
  namespace -- files persist between calls, variables do not -- and
  `docs/CONTEXT_AS_ENVIRONMENT.md` says so alongside the rest of what is not
  built.
- **A guest executed** (`hv2-core`). The boot path had only ever been
  type-checked: `VM::provision`, `load_boot` and `launch` were described as
  creating a hypervisor VM, writing an image into guest physical memory and
  running it, and no kernel had ever been asked to do any of it. Two new
  examples do. `kvm_probe` reports the platform, `VM::new` and `provision`
  separately, because a host can pass one and fail the next -- Windows does,
  when Hyper-V owns VT-x. `boot_probe` runs the whole path against a 512-byte
  real-mode image and prints what came back: a KVM VM and its vCPUs exist, the
  image is in guest memory, the guest runs, and `Hello, World!` arrives through
  `SerialDevice` and `DeviceManager` into `VM::console_output` -- which
  independently confirms the console plumbing as well. The gate on this was an
  inherited assumption rather than hardware: WSL2 here has nested
  virtualisation, `kvm_amd` loaded and an accessible `/dev/kvm`.
- **A guest can be reached, and a command can be run inside it** (`hv2-core`,
  `hv2-guest-agent`, `hv2-agent`, `hv2-api`). `execute_script` was described in
  four places as running inside the guest and always evaluated a Rhai script on
  the host; the engine was never the problem, because nothing in the repo could
  reach a guest at all. Every virtio device kept its descriptor table in host
  `Vec`s a test filled in, and no virtio register file was ever mapped into
  guest physical address space. Three pieces close that: `GuestQueue` reads the
  descriptor, available and used rings out of guest memory, bounding every
  guest-written field (a chain that cycles is refused, an index past the table
  errors, indirect tables may not nest, a chain may not claim more than 64 MiB);
  `VirtioMmioTransport` is a virtio-mmio v2 register file implementing `Device`,
  so `register_mmio_region` puts it on the MMIO exit path `VM::run` already
  takes; and `VsockDevice` carries the connection state machine and credit
  accounting, both bounded. The new `hv2-guest-agent` crate holds the wire
  protocol and the in-guest binary that answers it, `GuestAgent` is the host
  client, and `AgentVM::exec_in_guest` is the operation an agent calls —
  surfaced as the `vm.exec` MCP tool and `POST /api/v1/vms/{id}/exec`. Four
  ways of having no guest to run in each fail as themselves rather than as a
  timeout, because an agent that receives a timeout when the real problem is a
  missing device retries forever. Gated on `Capability::GuestExec`, which
  existed and had never been consulted.
- **Confinement the operating system enforces** (`hv2-sandbox`, `hv2-agent`).
  Two things here were named like sandboxes and confined nothing: `hv2-agent`'s
  `Sandbox` says so in its own docs, and `hv2-core::container` was 3,866 lines
  of namespace, cgroup and seccomp types whose `start` fabricated a PID. A
  repo-wide search for `seccomp|setrlimit|unshare|prctl|CreateJobObject` found
  prose and struct fields, and not one confinement syscall. The new crate is
  built on one rule: silently dropping a control is worse than no sandbox,
  because a caller who asked for no network and got one believes the opposite
  of the truth. `Sandbox::controls()` reports what *this host* enforces, probed
  by attempting each control rather than assumed; a spec asking for something
  the backend lacks is refused, naming the control and why it is unavailable;
  and `best_effort()` is an explicit opt-in whose result reports what was
  dropped. Two backends behind one trait — `ProcessSandbox` (cgroup v2,
  namespaces, `RLIMIT_*` and `no_new_privs` on Linux; job objects on Windows;
  resource limits and an honest refusal elsewhere) and `MicroVmSandbox`, which
  gets the controls no host kernel gives an unprivileged process by having a
  different kernel. Exposed to agents as `sandbox.run` and
  `sandbox.capabilities`, dispatched against a `SandboxHost` the way `vm.*`
  dispatches against a `VmHost`; with no host installed the tools refuse,
  because the alternative to confinement is not running the program unconfined.
  See `docs/SANDBOXES.md` and `docs/GUEST_AGENT.md`.
- **Boot sources — a VM can now be told what to boot** (`hv2-core`): the new
  `BootSource` (`boot::source`) describes a Linux bzImage (with optional initrd
  and command line), a Multiboot kernel, or a raw image, and resolves to a
  validated `LoadedBoot` with every image read off disk. `VMConfig` gained a
  `boot` field, so a boot source is part of a VM's configuration and survives
  serialization.
- **`VM::provision` and `VM::launch`** (`hv2-core`): `provision()` creates the
  backend VM — the WHPX partition or KVM VM fd, which nothing previously did —
  loads the boot images into guest physical memory, and leaves vCPU 0 at the
  entry point. `launch()` provisions, starts, and runs the guest on a background
  task; `stop()` reaps that task and reports any error from it. Together these
  close the gap between "a VM exists" and "a guest is executing".
- **`HypervisorBackend::load_boot`** (`hv2-core`): backends load a boot source
  into their own guest memory and set up the architectural state the protocol
  needs. Implemented for WHPX (delegating to the existing `WhpxVcpu::boot_linux`
  / `boot_multiboot` sequences, previously unreachable from `VM`), for KVM
  (flat 32-bit protected mode with a boot GDT for Linux, real mode for raw
  images), and for TCG. The default reports `NotSupported`, so a backend that
  cannot boot a guest says so instead of starting a VM that will never execute.
- **`KvmVm::write_guest_memory`** (`hv2-core`, Linux): bounds-checked host-side
  writes into the KVM slot-0 guest allocation.
- **Boot flags on `hm t2 create`** (`hm-cli`): `--kernel`, `--initrd`,
  `--cmdline`, and `--image`. The boot source is validated at create time — a
  bad path fails immediately rather than registering a VM that cannot start —
  persisted in the VM registry, and used by `hm t2 start`, which now launches
  the guest instead of only flipping state. A VM created without a boot image
  says so, at create time and in the logs at start time.
- **`boot` on the REST and MCP VM-creation payloads** (`hv2-api`, `hm-cli`):
  `POST /api/v1/vms` accepts a boot source (rejecting a malformed one with 400
  rather than 500) and `POST /api/v1/vms/{id}/start` launches a VM that has one.
- **`VmHost`: the MCP `vm.*` tools now drive real VMs** (`hv2-agent`): a new
  `vm_host` module defines the `VmHost` trait and an in-process `LocalVmHost`
  backed by `AgentVM`. `McpServer::set_vm_host` installs one; without it the
  server keeps its previous session-state behaviour, so tool schemas and agent
  logic stay testable with no hypervisor present. The server enforces session
  ownership before dispatching, and a non-owner's probe is indistinguishable
  from a VM that does not exist.
- **Multiboot boot on the KVM backend** (`hv2-core`): previously
  `NotSupported`. Enters 32-bit protected mode with `EAX` holding the
  bootloader magic and `EBX` the `multiboot_info` address.
- **`load_boot` on the HVF backend** (`hv2-core`, macOS): previously the
  `NotSupported` default. HVF's `create_vm` now also allocates and maps guest
  RAM at GPA 0 — it never had any — and `HvfBackend::write_guest_memory` writes
  into it. Linux, Multiboot, and raw images are all supported, entered through
  VMCS guest-state fields.
- **`MultibootProtocol::prepare_guest_memory` and `MultibootLayout`**
  (`hv2-core`): one shared Multiboot memory layout that every backend writes,
  so their guest images cannot drift apart.
- **`PlanExecutor`, `SimulatingExecutor`, and `VmHostExecutor`** (`hv2-api`):
  ontology plans now execute for real. `HyperMachineOntology::execute_plan_with`
  dispatches each step through an executor and, on failure, runs a genuine
  compensating action — a rolled-back plan destroys the VM it created. A step
  whose compensation fails is reported as *not* rolled back, with the reason, so
  a leaked resource is visible rather than hidden.
- **`GpuHost`, `InMemoryGpuHost` (`hv2-agent`) and `AgentGpuHost`
  (`hv2-runtime`)**: the `gpu.*` agent tools now reach the real
  `GpuTopologyMap`. Attaching a GPU removes it from the placement pool
  fleet-wide, so the scheduler stops offering a device an agent already took —
  something per-session bookkeeping could not model. Same trait-in-the-caller
  inversion as `VmHost`, which is what avoids the `hv2-runtime` → `hv2-agent`
  dependency cycle.
- **`GpuTopologyMap::devices` / `device` / `contains_device`** (`hv2-runtime`):
  enumerate the full inventory, including allocated devices.
- **Image admission control on the provisioning path** (`hv2-core`):
  `VM::set_image_registry` installs an `ImageRegistry` that `VM::provision`
  consults before loading a boot image, so denying or revoking an image now
  stops a VM booting it rather than only being reportable through
  `POST /api/v1/images/check-admission`. Admission is by SHA-256 of the bytes
  about to be loaded — `LoadedBoot::primary_image_digest`, matched by the new
  `ImageRegistry::check_admission_by_digest` — so renaming or moving a kernel
  cannot change the answer. Initrds and Multiboot modules are excluded from the
  digest; they are separate artifacts with their own registry entries. Opt-in:
  with no registry installed, any readable image boots exactly as before. A
  digest that cannot be computed (the `ring` feature is off) is a denial, never
  a pass.
- **`enforce_image_admission` on the API server** (`hv2-api`): the
  `/api/v1/images` routes and the VMs the API creates now share **one**
  `ImageRegistry`, so approving, denying, or revoking an image there decides
  whether a VM can boot it. `AppState::with_image_registry` installs it, and
  `ApiVmHost` carries the same registry so a plan-created VM is gated
  identically. Off by default and settable from TOML: `RegistryConfig::default`
  enforces, so enabling this against an empty catalogue refuses every boot image
  until images are registered and approved.
- **Policy governance over the MCP tool surface** (`hv2-agent`):
  `McpServer::set_policy_set` installs a `PolicySet` that is evaluated before
  every tool call — the questions capabilities cannot express, such as denying a
  destructive action on one named VM, or outside a maintenance window. A denial
  is refused *and* written to the audit log, since an unrecorded denial is the
  one an incident review most needs. Opt-in: with no set installed, the gate
  stays capabilities plus VM ownership, exactly as before. `PolicySet::new`
  denies by default, so an installed set must name everything agents may do,
  including tools added after it was written.
- **`VmMetrics` and `VmHost::metrics`** (`hv2-agent`): telemetry reaches the
  host. The `get_metrics` plan step and the `vm.metrics` agent tool now report
  a VM's real status, vCPU count, allocated memory, and uptime. Quantities the
  host cannot observe — `cpu_usage_percent`, `memory_used_bytes` — are `null`,
  never a placeholder number, so an agent can distinguish an idle guest from an
  uninstrumented one. `ApiVmHost` serves the same figures as
  `GET /api/v1/vms/{id}/metrics`, so a plan step and a direct request cannot
  disagree about a VM.

### Changed
- CI crypto/API measurements use a fresh Criterion directory outside the Cargo
  cache. Benchmark execution failures and missing, incomplete or invalid
  measurement data fail the workflow; raw artifacts are retained on failure.
- Successful SDK stdin, EOF and signal requests wake the affected process's
  output poller rather than waiting for its next 50 ms tick. Unsolicited output
  retains periodic polling. The guest agent publishes process exit only after
  its stdout/stderr readers finish, and polls observe completion before draining
  final bytes, preventing a faster poll from losing final command output.
- **ML-KEM and ML-DSA come from IronCrypto** (`hv2-core`). All six
  parameter sets now use `ic-mlkem` and `ic-mldsa` 0.2.3, checked against
  NIST's ACVP vectors and self-tested by `ic-fips`, so `FipsMode::Strict`
  admits them. Stored keys carry over: both use the same FIPS seeds as
  RustCrypto's crates did, and tests prove each side's keys, ciphertexts and
  signatures work with the other. ML-DSA signing is now hedged (FIPS 204's
  default) rather than deterministic. SLH-DSA stays on RustCrypto.
- **IronCrypto 0.2.2** (`hv2-core`, `hv2-api`), from 0.1.x: all 13 `ic-*`
  crates. It brings the RSA-4096 fix reported from here, faster
  SHA-256/HMAC/HKDF on SHA-NI and faster AES-GCM, constant-time fixes in the
  NIST field code, and the vector-checked `ic-mldsa`. None of 0.2.0's
  breaking changes reach HyperMachine's usage. RSA-4096 key generation and
  signing work again, and the refusal below is gone.
- **macOS no longer claims a process-count limit** (`hv2-sandbox`). Breaking
  for a caller that asked for one: a spec with `max_processes` set and not
  `best_effort` was accepted by 1.1.0 and is now refused with
  `SandboxError::Unsupported`. On macOS the only mechanism was
  `RLIMIT_NPROC`, which caps every process the *user* owns rather than the
  workload's. A desktop user already over the limit had every spawn refused,
  and a user under it got a workload that could fork until the user's total
  reached the limit. Neither is "at most N processes". `Controls::reason`
  says so. A caller that set no process limit, such as Lit, is unaffected.
- **Refusal reasons on Linux say what to do** (`hv2-sandbox`). On a host
  where AppArmor restricts unprivileged user namespaces (the default since
  Ubuntu 23.10), the three namespace-based reasons name
  `kernel.apparmor_restrict_unprivileged_userns`. The cgroup reason names a
  delegated cgroup (`systemd-run --user --scope -p Delegate=yes`). Both end
  with the microVM sandbox as the way out.
- **Six tools that reported an effect nothing performed now refuse**
  (`hv2-agent`). Each returned a plausible success and did nothing, which is
  the defect `execute_plan` had and which the `vm.exec` fallback in the same
  file already refuses by name:

  - `snapshot.create` minted an id, a name and a timestamp and captured no
    memory, disk or device state; `snapshot.restore` checked that the id
    existed and reported `"restored"` without touching the VM. Together they
    turned an agent's recovery plan -- snapshot, try the risky thing, restore
    on failure -- into a no-op that reported success at every step. There is no
    snapshot host in this project for any installation to supply, so the
    refusal is unconditional.
  - `network.attach` returned an `interface_id` for an interface it did not
    create, writing nowhere and discarding the `mac_address` it accepts;
    `network.detach` performed no lookup at all, so any invented id on any VM
    came back `"detached"`.
  - `agent.broadcast` reported a `recipients` count that was the number of
    sessions that exist, presented as the number that received something;
    `agent.send` reported `"delivered"` after finding a session and writing
    nothing to it. `AgentSession` has no inbox, so two agents told to
    coordinate this way would each be told delivery succeeded and would wait
    for a reply that cannot come.

  Two tests had to be corrected to land this, and both are the point: an
  end-to-end "full workload lifecycle" test asserted the snapshot round trip
  succeeded -- it did, and it did nothing -- and an integration test asserted
  `snapshot.create` "falls through to the session mirror". A green test over a
  no-op is how a fabricated feature survives.
- **One producer of the guest-channel kernel argument** (`hv2-agent`).
  `LocalVmHost` rendered `virtio_mmio.device=` itself, merged it into the boot
  source, and then cross-checked its own string against the device's once the
  device existed. That check was there to catch the two drifting apart -- which
  is a thing worth checking only while two things build one string. Now that
  the VM applies the argument itself when it loads the image, the merge and the
  check are both gone and `guest_channel_kernel_arg` delegates to
  `VM::vsock_kernel_args_for`. Sixteen lines removed and one class of drift with
  them.

  The tests that covered it were asserting on the *configured* boot source,
  which is no longer where the argument lands -- so they would have passed while
  a guest booted without it. They now assert on what the guest will actually be
  booted with, via the new `VM::extra_kernel_args`.
- **`Admin` no longer implies `HostExec`** (`hv2-agent`). Every other capability
  is implied by the `Admin` wildcard, and every other tool acts on VMs the
  server manages; `sandbox.run` acts on the machine the server runs on. Leaving
  it in the wildcard would have granted host execution to every session already
  holding `Admin` the moment the tool shipped — a privilege expansion nobody
  would have written down. `AgentCapabilities::full()` now names it explicitly,
  so "full" still means full. `PolicyAction::HostExec` and
  `ToolCategory::GuestExecution` are likewise separate from their guest-side
  counterparts: a program in a guest cannot reach the host, and one on the host
  can, however well confined.
- **Documented the agent enforcement boundary** (`hv2-agent`, `hv2-api`). An
  audit of the security-shaped types found several that decide but never
  intercept, described as though they were active. `limits` claimed "real-time
  enforcement to prevent runaway agents"; `policies` claimed to control "what AI
  agents can and cannot do". Both are toolkits that take effect only where a
  caller consults them, and neither had a caller. `policies` has since been
  given an opt-in enforcement point on the MCP tool surface (see Added);
  `limits` remains consult-only, and its `RateLimiter` is *not* the limiter the
  MCP server uses — that is `McpConfig::rate_limit`, which was always enforced.
  `permissions` is wired into a request path by `hv2-api`'s permission
  middleware. `Operation.rate_limit`, published to agents through
  `GET /agentic/ontology`, is advisory: nothing reads it back to reject a
  request.
- The `vm.create` MCP schema advertises a structured `boot` object in place of
  the `boot_image` string, which no handler read.
- `/agentic/plans/execute`, as served by `create_router_with_state`, executes
  plans against the server's own VM inventory instead of simulating. Simulation
  remains available through `execute_plan` and is still tagged
  `"simulated": true`, so the two can never be confused.

### Known gaps
- **A device interrupt is delivered, but only as a side effect of a guest
  access.** `pending_interrupt` is polled after each I/O access, so a device
  whose condition becomes true while the guest is *not* touching it -- input
  arriving on a serial port, a timer expiring -- has no way to say so until
  the guest happens to ask. That is enough for a UART the guest is actively
  driving and is not enough in general.

### Known gaps
- **No vsock data has moved yet.** The driver is bound and the guest agent is
  listening on port 1024 inside, but a host-initiated connection stays in
  `Connecting`: the guest never answers, so the host-to-guest packet is not
  completing through the receive queue. Enumeration and negotiation are done;
  the data path is not, and `vm.exec` remains an API with nothing behind it.
- **A device interrupt still needs a guest access to be noticed in one case.**
  `pending_interrupt` is polled after an access; self-raised interrupts go
  through `InterruptSink` instead. Devices that use neither -- currently the
  virtio transport -- cannot signal the guest at all.

### Known gaps
- **vsock still moves no data, and the reason is now specific.** With the
  publish-and-signal path in place, a host-initiated connection still stays in
  `Connecting`, and the device can see why: the guest's driver has programmed
  all three ring addresses for the receive queue (`desc`, `avail`, `used`, and
  `QUEUE_READY` set, size 256) but `avail_idx` reads 0 and the descriptor table
  and available ring are both **all zeros** in guest memory at the addresses
  the driver itself supplied. So the device is not failing to find posted
  buffers -- there are none to find. Whether Linux's `virtio_vsock` driver
  never filled the queue, or filled it somewhere this VM's view of guest memory
  does not reach, is the next thing to establish. Writes in the other direction
  are known good: this is how the kernel image gets into the guest.
- **`vm.exec` cannot reach a guest through the tool surface at all**, for a
  reason upstream of the above: nothing on the MCP path ever attaches a vsock
  device. `LocalVmHost::start` never calls `attach_guest_channel`, `VmSpec` has
  no field for a guest CID, and nothing merges `vsock_kernel_args()` into the
  guest's command line -- only the boot probe does that, by hand. The host-side
  chain from `vm.exec` down to `VsockDevice` is otherwise unbroken and drives
  the emulated device directly rather than the host's own `AF_VSOCK` stack.
### Known gaps
### Known gaps
### Known gaps
### Known gaps
- **The PIT models the interface, not the interrupt** (`hv2-core`).
  `TimerDevice` raises IRQ 0 on the userspace PIC only, and unlike the keyboard
  and the serial port it was left that way deliberately: KVM is asked for an
  in-kernel PIT, the guest's clock comes from there, and delivering this
  device's tick as well would give the guest two per period and a clock that
  runs fast. Said in the type's documentation, because "raises IRQ 0" is
  otherwise a reasonable thing to assume from the code.

### Known gaps
- **`AgentPolicy` quotas and rate limits are recorded, not enforced**
  (`hv2-agent`). `allows` consults `enabled` and `permissions` and nothing
  else, so a `max_vms` of five does not stop a sixth, and
  `PolicyError::QuotaExceeded` is never constructed. Both fields now say so, in
  the style the sibling module already uses -- the module documentation
  explains that `PolicySet` *is* wired up, which made silence about these read
  as endorsement. Enforcing them needs usage counters that do not exist:
  `PolicyContext` carries an agent id and a clock, not a tally, and where such
  a tally should live is a design decision rather than an oversight.

### Fixed
- **Secure boot verifies signatures** (`hv2-core`). It had admitted any
  component whose claimed signer's subject string matched a trusted entry
  (to 2026-09-22), then refused every signed component. Now each signature is
  checked against the trusted database entry's public key, a DER
  SubjectPublicKeyInfo, never the certificate travelling with it. It covers
  `BootComponent::signed_message()`: a domain separator, the component type
  and the image digest. RSA PKCS#1 v1.5 and ECDSA P-256/P-384 are supported,
  and validity windows, status and dbx-listed keys are honoured. This is
  HyperMachine's own format, not UEFI Authenticode.
- **`FipsMode::Strict` refused nothing** (`hv2-core`). Documented as "fail if
  non-FIPS operation attempted", it was never read: `AlgorithmNotApproved`
  was defined and never returned, and `Strict` behaved exactly like
  `Enabled`. Now every operation is checked against IronCrypto's `ic-fips`
  before it runs. The post-quantum algorithms, which come from RustCrypto
  and outside the boundary `ic-fips` self-tests, are refused with a reason.
- **FIPS modes drew keys from ChaCha12** (`hv2-core`). `Enabled` and `Strict`
  now construct only after `ic-fips` runs IronCrypto's pre-operational
  self-tests, and they take random bytes from an SP 800-90A HMAC_DRBG
  (`ic-drbg`). `Disabled` is unchanged. None of this is CMVP validation.
- **RSA-4096 key generation and signing panicked** (`hv2-core`, in IronCrypto's
  `ic-rsa` 0.1.3), which under `panic = "abort"` ends the process. They were
  refused with `UnsupportedAlgorithm` until IronCrypto 0.2.2 fixed the cause;
  now RSA-4096 has an end-to-end test like every other size. Every offered
  RSA signature algorithm is now tested, where two were.
- **AES-128-GCM did not work** (`hv2-core`). It was listed among the approved
  algorithms, `AesKeySize::Aes128` generated 16-byte keys, and
  `aes_gcm_encrypt`/`aes_gcm_decrypt` accepted them, but the implementation
  built IronCrypto's `Aes256Gcm` for every key, so every AES-128 call failed.
  It had been broken since the move to IronCrypto. The cipher now follows the
  key length, and AES-128 is checked against GCM-spec Test Case 2.
- **A process limit that counted the user, not the workload** (`hv2-sandbox`,
  Linux). `Control::ProcessCount` set `RLIMIT_NPROC` beside the cgroup's
  `pids.max`. `RLIMIT_NPROC` counts every task the user owns, host-wide and
  threads included, so `SandboxSpec::untrusted`'s default of 64 had every
  spawn that forked refused with `EAGAIN` whenever the user already ran more
  than 64 threads. Linux now relies on `pids.max`, which counts only the
  workload's cgroup.
- **A new root plus a PID namespace could not start on Ubuntu**
  (`hv2-sandbox`, Linux). The fresh `/proc` was mounted after the pivot, when
  the kernel no longer lets a user namespace mount one (`EPERM`). It is now
  mounted inside the new root before the pivot. WSL2's kernel allowed the old
  order, which is why it went unseen.
- **hv2-sandbox's containment tests had never run in CI.** On
  `ubuntu-latest` every one skipped itself, because AppArmor blocks user
  namespaces there and no cgroup is writable. The new *Sandbox Containment*
  job grants both, and fails if the probe reports fewer than all eight
  controls or if any test skips. Both bugs above were found by it.
- **Four more parameters that were accepted and ignored** (`hv2-agent`).
  - A **disabled tool was hidden, not refused**. `McpTool::enabled` was
    filtered in `list_tools` and never checked on the call path, so a tool
    disabled in future would still run for anyone who knew its name. Nothing
    can disable one today, which is why it was worth closing before something
    can.
  - **`agent.send` could not be called as documented.** Its schema names the
    parameter `target_agent`; the handler read `target_agent_id`, so a
    schema-conforming call failed on a missing parameter it had supplied under
    the documented name. Both are accepted now, since the undocumented one has
    been the only one that worked.
  - **`guest.file.write` ignored `encoding`.** The schema offers `text` and
    `base64`; a base64 payload was forwarded as literal base64 text, writing
    the characters of the encoding rather than the bytes they stand for, with
    the caller told it succeeded. There is no decoder here, so the encoding is
    refused rather than mis-delivered.
  - **`guest.file.read` ignored `max_size_kb`**, so the request reaching the
    guest agent carried no bound at all. It is carried through now.
- **`vm.create` over the MCP tool path dropped the kernel** (`hm-cli`). It
  deserialised a `CreateVmRequest` carrying `boot` and then called the
  five-argument `create_vm`, which hardcodes `boot: None`. An agent that
  supplied a kernel got a success record back and a VM with no guest code in
  it. The REST handler on the same request type already called
  `create_bootable_vm`; only the tool path dropped the field.
- **Three of four system prompts told agents that scripts run inside the guest**
  (`hm-cli`). `vm.execute_script` evaluates a Rhai expression on the *host*
  against a read-only view of a VM -- four scope variables and no I/O -- and
  `SystemPrompts::generic` had been corrected to say so, while the OpenAI,
  Anthropic and Gemini prompts still said "Execute scripts within VMs". A wrong
  prompt is worse than a wrong schema: it shapes the agent's plan before any
  schema is read.
- **The ontology taught that same tool as a shell** (`hm-cli`). Its only
  example was `echo 'Hello, World!'` returning `{"success": true, "output":
  "Hello, World!
", "exit_code": 0, "execution_time_ms": 15}` -- an input the
  engine cannot run and an output shape it never produces -- and the declared
  `ScriptResult` type promised the same four fields. Both are served verbatim
  to agents through `/agentic/schema` and the vendor tool exports, so the wrong
  shape was taught everywhere. The example is now a Rhai expression over the
  VM's scope, and `ScriptResult` says what the engine actually returns: the
  final expression's value, as JSON.
- **No tool call was bounded in time** (`hv2-agent`).
  `McpConfig::default_timeout` documented itself as the default tool timeout
  and `ToolCallRequest::timeout` as a per-call override; neither was ever read,
  and `call_tool` awaited without limit. A hung `vm.exec` or `sandbox.run` held
  the request forever and held its concurrency permit with it, so one stuck
  guest could take the surface down while every field that promised otherwise
  sat unused. Both are honoured now.
- **`agent.claim` handed out the access it advertised as a restriction**
  (`hv2-agent`). Its description promises exclusive access and that it
  "prevents other agents from modifying" a VM. What it did was append the id to
  the calling session's `owned_vms` with no checks at all -- and `owned_vms` is
  the authorisation gate (`session_owns`) for `vm.delete`, `vm.exec` and the
  rest. So claiming another agent's VM did not protect it, it granted full
  access to it. A claim held by another session is now refused, and the reply
  says plainly that this server keeps no timed leases rather than implying the
  `duration_seconds` it accepts means something.
- **The microVM sandbox claimed a confinement it discarded** (`hv2-agent`).
  `MicroVmSandbox::controls` reported `Control::ProcessCount` as enforced while
  `max_processes` never left the host -- the name appears nowhere in the crate.
  Because `unenforced` came back empty, and this crate defines that as "every
  requested control was applied", a caller asking for one process was told the
  limit held while the guest could fork freely. It is now refused with a reason,
  which is the pattern the sandbox crate already prescribes.

  The test covering it asserted that *every* control was enforced, which is what
  kept the claim alive: it agreed with the advertisement rather than checking
  the behaviour.
- **The looser of two deadlines won** (`hv2-agent`). `wall_clock.or(cpu_time)`
  returned the wall clock whenever one was set, so one second of CPU inside a
  ten-minute wall clock bounded at ten minutes, with both controls still
  reported as enforced. It takes the minimum now, as `AgentVM` already did for
  the same choice.
- **Two tool parameters were accepted and dropped** (`hv2-agent`). `guest.exec`
  read `timeout`, while its schema and the sibling `vm.exec` path both say
  `timeout_seconds`, so a caller's deadline was silently replaced by the
  default. `vm.create` never read `gpu_enabled` or `network_enabled` -- `VmSpec`
  has both fields and the host forwards them, the dispatcher just never looked
  -- so every VM built through that surface had networking off while the
  schema documented a default of `true`.
- **An injected keystroke never reached the guest** (`hv2-core`).
  `KeyboardDevice::inject_scancode` is a public API for "give the guest a
  keystroke", and it raised IRQ 1 only on the userspace `Pic8259` -- which a
  guest whose interrupt controller lives inside the hypervisor never reads. The
  byte went into the output buffer and the guest was never told. It also had an
  inherent `has_pending_interrupt` that the dispatch layer could not see,
  because the `Device` trait method did not exist: exactly the defect the RTC
  carried and its own comment describes, left in the device beside it. Both
  paths are wired now.

  Found by the sweep this session's failures argued for -- looking deliberately
  for an API that describes an effect nothing performs, rather than waiting for
  each one to surface as a hang.
- **Three defects between the guest channel and the API in front of it**
  (`hv2-core`, `hv2-agent`), each of which made a working transport look like a
  dead guest, and none of which the boot probe could have found -- it drove the
  device by hand, so it never used the API the way an embedder must.

  **Queued was not delivered.** A host-side packet sits in the device until
  something moves it into a receive buffer the driver posted and signals the
  used queue. The probe did that after every step; nothing else did. So
  `exec_in_guest` queued a connection request no guest ever saw and then timed
  out reporting that the guest might not be running. Delivery is now driven by
  the device itself, through `PendingWake`: queueing a packet says so, and the
  VM delivers it on a dedicated thread. A rule the caller has to remember is
  not a design, and this one had exactly one caller who remembered.

  **The kernel argument was reported, not applied.** `attach_guest_channel`
  returned the `virtio_mmio.device=` argument a guest needs and left putting it
  on the command line to the caller -- who cannot have written it down, because
  the device did not exist when the boot source was described. A caller who
  misses it gets a guest that boots perfectly and enumerates nothing, which
  reads as a broken device. The VM now records the argument at attach and
  applies it when the image is loaded.

  **The host port was a constant.** `VsockChannel` always connected from port
  2048, and a closed connection keeps its port pair until it is forgotten, so
  the *second* call on a VM was refused for colliding with the first -- worse
  than the concurrency limit it was recorded as. Ports are now allocated from
  an ephemeral range that rotates: freeing a port does not hand it straight
  back out, because the guest may still hold the other end and a reused pair
  goes unanswered. The client also forgets a connection when it drops, and
  treats a refusal as retryable within the caller's timeout -- an agent serving
  one caller at a time is briefly busy rather than absent.
- **A guest could wait on an interrupt that only its own idleness prevented**
  (`hv2-core`). Interrupts raised by device code were delivered by a task on the
  same tokio runtime as the vCPU loop -- and the vCPU loop calls `KVM_RUN`,
  which is a blocking ioctl. While the guest runs it does not return, and while
  the guest is *idle* it does not return for a long time, because KVM halts the
  vCPU in the kernel and waits there for an interrupt. Either way it occupies a
  runtime worker.

  So the two could meet in the middle: a halted guest waiting for an interrupt,
  and the interrupt that would wake it queued behind a worker the guest itself
  was occupying. Nothing deadlocked outright -- the guest woke on the next timer
  tick or console byte and found data that had been there all along -- so it
  presented as seconds of latency, intermittently, which is far harder to see
  than a hang. A trace makes it plain: the delivery thread logs its last line,
  then two interrupts are queued with `sent=true` and neither is ever acted on.

  Delivery now runs on a dedicated OS thread. Measured on the configuration that
  showed it worst -- an idle guest with a quiet console -- the agent round trip
  went from 0 of 1 runs answering to 4 of 4.

  This is also why the vsock round trip had been intermittent, and why neither
  an edge pulse nor a held level line changed anything: the trigger mode never
  mattered, because the line was not being driven at all.
- **The vsock operation numbers were each one below the specification's**
  (`hv2-core`). This is why no data ever moved over a channel whose handshake
  worked perfectly. `REQUEST` (1) and `RESPONSE` (2) happened to be correct;
  everything else was wrong. What the host sent as `RW` arrived at a Linux
  guest as `SHUTDOWN` with flags of zero -- which that stack handles by doing
  nothing at all: no data delivered, no reply, no reset, and not one line in
  the guest's log. In the other direction the guest's real `RW` (5) was read as
  `CREDIT_UPDATE` and its `CREDIT_UPDATE` (6) as `CREDIT_REQUEST`, so a reply
  that did arrive was discarded as a bookkeeping packet.

  Twenty tests covered this device and all twenty passed against the wrong
  table, because each compared the device with itself: a test that asserts
  `op::RW == op::RW` passes against any numbering. The replacement test writes
  the eight values out from section 5.10.6.1 of the specification instead.

  Found by having the guest agent write before reading, which made the guest
  send a packet of its own -- the first packet in the whole project whose
  operation number came from Linux rather than from this codebase, and it did
  not match.
- **A virtio interrupt was pulsed rather than held** (`hv2-core`). A virtio
  interrupt is level-triggered: the driver clears it by writing `InterruptACK`,
  and a line asserted and released before that acknowledgement can be dropped
  between one delivery and the next. `InterruptSink` gained `assert_line` and
  `deassert_line` for it, defaulting to the old pulse so a sink that predates
  them is no worse than it was; the transport holds the line while any status
  bit is outstanding and releases it when the driver has acknowledged them all.
- **The device model and the guest were reading different memory**
  (`hv2-core`). This is the root cause of everything below it. `VM::new`
  created a `GuestMemory` and the backend separately allocated the pages it
  registered with the hypervisor -- two buffers. The guest ran in the
  backend's; every emulated device read and wrote the other one. So a virtio
  driver published descriptors into memory the device could not see, and the
  device wrote replies where the guest would never look.

  Nothing caught it, and the reason is worth recording: the boot path writes
  the kernel image through the *backend*, so guests booted perfectly, printed,
  ran userspace and took an interactive shell. Only a device that reads guest
  memory could notice, and until virtio-vsock was driven by a real Linux driver
  there was none. The symptom was a receive queue whose descriptor table and
  available ring read as all zeros at the addresses the driver had just
  programmed -- which reads like a driver that posted no buffers, and was
  really a device looking at the wrong pages.

  `GuestMemory::adopt_backend_pages` points the regions at the backend's
  allocation and releases its own, called from `VM::provision` as soon as the
  backend VM exists. `HypervisorBackend::guest_memory_host_addr` is how a
  backend reports pages it owns; the default is `None`, meaning the caller's
  allocation was already the right one.
- **A timer test measured the host's scheduler rather than the timer**
  (`hv2-core`). It counted PIT ticks across a real wall-clock second and
  allowed 15 to 21. The interval uses `MissedTickBehavior::Skip`, which is
  right for a timer and means a loaded machine genuinely loses ticks, so the
  test failed under parallel builds for a reason unrelated to the code -- it
  had done so repeatedly. It runs on paused time now, where the count is exact
  (19) and the assertion fails only if the configured period changes.
- **The transmit interrupt was never delivered, for three separate reasons**
  (`hv2-core`). Each one hid the next.
  1. `IIR` gated the transmit interrupt on a `thr_empty` flag that was set
     false on every write to the transmit register and only set true again
     when the *host* drained the buffer. So after the first byte, the register
     that a driver's handler reads to find out why it was interrupted said
     "nothing to do" -- forever. This device transmits the instant the guest
     writes, so the holding register is empty again before the write returns.
  2. The interrupt pulse was wired into `handle_exit_static` only, and a
     single-vCPU guest takes the instance path. The plumbing was correct and
     ran zero times; a trace counting pulses reported exactly none.
  3. A FIFO reset erased the transcript, fixed earlier in this release, which
     is why the first two were invisible: the boot log looked empty for a
     reason that had nothing to do with interrupts.
- **A correction to an earlier diagnosis in this changelog.** An entry here
  previously said the guest "writes `IER = 0` every time and never sets the
  `OUT2` bit in `MCR`", concluding it ran the port without interrupts. That
  was wrong, and wrong in an avoidable way: it was measured from a capture
  that never reached the tty driver's startup, because the logging needed to
  see the registers slowed the guest enough that it never got there. Traced
  properly -- narrow filter, full boot -- the driver writes `IER = 0x07`
  sixty-seven times and `MCR = 0x0b`, `OUT2` included. It was asking for
  interrupts all along and not getting them.
- **The initrd was placed where the kernel unpacks itself** (`hv2-core`). It
  went to a fixed 32 MB. A compressed kernel unpacks into `init_size` bytes
  starting from where it will run -- 62 MB from 16 MB for the kernel tested
  here -- so 32 MB is *inside* that region for any kernel of ordinary size, and
  decompression wrote straight over the initrd. The kernel then reported
  `invalid magic at start of compressed archive` about bytes it had destroyed
  itself, fell back to treating it as a block device, and panicked with
  `Unable to mount root fs`. The initrd is now placed as high as it will go,
  under the header's `initrd_addr_max`, clamped to the guest's memory, page
  aligned, and refused with "give the guest more memory" if it cannot be
  placed clear of the unpack region. Two tests that asserted the fixed address
  now assert the property instead, since the constant is what hid the
  collision.
- **A FIFO reset erased the console transcript** (`hv2-core`). Linux resets
  both FIFOs when its 8250 driver takes the port over from earlyprintk, and
  `SerialDevice` responded by clearing the transmit buffer -- so every guest
  that got far enough to initialise its serial driver properly wiped its own
  boot log on the way past, and the VM looked as though it had never printed at
  all. On real hardware a transmit-FIFO reset discards bytes that have not gone
  out yet; here a THR write *is* the transmission, so there is nothing unsent
  to discard and the buffer is the transcript. The receive direction still
  discards, because unread input is exactly what that reset is for.
- **Every device access was four bytes wide, whatever the guest asked for**
  (`hv2-core`). `IoDeviceHandle`/`MmioDeviceHandle` built a `[u8; 4]` and
  handed the whole thing to the device, discarding the access size the exit
  had reported. Register files are byte-wide and their reads have side
  effects, so a one-byte `inb` of a UART made the device pop its receive
  buffer *and* clear its interrupt-identification register; a one-byte `outb`
  of a character wrote the three registers after the target with the padding
  bytes, clearing interrupt-enable, FIFO-control and line-control on every
  character printed. It looked like it worked, which is why it survived a
  kernel boot. The width now travels with the access and a device is handed
  exactly the bytes the guest asked for.
- **An unhandled guest exception livelocked the VM** (`hv2-core`). Only a
  double fault stopped the VM; every other exception was logged at warn level
  and the vCPU resumed, which re-executed the faulting instruction and took the
  same exception again, forever. A guest that hit one stopped making progress
  while the VM stayed `Running` and nothing said why -- observed as roughly
  60,000 exits a second, all identical. An exception reaches userspace only
  because the backend could neither handle nor emulate it, and this VMM does
  neither and cannot yet inject one back into the guest, so it stops and
  reports the vector. `KVM_SET_VCPU_EVENTS` would make injection possible and
  turn this into the fallback rather than the whole answer.
- **The PIT refused wide reads and dropped wide writes** (`hv2-core`, found by
  an audit prompted by the three device defects a kernel boot turned up).
  `TimerDevice::read` returned `Err` for any access that was not exactly one
  byte -- and an error there stops the VM -- while `write` consumed `data[0]`
  and silently ignored the rest. Both now walk consecutive registers a byte at
  a time, which is also what the channel latch semantics require. The same
  audit found the same shape in `IdeController` and `VgaDevice`; neither is
  registered with the device manager, and both now say so in their module
  docs, along with what wiring them up would require.
- **The setup header was truncated at the length it had in 2009** (`hv2-core`).
  `create_boot_params` copied a fixed `0x1f1..0x250` out of the bzImage, which
  was the whole header under boot protocol 2.09 and has not been since. The
  header does not have a fixed length -- the image says where it ends, at
  `0x202 + the byte at 0x201` -- so everything past 0x250 reached the guest as
  zero. `init_size` at 0x260 is the field that bites: the kernel computes its
  stack pointer from it, so zero put `%rsp` somewhere unmapped and the guest
  triple-faulted on the first `push`, with no console, no exception, and a
  vCPU that KVM had already reset. The header extent is read from the image now
  and clamped at both ends, since it is a guest-supplied number deciding how
  much gets copied.
- **Three devices that could not work behind the device manager** (`hv2-core`).
  All three shared a shape: correct when a unit test called them directly, and
  broken the moment a guest did. `SerialDevice::read` refused anything but a
  single byte, and the refusal reached `handle_exit` as a device error that
  stopped the VM -- so a kernel probing the port with a word read killed its own
  guest, where hardware would simply have answered; its `write` silently dropped
  every byte after the first. `RtcDevice` and `KeyboardDevice` decoded the
  absolute ports 0x70/0x71 and 0x60/0x64, but `DeviceManager` passes
  `port - base_port`, so every real access arrived as a small offset, fell
  through to an error arm, and stopped the VM. A 16550 and an i8042 are
  byte-wide register files: a wide access now walks consecutive registers, and
  a port the device does not implement reads as absent rather than as a
  failure. Found by booting a kernel against them, one after the next.
- **A search hit returned the whole payload when it happened to be small**
  (`hv2-context`, found by its own integration test). Previews were bounded in
  the payload store and nowhere else, so an inline payload -- anything under the
  8 KiB externalization threshold -- came back from `search` in full, once per
  hit. A search over a long session was then as expensive as re-reading it,
  which is the exact cost the crate exists to avoid. The index bounds previews
  itself now, whatever the storage decided.
- **A Linux kernel was handed no memory map and no CPU** (`hv2-core`).
  `BootSource::Linux` has always described itself as implementing the Linux
  boot protocol. Running one found two pieces of it missing. `boot_params`
  carried no `e820` map: the byte at `0x1e8` was zero and the table at `0x2d0`
  was empty, and a guest booted this way runs no BIOS, so there is no
  `INT 15h` for the kernel to fall back on -- it finds no RAM at all and stops
  before it has a console to say so on. The map is built now from the guest's
  memory size, which `LoadedBoot::set_memory_size` threads in from the VM
  (separate from `load`, because callers validate images before a VM exists),
  and asking for the memory regions without one is refused rather than
  answered with an empty map. Separately, no KVM vCPU was ever given a CPUID
  configuration: `set_cpuid` and `get_supported_cpuid` were both written and
  neither was ever called, so the guest's `CPUID` reported no vendor, no
  features and a maximum leaf of zero. A Linux kernel asks within its first
  few dozen instructions. `KvmVm::create_vcpu` now applies the host's
  supported set. The visible effect is a guest that executes instead of
  spinning: it now runs and triple-faults, which is a failure that can be
  worked on. `examples/linux_boot_probe.rs` is the program that reports it.
  The 32-bit protected-mode entry state itself was verified separately and is
  correct -- a hand-assembled 32-bit image entered at 1 MB executes and drives
  COM1 -- so what remains is specific to the Linux path. The kernel still does
  not boot; nothing here claims otherwise.
- **`HypervisorPlatform::detect` reported KVM on hosts that could not use it**
  (`hv2-core`). It tested `Path::new("/dev/kvm").exists()`, which is true
  wherever the module is loaded -- including the very common case of a user not
  in the `kvm` group. `detect` then returned `Kvm` and every call after it
  failed with a permission error naming none of this. It opens the device now,
  which is the question it was always asking. Verified both ways on one
  machine: as root `Kvm`, unprivileged `Tcg`. Same shape as
  `SetInformationJobObject` succeeding not meaning memory is capped.
- **A container runtime that reported success for things that never happened**
  (`hv2-core`). `ContainerRuntime::start` invented a PID — `1000 + n` — and
  marked the container `Running`, so a caller could not tell it from a working
  runtime: the state was right, the PID was plausible, and nothing was confined
  or even executing. Writing the test for that turned up the same defect beside
  it: `kill()` returned `Ok(())` for a container in any state other than
  `Running`, reporting a signal delivered to a process that did not exist. Both
  refuse now, and the module says plainly that it models the OCI spec without
  implementing it. `Container::start` is unchanged and stays honest — it takes a
  PID from its caller, so whoever supplies one is the party that knows it is
  real.
- **The Linux sandbox claimed isolation the workload could see through**
  (`hv2-sandbox`). Found by running it on a real kernel rather than
  cross-compiling for one. `/proc` and `/sys` are not ordinary directories —
  each is a view of the namespace it was *mounted in* — so an inherited pair
  left a workload correctly PID 1 in its own namespace while it enumerated 48
  host processes, and showed only loopback on netlink while `/sys/class/net`
  listed the host's interfaces. "Cannot signal" was true and "cannot see" was
  not. Both namespaces now carry `CLONE_NEWNS` and remount the filesystem that
  describes them, and both probes rehearse the remount rather than only the
  `unshare`.
- **`best_effort` attempted controls the probe had already refused**
  (`hv2-sandbox`). On a host without cgroup delegation it promised to run with
  whatever was available and then failed trying to create a cgroup its own
  probe had reported unavailable. `Sandbox::run` now filters the spec through
  `SandboxSpec::without_controls` before handing it to a backend, so no backend
  can attempt a control its probe rejected.
- **Boot admission was decided after the backend had already been asked for a
  partition** (`hv2-core`). `VM::provision` called `backend.create_vm` first, so
  a refused image had still cost hypervisor resources — and on a host where
  `create_vm` fails the refusal was masked by the backend's error entirely.
  Resolution, admission, and the memory-fit check now all happen before the
  backend is touched.
- **A flaky PIT timer test** (`hv2-core`) asserted a background task's
  wall-clock throughput. The timer uses `MissedTickBehavior::Skip`, so under a
  loaded machine ticks are dropped rather than queued and the count falls short.
  It now drives `tokio::time::pause`/`advance` against virtual time, bounded so
  a timer that never ticks still fails. Adds `tokio`'s `test-util` feature as a
  dev-dependency.
- **The image allowlist was never consulted before booting an image**
  (`hv2-core`). `ImageRegistry`'s own doc comment claimed admission checks that
  "the scheduler and VM provisioning path call before launching workloads";
  neither did. `check_admission` was reachable only from an advisory REST
  endpoint and tests, so an image could be denied or revoked in the registry and
  still boot. There is now a real enforcement point (see Added), and the doc
  states what is and is not on it.
- **`AgentVMBuilder` had no setter for `capabilities` or `sandbox_config`**
  (`hv2-agent`). Both fields existed, were passed to `ScriptEngine` and
  `Sandbox`, and were unreachable from any caller — so every `AgentVM` ran with
  the defaults and neither control could be tightened. Added
  `AgentVMBuilder::capabilities` and `AgentVMBuilder::sandbox`.
- **`SandboxConfig::max_cpu_time` bounded nothing** (`hv2-agent`). Script
  execution was bounded only by the builder's `script_timeout`, so a caller who
  tightened the sandbox limit got no effect. `AgentVM::effective_script_timeout`
  now takes the stricter of the two.
- **A flaky `hv2-runtime` health test** raced a 1 ms `check_interval`: it
  recorded a probe and immediately asserted no check was due, which fails if the
  thread is descheduled for a millisecond. It now uses a long interval, which a
  never-checked VM is due under regardless.
- **Two rustdoc warnings** (`hv2-agent`, `hv2-core`) — a redundant explicit link
  target and an unresolved link to `LoadedBoot::entry_point` — restoring the
  0-warning rustdoc build.
- **`ScriptEngine` never enforced its capability set** (`hv2-agent`). The engine
  was constructed with a `CapabilitySet` and then never consulted it, so an
  engine deliberately built with no capabilities handed scripts the same VM view
  as a fully privileged one. `execute` now requires `Capability::VmRead`.
- **`execute_script` was described to agents as running inside the guest**
  (`hv2-api`, `hm-cli`). It evaluates a Rhai script *on the host* against a
  read-only view of the VM — there is no in-guest agent — and the scope holds
  four scalars, not the "VM control operations" the ontology advertised. Four
  descriptions across the REST ontology, the CLI ontology, the CLI MCP server,
  and the LLM adapter prompt were corrected, and the documented example was
  changed from `echo 'Hello'` (a shell command Rhai cannot parse) to Rhai.
- **`vm.metrics` returned hard-coded zeros for every counter** (`hv2-agent`).
  Zero is a measurement; an agent reading 0% CPU on a busy VM would draw the
  wrong conclusion. Unmeasured fields are now `null`.
- **Multiboot `mods_addr` pointed at the module data rather than the module
  descriptor array** (`hv2-core`, WHPX). A Multiboot kernel walking that pointer
  read its module's contents as if they were addresses, so it would silently
  fail to find its initrd. Module command-line strings were never written at
  all.
- **Ontology plan steps with no dependencies executed in nondeterministic
  order** (`hv2-api`): `topological_sort` seeded its queue from a `HashMap`, so
  independent steps ran in hash order and a plan that happened to work was a
  coin flip. Ties now break toward declaration order.
- **A `stop()` racing a `launch()`** reported the resulting `InvalidState` as a
  VM failure (`hv2-core`).
- `GpuTopologyMap::devices_on_host` excludes allocated devices; its doc comment
  said otherwise.
- Clippy lints introduced by a newer toolchain (`float_literal_f32_fallback` in
  `hm-gui`, `for_kv_map` and an unused macOS import in `hv2-core`), restoring a
  `-D warnings` clean build on every supported target.

### CI
- **The release workflow never ran because of a billing block, not a defect.**
  All four build jobs at tag `v1.1.0` reported `steps: []` and no runner, and
  the check annotation reads "The job was not started because recent account
  payments have failed or your spending limit needs to be increased." No
  amount of workflow repair fixes that. Real defects behind it were fixed
  anyway, so the first run after billing is resolved has a chance: `protoc` was
  never installed although `hv2-cli` pulls in `hv2-api`, whose build script
  needs it; the aarch64 Linux target was built through `cross` in a container
  the host's `protoc` could not reach, and is now cross-compiled directly with
  `gcc-aarch64-linux-gnu` and an explicit linker.
- **Three workflow steps that had never run once** were repaired. `Miri` asked
  for the `miri` component on the *stable* toolchain: the job installs nightly,
  but `rust-toolchain.toml` pins `channel = "stable"` and overrides whatever the
  action installed, so bare `cargo miri` ran on a toolchain that never ships it
  — the same fix the HV1 jobs already carry. `Coverage` and `Benchmarks` both
  died before doing any work because `hv2-api`'s build script runs `prost-build`
  and neither workflow installed `protoc`, which every building job in `ci.yml`
  does. The `CLA` check pinned `contributor-assistant/github-action@v2`, a tag
  that does not exist — the action publishes only `vX.Y.Z` — so it failed to
  resolve on every pull request; note that repairing it makes an inert check
  enforce again. `Benchmarks` remains red for a separate reason: the action is
  configured with `tool: 'cargo'`, which parses libtest format, and the repo's
  benches are criterion, which does not emit it — `--output-format bencher` is
  not the fix, as it prints no `test <name>` prefix.


Master had been red since June, for reasons predating this work. Six
failures were stacked behind one another, each hidden by the one in front:

- **Tests `include_bytes!`d gitignored build artifacts.**
  `examples/guest_code/*.img` are `build.sh` outputs excluded by
  `.gitignore`, so a clean checkout failed to *compile*, which took down
  `cargo check` for `hv2-core` and cascaded into most jobs.
  `guest_code_integration.rs` and `guest_execution.rs` now skip when an
  image is absent, the way `exit_handling.rs` already skips without a
  hypervisor.
- **The HVF backend could never link on Apple Silicon.** It is built on
  Hypervisor.framework’s VMX API, which exists only on Intel Macs, but was
  gated on `target_os` alone. `cargo check` does not link, so this passed
  every cross-target check for months. Now gated
  `all(target_os = "macos", target_arch = "x86_64")`; Apple Silicon falls
  through to TCG.
- **`hv2-net` used `libc` on macOS without depending on it** — declared
  only under `cfg(target_os = "linux")`.
- **The ARM64 job asked an x86-only crate to build for ARM.** `hv1-core`’s
  own manifest says ARM belongs to `hv1-arm`; the step was removed rather
  than papered over. If `hv1-core` gains an ARM mode, it should return.
- **`coverage.yml` passed `--codecov` and `--html` to one `cargo llvm-cov`
  invocation**, which rejects the combination outright. Now one
  instrumented run plus two reports.
- **Four tests assumed the host had a working hypervisor** and failed on
  runners where `/dev/kvm` exists but is not accessible. The skip lives in
  the shared helper so later tests inherit it.

The HV1 jobs now pin an exact nightly (`nightly-2026-08-17`, the toolchain
that produced the first all-green run). Unpinned, they broke four times in
a day for reasons unrelated to any commit here: a new required method on
`core::iter::Step`, a renamed `rustc-abi` value, `unused_must_use` becoming
an error, and a yanked `spin` release. Note that `rust-toolchain.toml` pins
`channel = "stable"` and overrides the installed default, so those steps
name the toolchain explicitly; keep the date in the `with:` blocks and the
commands in step. `coverage.yml` and `security.yml` remain unpinned — they
did not break and use nightly for different tooling.

### Dependencies

- `x86_64` 0.15.4 → 0.15.5 — a recent nightly added required methods to
  `core::iter::Step`. The two are mutually exclusive by toolchain vintage.
- `bootloader` 0.11.15 → 0.11.17 — 0.11.15’s vendored target specs carry
  `"rustc-abi": "x86-softfloat"`, which current nightly rejects; 0.11.17
  ships `"softfloat"`. Its build script still fails to build its UEFI
  stage (`lock_api` 0.4.10, `can’t find crate for std`), reproducible on
  other nightlies and for the host target, so that step is
  `continue-on-error` pending an upstream fix.
- `spin` 0.9.8 → 0.9.9 — 0.9.8 is yanked.

## [1.1.0] - 2026-06-04

### Added
- **Complete agentic tool ontology** (`hv2-agent` / `hv2-api`): a 30-tool MCP
  registry (vm / gpu / guest / snapshot / network / agent / system — including
  `vm.pause` / `vm.resume` and the `gpu.*` fabric tools) is the single source of
  truth, projected identically to the native MCP manifest and the OpenAI /
  Anthropic / Gemini tool-use formats. All four agent transports are verified in
  lockstep by drift-guard tests.
- **GPU acceleration fast paths** (`hv2-gpu` / `hv2-core`): WGPU compute
  pipelines are cached across dispatches (no per-call recompile), and
  `TRANSFER_TO_HOST_2D` / framebuffer scroll collapse contiguous regions to a
  single `memcpy`.
- **VFIO passthrough mmap fast path** (`hv2-gpu`, Linux): MMIO maps the BAR
  regions (parsing the sparse-mmap capability chain) for lock-free volatile
  access, falling back to positioned `pread` / `pwrite` only for non-mappable
  ranges.
- **Real post-quantum cryptography** (`hv2-core`): ML-KEM (FIPS 203), ML-DSA
  (FIPS 204), and SLH-DSA (FIPS 205) now delegate to the audited pure-Rust
  RustCrypto crates (`ml-kem`, `ml-dsa`, `slh-dsa`) with real
  keygen/encapsulation/sign/verify and canonical byte serialization, replacing
  the previous SHA-256/HMAC API placeholders. Gated behind the default `pqc`
  feature.
- **Real RSA** (`hv2-core`): RSA key generation and PKCS#1 v1.5 / PSS signing
  via the pure-Rust `rsa` crate (the `ring` backend cannot generate RSA keys).
- **Agent-driven VM workflow example** (`hm-cli`): `agent_vm_workflow` shows an
  AI agent discovering tools (OpenAI/Anthropic/Gemini schemas) and driving a
  full VM lifecycle (`vm.create` → `start` → `execute_script` → `delete`)
  through the typed `ToolExecutor`.
- **MCP workload example** (`hv2-agent`): `agent_mcp_workflow` drives a complete
  VM lifecycle over the `McpServer` tool surface — capability-scoped session,
  tool discovery, provision → boot → `guest.exec` → snapshot → resize → restore
  → teardown — and prints the resulting audit log.
- **LLM tool-schema example** (`hv2-agent`): `llm_tool_schemas` projects the MCP
  tool registry into the OpenAI, Anthropic, and Gemini tool-use formats.
- **Multi-agent orchestration example** (`hv2-agent`): `multi_agent_orchestration`
  shows role-scoped agents, exclusive VM claims (conflict-free coordination), and
  inter-agent messaging via `AgentOrchestrator`.
- **GPU-fabric reservation example** (`hv2-runtime`): `gpu_fabric_reservation`
  publishes a GPU VM class, reserves capacity with SLA tiers, consumes/releases
  slots, and cancels — exercising `CapacityManager`.
- **MCP workflow regression tests** (`hv2-agent`): `tests/mcp_workflow.rs` locks
  in the schema↔handler parameter contract for the common-workload tools.
- **Guest-command round-trip** (`hv2-agent`): new `guest.exec.status` MCP tool
  and `AgentSession::deliver_guest_response` API complete the `guest.exec` /
  `guest.file.*` loop — an agent submits a command, the guest-agent channel
  delivers the result, and the agent polls it to completion. Covered by a
  regression test and shown in `agent_mcp_workflow`.
- **Recursive planner** (`hv2-agent`): the GOAP planner now performs real
  recursive backward chaining with backtracking to satisfy action
  preconditions, replacing the previous single-level stub.

### Changed
- **Crypto enabled by default** (`hv2-core`): `default = ["ring", "pqc"]`, so
  AES-256-GCM/SHA use the validated `ring` backend out of the box. The insecure
  software AES-GCM fallback is removed — without `ring` the operations return
  `NotImplemented` instead of weak crypto.
- **README/docs**: reconciled crypto claims — "FIPS-approved algorithm
  implementations, not a FIPS 140-3 validated module"; AES-256-GCM only; added
  an ECDSA P-521 caveat; PQC described as real and RustCrypto-backed.

### Fixed
- **KVM/HVF FFI doc-comment corruption** (`hv2-core`): repaired control bytes
  (VT/FF/ESC/BEL) and a split `///` comment in the Linux (`kvm_ffi.rs`) and
  macOS (`hvf.rs`) backends that produced module-scope syntax errors — invisible
  on Windows (cfg-gated) but breaking Linux/macOS compilation and `cargo fmt`.
- **Clippy**: resolved lints surfaced by enabling `ring`/`pqc` and by the
  toolchain bump (`needless_return`, `collapsible_match`, `unnecessary_sort_by`,
  `manual_checked_ops`); workspace is `clippy -D warnings` and `cargo fmt`
  clean. Aligned `clippy.toml` MSRV to 1.95.
- **MCP tool schema/handler mismatch** (`hv2-agent`): the published tool schemas
  disagreed with the dispatcher, so agents following the advertised contract got
  wrong results — `vm.create` ignored `cpu_cores`/`memory_gb` (silently creating
  a 1-CPU/512 MB VM), and VM/snapshot/network tools advertised `vm_name`/
  `snapshot_name`/`network_name`/`interface_name` while the handler read
  `vm_id`/`snapshot_id`/`network`/`interface_id`. Schemas and handlers are now
  consistent and covered by regression tests.
- **Cross-platform build & test** (workspace): the `cfg(target_os = "linux")`
  paths — VFIO passthrough, the TAP network device, and the KVM FFI — now build
  and lint clean on Linux (previously: undeclared `libc` deps, an escaping
  borrow in `enumerate_gpus`, unconditional WHPX imports, and `derive(Default)`
  on >32-element arrays, all invisible to the Windows dev host). Backend-
  dependent tests skip gracefully where `/dev/kvm` is unavailable, so the full
  suite is green on Windows, Linux, and macOS.

### Performance
- Measured with Criterion on an AMD Ryzen 9 9900X (`cargo bench`): **O(1) agent
  spawn** — a copy-on-write clone is ~9 ns regardless of fleet size (constant at
  1/16/64 baseline units) versus a full copy at 206 µs–9.3 ms; CoW first-write
  fault 73 ns; guest-memory read/write 27–96 ns / 9–19 ns; MCP tool dispatch
  547 ns (47 µs for 64 concurrent); snapshot 18–21 ns; AES-256-GCM ~9–10 GiB/s
  (AES-NI). See the README Benchmarks table.

### Security
- Triaged RUSTSEC-2023-0071 (the `rsa` crate's Marvin-attack timing
  side-channel) in `deny.toml` with justification; no fixed upstream release
  exists and HyperMachine does not expose RSA as an online decryption oracle.

## [1.0.0] - 2026-03-25

### Added
- **TLS/HTTPS support** (`hv2-api`): `TlsConfig`, `build_rustls_config()`, and
  `serve_tls()` for encrypted API transport using rustls.
- **Permission middleware**: Graph-based permission middleware wired into the API
  server with resource scope hierarchy and role-based access control.
- **Fuzz testing targets**: 7 `cargo-fuzz` targets covering API parsing, VM config,
  agent messages, CPU instruction decoding, memory operations, PCI config, and
  interrupt controller state.
- **Integration tests**: 15 tests for `hv2-agent`, 31 for `hv2-cpu`, 15 headless
  CI tests for `hm-gui`, and 15 end-to-end stack tests for `hv2-api`.
- **End-to-end stack tests** (`hv2-api`): Full-router tests exercising health checks,
  VM CRUD lifecycle, runtime sessions, workload scheduling, Prometheus metrics,
  agentic ontology (JSON-LD), AI tool formats, feature toggling, and snapshots.
- **Feature-gated GPU** (`hv2-gpu`): `wgpu-backend` and `vulkan-backend` features
  with optional dependencies so the crate compiles with `--no-default-features`.

### Changed
- **Axum 0.7 → 0.8 upgrade**: Bumped `axum` workspace dependency from 0.7 to 0.8,
  migrated WebSocket `Message::Text` to `Utf8Bytes`, updated all route path parameters
  from `:param` syntax to `{param}` syntax across 7 source files.
- **Version bump**: Workspace version `0.1.0` → `1.0.0` across all 13 crates.
- **Documentation link**: Updated `documentation` field in `Cargo.toml` from invalid
  `docs.rs/hypermachine` to GitHub docs directory.

### Fixed
- **Axum route parameter syntax**: All VM CRUD routes in `rest.rs` and ontology routes
  in `ontology.rs` were using `{id}` (Axum 0.8 syntax) while depending on Axum 0.7,
  causing all parameterized routes to return 404. Fixed by upgrading to Axum 0.8.
- **Unsafe audit**: Added `SAFETY` comments to 33 unsafe blocks across 7 files
  (`kvm.rs`, `interrupt.rs`, `memory.rs`, `serial.rs`, `svm.rs`, `vmx.rs`, `vm.rs`).

## [0.3.0] - 2026-03-24

### Added
- **Graph-based hierarchical permissions**: DAG-structured permission system for
  agentic AI systems with resource scope hierarchy (Root → Org → Tenant → Project → VM),
  role inheritance with cycle detection, controlled delegation with attenuation and
  depth limits, permission resolution engine, and append-only audit trail.
  23 unit tests covering the full permission lifecycle.
- **Unikernel lifecycle integration tests**: 19 tests covering boot protocols, guest
  memory management, and pool operations.
- **Expanded test coverage**: +32 tests across hm-cli, hv2-cpu, hv2-net, and hv2-api.

### Changed
- Removed internal business strategy and session planning documents from public
  repository (moved to .gitignore).
- Removed duplicate docs/GETTING_STARTED.md (root copy is canonical).
- Bumped Containerfile Rust version to 1.87.
- Replaced invalid crates.io category `"os"` with `"emulators"`.

### Fixed
- Resolved all remaining clippy warnings across the workspace.

## [0.2.0] - 2025-07-13

### Fixed
- **PIC 8259 cascade deadlock**: `acknowledge_interrupt()` and `send_eoi()` held master
  lock while re-acquiring it in slave path, deadlocking on `parking_lot::Mutex`. Added
  `drop(master)` before slave branch in both methods.
- **E1000 DMA undefined behavior**: `process_rx_ring_dma()` and `process_tx_ring_dma()`
  took `&[u8]` but wrote through immutable references via raw pointer casts (UB at
  opt-level=1). Changed to `&mut [u8]` with safe `copy_from_slice`.
- **E1000 RX queue drain**: `process_rx_queue()` consumed packets without DMA writeback
  when ring was configured, starving `process_rx_ring_dma()`.
- **IOAPIC EOI re-delivery**: `end_of_interrupt()` now checks `irq_state` bitmap after
  clearing `remote_irr` to re-assert level-triggered interrupts still active.

### Added
- **MCP guest agent operations**: `execute_command`, `read_file`, `write_file`,
  `list_processes` on the MCP guest agent interface.
- **E1000 DMA**: Full RX/TX DMA with guest memory read/write for the Intel 82540EM NIC.
- **VirtIO GPU**: Capset dispatch (Virgl, Virgl2, Venus, Cross-Domain) and
  `transfer_to_host_2d` with guest memory DMA.
- **xHCI USB**: Transfer ring processing (Normal, Setup, Data, Status TRBs).
- **Intel HDA**: CORB verb dispatch (get/set parameters, power, pin config, stream format,
  AMP gain, connections).
- **RSA crypto**: Software modular exponentiation for encrypt/decrypt.
- **Post-quantum crypto**: ML-DSA (Dilithium) and SLH-DSA (SPHINCS+) signature
  verification with hash-based schemes.
- **FIPS AES-GCM fallback**: Hand-rolled GHASH + CTR mode when hardware AES-NI unavailable.
- **TAP loopback mode**: Memory buffer mode for testing without OS TAP devices.
- **DurableStore External backend**: HTTP-based storage with retry and circuit breaker.
- **Linux boot helper**: `boot_with_mapper()` loads kernel, initrd, and cmdline via
  address-space mapper.
- **ACPI DSDT enhancements**: `_CRS` resource blocks, `CPU0` device scope, `\_S5_` sleep
  state object in AML bytecode.
- **PIC cascade tests**: `test_slave_pic_cascade` (fixed, formerly ignored) and
  `test_slave_pic_multiple_irqs` for IRQs 8/10/12/15 through cascade.

## [0.1.0] - 2025-01-01

### Added

- **Type-2 hosted hypervisor** (`hv2-core`) with vCPU, memory, and device management
  - KVM (Linux), WHPX (Windows), and HVF (macOS) backend support
  - Zero-copy memory mapping with `memmap2`
  - Full device model: serial console, block storage, RTC, PCI bus, interrupt controller
  - FIPS 140-3 compliant cryptography (AES-256-GCM, SHA-256/512, ECDSA, RSA)
  - JIT compilation engine for dynamic code
  - Snapshot and restore for VM state
- **Type-1 bare-metal hypervisor** (`hv1-core`, `hv1-boot`) with UEFI bootloader
- **CPU virtualization** (`hv2-cpu`) with x86_64, ARM64, and RISC-V instruction decoding
- **GPU virtualization** (`hv2-gpu`) with Vulkan/WebGPU, passthrough, and virtual GPU
- **Networking** (`hv2-net`) with full TCP/IP stack, TAP/TUN, virtio-net, and DHCP
- **AI agent interface** (`hv2-agent`) with MCP server, WASM plugin runtime, and scripting API
  - OpenAI/Claude/Gemini tool format support
  - Agent lifecycle management and learning framework
- **REST/gRPC API server** (`hv2-api`) with WebSocket streaming
- **CLI tool** (`hm-cli`) with integrated MCP server and VM management commands
- **Desktop GUI** (`hm-gui`) with virt-manager style interface and AI automation API
- **Deployment infrastructure**: Helm chart, Kubernetes manifests, Terraform configs
- **CI/CD**: Build, test, security audit, coverage, benchmarks, release, and deploy workflows
- **Security**: `cargo-deny` configuration, Dependabot, seccomp filtering, capability-based access

[Unreleased]: https://github.com/nervosys/HyperMachine/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/nervosys/HyperMachine/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/nervosys/HyperMachine/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nervosys/HyperMachine/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nervosys/HyperMachine/releases/tag/v0.1.0
