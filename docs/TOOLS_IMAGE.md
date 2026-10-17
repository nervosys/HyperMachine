# A template with the development tools in it

A sandbox boots from a template, and the template HyperMachine ships holds
little beyond the guest agent. This is a second kind: Ubuntu 24.04 with the
tools a coding agent reaches for already installed, so a sandbox can run
`git`, `python3`, `node`, `npm`, `gcc` and `make` at once, with nothing to
install first. With the network on, `git clone`, `npm install` and `apt
install` work from inside it too.

| Tool | Version in the image checked |
|---|---|
| Ubuntu | 24.04 (noble) |
| git | 2.43.0 |
| Python | 3.12.3 |
| Node.js / npm | 18.19.1 / 9.2.0 |
| gcc / make | 13.3.0 / 4.3 |
| curl, jq, less, ps, ssh | the distribution's |

Nothing is shipped as a binary. You build it, from Ubuntu's archive, with
[`tools/guest-image/build-tools-image.sh`](../tools/guest-image/build-tools-image.sh).

## Building it

```sh
sudo tools/guest-image/build-tools-image.sh -o tools.cpio.gz \
    --busybox /path/to/static/busybox \
    --agent /path/to/static/hv2-guest-agentd
```

It needs root (for `chroot` and `mount`), `debootstrap` and `ubuntu-keyring`,
and network. It uses no Docker and no container runtime, and installs nothing
on the host. `debootstrap` makes a minimal Ubuntu root filesystem, `apt`
installs the packages into it in a chroot (`--packages` changes the list),
the documentation and `apt`'s lists are removed, and
[`build.sh`](../tools/guest-image/build.sh) packs it with the agent and init.
The result was 654 MiB of files, 576 MB unpacked and 179 MB compressed.

The image is not byte-for-byte reproducible: it is whatever the archive held
the day it was built. Record the archive date, or keep the file.

## Using it

```sh
HV2_INITRD=tools.cpio.gz hv2-sandboxd --memory-mb 1536 ...
```

That is how the check ran it. The daemon's `--template NAME=INITRAMFS` flag
(`--template tools=tools.cpio.gz`, then `templateID: "tools"` at create) keeps
the small template beside it; **that form was not run with this image.**

For the Internet, start the node with `--network` and an egress policy that
allows it (`--egress-default allow`, or per sandbox `allowInternetAccess`).

### How much memory

**At least 1536 MiB for the guest.** A sandbox's root filesystem is in
memory, and this one is 576 MB before anything runs. The kernel caps that
filesystem at about 44% of the guest's memory (660,876 kB of 1,509,068 kB at
1536 MiB), and the guest's `init` raises the cap to 90% once it is unpacked.
At **1024 MiB the guest still boots and the unpacking stops short: `npm` was
missing, and the daemon's log says nothing about it** (the guest's own
console was not read; that the cap is the cause is inferred from the sizes).
At 1536 MiB the guest sees 1.5 GB; after the whole network check, about
530 MB was still available and the root held 916 MB of 1.36 GB. If you build
a larger image, test its smallest size the same way: a tool missing from
`PATH` is how too little memory shows. Installing more with `apt` inside a
sandbox spends the same memory.

## What was checked

On real KVM in WSL, with
[`tools/check-tools-image-kvm.py`](../tools/check-tools-image-kvm.py) (results
in [the evidence](benchmarks/2026-10-16/tools-image-kvm/README.md)):

- the guest is Ubuntu 24.04, and each tool answers `--version`;
- with no network: a Python program, a C program compiled and run, a Node.js
  test run with `node --test`, and a git repository with a commit;
- with the network: `curl` over HTTPS, `git clone` of a public repository,
  `npm install` of a package and `require` of it, `apt-get update` and
  `apt-get install tree`, and files written under `/root` and `/srv`;
- a fork carries the files and the tools;
- the create and free-memory figures above.

Three runs in a row passed. The network cases need the Internet, and
`archive.ubuntu.com`, `github.com` and the npm registry, to be up.

## Not checked

- **The parity table's 1,000-run loop.** The checks' Node.js test is two
  assertions. A heavier workload, V8's Web Tooling Benchmark, ran in a 4 vCPU,
  8 GiB sandbox from this image: [median 12.56 runs/s on a busy host](benchmarks/2026-10-16/node-web-tooling-kvm/README.md).
  That is a different benchmark from the table's loop.
- **Machines.** This is a sandbox template. A machine boots a distribution's
  disk image and can [install tools at first boot](MACHINES.md#agents-installed-at-first-boot);
  no machine image with tools baked in was built.
- **Docker, Chrome, VS Code, other languages and the agent programs** that
  boat lists are not in it.
- **Sizes other than the template's.** [Sized templates](SANDBOX_SIZES.md)
  rebuild a template at another size; with an image this large they are
  bounded by `--sized-template-disk-gb`, and none was built from it.
- **Any other host or kernel** than the one in the evidence.
