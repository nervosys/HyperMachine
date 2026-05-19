# Standard guest image

The harness needs ONE base image shared by every hypervisor so comparisons
isolate the hypervisor, not the guest. Build it once and check it in only
locally — it is too large for the repository.

## Recommended: Debian 12 cloud image + cloud-init

`build-guest.sh` produces `build/debian-12-bench.qcow2` with:

- bench user with passwordless sudo + your SSH key (generated to `build/id_ed25519`)
- pre-installed: `sysbench`, `fio`, `iperf3`, `netperf`, `build-essential`, `curl`
- sshd on port 22, password auth disabled
- qemu-guest-agent + cloud-init disabled at first boot (so cold-boot time is
  measured against a steady-state init, not provisioning)

Output paths consumed by the harness:

```
build/
├── debian-12-bench.qcow2       # default for QEMU/KVM, VirtualBox, Hyper-V (after convert)
├── ch-vmlinux                  # uncompressed kernel for Cloud Hypervisor
├── firecracker-vmlinux         # uncompressed kernel for Firecracker
├── firecracker-rootfs.ext4     # flat ext4 rootfs for Firecracker
└── id_ed25519                  # private key the harness uses for SSH
```

## Building

Requires Linux host with: `qemu-img`, `qemu-system-x86_64`, `cloud-localds`
(`cloud-image-utils` on Debian/Ubuntu), `curl`, `e2fsprogs`, optionally
`virt-customize` (`libguestfs-tools`).

```bash
cd benchmarks
./images/build-guest.sh
```

The script:

1. Downloads the Debian 12 generic cloud image.
2. Generates an SSH keypair under `build/`.
3. Runs `virt-customize` (or boots once with cloud-init seed) to install
   benchmark tools and inject the SSH key.
4. Extracts the kernel for Firecracker / Cloud Hypervisor and produces a flat
   ext4 rootfs by mounting the qcow2 and copying.

## Manual alternatives

If you have an existing test image you'd rather use, set `[guest].image` in
`config.toml` to its path and ensure it satisfies:

- Listens on TCP/22 with the SSH public key in `~bench/.ssh/authorized_keys`
- Has `sysbench`, `fio`, `iperf3`, `netperf`, `cc`, `make` on PATH for the bench user
- Allows the bench user to `sudo` without a password (kernel build workload uses it)
