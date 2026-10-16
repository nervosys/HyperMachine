# A sandbox template with the development tools in it, on KVM (2026-10-16)

[`tools/check-tools-image-kvm.py`](../../../../tools/check-tools-image-kvm.py)
ran an owned `hv2-sandboxd` whose `base` template is the image
[`tools/guest-image/build-tools-image.sh`](../../../../tools/guest-image/build-tools-image.sh)
builds: Ubuntu 24.04 with git 2.43.0, Python 3.12.3, Node.js 18.19.1, npm 9.2.0,
gcc 13.3.0, make 4.3, curl 8.5.0 and jq 1.7. See
[the guide](../../../TOOLS_IMAGE.md). Host: WSL2, Linux 6.18.33 with nested
KVM, an AMD Ryzen 9 9900X; the guest kernel is `bzImage-known-uart-irq`; the
daemon is a release build made earlier the same day, whose hash is in
`report.json`.

## Result: 4 cases, 3 runs in a row, all passed (1536 MiB)

1. **The guest is Ubuntu 24.04 and every tool answers.**
2. **Work with no network and nothing installed:** `python3` summed 0..100 and
   printed JSON; `gcc -O2` built a C program that printed 42; `node --test`
   ran two assertions (2 tests, 2 pass, 0 fail); `git init`, `add`, `commit`
   gave one commit.
3. **A fork carries the files and the tools:** a marker file, the git
   repository (one commit) and `node --version` were all there in the fork.
4. **Cost.**

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| first create | 0.059 s | 0.066 s | 0.108 s |
| second create | 0.050 s | 0.031 s | 0.093 s |
| `MemTotal` / `MemAvailable` in the guest | 1,509,068 / 789,948 kB | 1,509,068 / 788,412 kB | 1,509,068 / 790,540 kB |

The create times are one stopwatch reading each, not a benchmark. The image
is 187 MB compressed.

## Memory it needs

| Guest memory | Result |
|---|---|
| 2048 MiB | all four cases passed (once; on the first build of the image). 2,025,164 kB total, 1,299,088 kB available |
| 1536 MiB | all four cases passed, three runs |
| **1024 MiB** | **failed:** the guest booted but `npm` was not found, so the image had not unpacked in full. The daemon's log said nothing about it; the guest's console was not read |

## Not verified

- **The network from inside** (`apt install`, `npm install`, `git clone`): the
  check is offline on purpose.
- **A real build-and-test loop**, such as the 1,000-run one in the parity
  table: the Node.js test is two assertions.
- **`--template NAME=...`**: the image was given to the daemon as
  `HV2_INITRD`, the template `base`.
- **A machine image with tools baked in**, **Docker**, **a browser**, **other
  languages**: none exist here.
- **Sized templates built from this image**, and **a size under 1536 MiB
  other than 1024** (1280, say, was not tried).
- **Another host or kernel.** One host, one guest kernel.
- **Reproducibility:** two builds from the same recipe gave different files
  (different hashes); the archive moves.
- The 2048 MiB and 1024 MiB runs used the first build; the 1536 MiB runs
  used the image from the committed script.
