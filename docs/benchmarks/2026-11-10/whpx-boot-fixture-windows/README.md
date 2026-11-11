# A reproducible kernel+initrd fixture for `linux_whpx_boot.rs`, built in WSL

`crates/hv2-core/tests/linux_whpx_boot.rs` has always taken its guest image from
environment variables (`HV2_TEST_KERNEL`, `HV2_TEST_INITRD`, ...); nothing in the repo
built one, so the suite silently skipped on every host, including this one. The repo
already ships the recipe (`tools/guest-image/build-kernel.sh`, `tools/guest-image/build.sh`);
it had just never been run here. This fixture closes that gap locally. It is **not**
wired into CI (see "Not done" below) and the built kernel/initrd are not committed or
published — both decisions are the owner's to make, not mine.

## Recipe (WSL2, Ubuntu)

Packages needed beyond a stock WSL Ubuntu image:

```
apt-get install -y bc flex bison libelf-dev busybox-static file cpio
```

Kernel (Linux 6.6.52 via the repo's own `hv2-guest.defconfig`):

```
tools/guest-image/build-kernel.sh -o /path/to/bzImage
```

Initramfs (busybox + a statically-linked `hv2-guest-agentd`):

```
RUSTFLAGS='-C target-feature=+crt-static' cargo build --release \
  --target x86_64-unknown-linux-gnu -p hv2-guest-agentd
tools/guest-image/build.sh -o /path/to/initrd.cpio.gz
```

Both scripts ran unmodified. The only failures were missing WSL packages (`file` and
`cpio` are not in a stock Ubuntu WSL image; `build.sh`'s own static-binary check and its
`cpio` archiving step depend on them and fail silently/loudly without them).

A login shell is required to run cargo/rustc as root in WSL (`wsl -- bash -lc "..."`,
not `bash -c`) — a non-login shell does not source the profile where rustup's `PATH`
export lives, so `rustc`/`cargo` report "command not found" even though they are
installed.

## Result: 15 of 17 tests pass against this fixture

Env vars used:

```
HV2_TEST_KERNEL=.../bzImage
HV2_TEST_INITRD=.../initrd.cpio.gz
HV2_TEST_INITRD_MARKER="=== HV2 READY ==="
HV2_TEST_INITRD_SHELL=1
HV2_TEST_SEED=.../seed.tar          # contains seed.txt = "HV2_SEED_MARKER_CONTENT"
HV2_TEST_SEED_MARKER=HV2_SEED_MARKER_CONTENT
HV2_TEST_DATA=.../data.img          # blank 16 MiB file
HV2_REQUIRE_GUEST=1
```

Run serially (`--test-threads=1`) with `HV2_REQUIRE_GUEST=1` set, which turns a
fixture-not-found skip into a hard failure rather than a silent pass — a first parallel
run without it reported "17 passed" but most of those were WHPX partition-creation
failures under contention (`HRESULT 0xC0370008`, "Failed to map guest memory at 0x0")
being swallowed as skips, not real boots.

```
test result: FAILED. 15 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

The 15 that pass cover: multi-vCPU boot (2/4/8 vCPUs), parallel busy loops, PCI data-disk
read/write, snapshot/fork/restore (including mid-boot and 2/4/8-vCPU variants), paused-vCPU
state round-trip, and the userspace-PIC backend boot path.

The 2 that fail are both named `a_root_filesystem_mounts_from_a_virtio_blk_disk_on_pci*`
(one and two vCPUs). Both time out at the suite's 120 s `BOOT_DEADLINE` because the test
unconditionally asserts the kernel log contains `EXT4-fs (vda1): mounted filesystem` —
reading the test source (`root_from_pci_disk`, around line 342–446), that assertion has
no `initrd.is_some() ||` guard the way neighboring assertions do. Neither a blank
`data.img` nor this fixture's `init` script (which never mounts `/dev/vda1`) can produce
that line. A real ext4-formatted root-disk image, plus an `init` that mounts it and emits
a matching log line, would be a separate, larger fixture — not built here.

## Not done

- **Not wired into CI.** GitHub-hosted Windows runners are Azure VMs without nested
  virtualization passthrough, so they likely cannot create a WHPX partition at all; this
  was not tested against an actual runner, and no `.github/workflows` file was touched.
  This is a deliberate choice, not an oversight.
- **Kernel/initrd are not committed or published.** They are GPL-licensed build output;
  hosting them is the owner's call, same as the prior status entry said.
- **The ext4 root-disk fixture** for the 2 remaining tests is not built.
