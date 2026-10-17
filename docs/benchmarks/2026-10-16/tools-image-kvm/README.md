# A sandbox template with the development tools in it, on KVM (2026-10-16)

[`tools/check-tools-image-kvm.py`](../../../../tools/check-tools-image-kvm.py)
ran an owned `hv2-sandboxd` whose `base` template is the image
[`tools/guest-image/build-tools-image.sh`](../../../../tools/guest-image/build-tools-image.sh)
builds: Ubuntu 24.04 with git 2.43.0, Python 3.12.3, Node.js 18.19.1, npm 9.2.0,
gcc 13.3.0, make 4.3, curl 8.5.0 and jq 1.7. See
[the guide](../../../TOOLS_IMAGE.md). Host: WSL2, Linux 6.18.33 with nested
KVM, an AMD Ryzen 9 9900X; the guest kernel is `bzImage-known-uart-irq`; the
daemon is a release build made earlier the same day, whose hash is in
`report.json`. The daemon ran with `--network --egress-default allow`, so
the guest reached the Internet through the node's egress gateway.

## Result: 5 cases, 3 runs in a row, all passed (1536 MiB, with the network)

1. **The guest is Ubuntu 24.04 and every tool answers.**
2. **Work offline, nothing installed:** `python3` summed 0..100 and printed
   JSON; `gcc -O2` built a C program that printed 42; `node --test` ran two
   assertions (2 tests, 2 pass, 0 fail); `git init`, `add`, `commit` gave one
   commit.
3. **From inside, with the network:** `curl` over HTTPS to
   `archive.ubuntu.com` answered 200; `git clone --depth 1` of
   `octocat/Hello-World` over HTTPS gave one commit and a README; `npm
   install is-odd@3.0.1` added 2 packages in 1 to 2 s and `require` of it
   returned true; `apt-get update` and `apt-get install tree` installed
   `tree` 2.1.1; and `/root` and `/home` could be written.
4. **A fork carries the files and the tools.**
5. **Cost.**

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| first create | 0.022 s | 0.029 s | 0.043 s |
| second create | 0.019 s | 0.051 s | 0.032 s |
| `MemAvailable` in the guest, after all of the above | 527,816 kB | 527,576 kB | 527,216 kB |
| root filesystem after them (1K blocks, used, available) | 1,358,164, 915,960, 442,204 | same | same |

The create times are one stopwatch reading each, not a benchmark. The image
is 187 MB compressed. `MemTotal` was 1,509,068 kB.

## What the network run found

The first network run failed at `apt`, and the cause was in the image, not
the network: **the guest's root is a tmpfs the kernel sizes below the image's
own size, so every write outside `/tmp` failed with "No space left on
device".** Inside the guest, `mount` showed `rootfs ... size=660876k` for an
unpacked image of 660,876 kB. Every earlier case wrote only to `/tmp`, so
none of them noticed. Remounting with a larger size worked (`/root` writable,
`apt-get update` fetched 28.6 MB), so
[`tools/guest-image/init`](../../../../tools/guest-image/init) now runs
`mount -o remount,size=90% /`. The minimal base image was booted with it:
created in 0.04 s, `/` capped at 90% of its memory, `/root` writable. Two
smaller defects of the build script were found the same way and fixed: an
emptied `/etc/resolv.conf` (init links it to the kernel's nameserver file
only when there is none, so DNS failed) and the removed `partial/`
directories `apt` needs.

## Memory it needs

The kernel's cap on the root, before `init` raises it, was 660,876 kB of
1,509,068 kB (44%) at 1536 MiB. The image needs 576 MB of it just to unpack.

| Guest memory | Result |
|---|---|
| 1536 MiB | all five cases passed, three runs (the table above) |
| 2048 MiB | all four offline cases passed, once, on the first build of the image |
| **1024 MiB** | **failed:** the guest booted but `npm` was not found, so the image had not unpacked in full. The daemon's log said nothing about it; the guest's console was not read. That the cap is the cause is an inference from the sizes, not something observed |

## Not verified

- **A real build-and-test loop**, such as the 1,000-run one in the parity
  table: the Node.js test is two assertions.
- **`--template NAME=...`**: the image was given to the daemon as
  `HV2_INITRD`, the template `base`.
- **A machine image with tools baked in**, **Docker**, **a browser**, **other
  languages**: none exist here.
- **Sized templates built from this image**, and **a size between 1024 and
  1536 MiB** (1280, say, was not tried).
- **Another host, kernel or network.** One host and one guest kernel. The
  network cases reached `archive.ubuntu.com`, `github.com` and the npm
  registry on the day, so they depend on those being up.
- **The 2048 and 1024 MiB runs and the first three 1536 MiB runs** used
  images built before the `init`, resolver and `apt` fixes (`runs/` files
  without `network` in the name); they are kept as they were.
- **Reproducibility:** builds from the same recipe gave different files.
- **The new `init` on other guests.** It was booted with the minimal base
  image and this one. With the minimal image,
  `tools/check-sized-templates-kvm.py` (restore, fork, resize, pause and the
  size limit) passed all five cases once; standby and the warm pool were
  not rerun with it.
