# Docker together with /dev/kvm in a sandbox, on KVM (2026-10-24)

`tools/check-docker-sandbox-kvm.py --network --nested` on WSL2's KVM, as root: an
owned `hv2-sandboxd` whose `base` template is the initramfs from
`build-tools-image.sh --docker docker-29.9.2.tgz --compose docker-compose-linux-x86_64
--packages "ca-certificates procps python3"` (155 MiB) on a Linux 6.6.52 kernel from
`build-kernel.sh --docker --kvm` (`resolved.config` is its full configuration), at
2048 MiB. 13 of 13 cases pass in each of 3 runs in a row (`runs/run1..3.json`;
`report.json` is run 1). Cases 1 to 12 are those of
[the Docker check](../2026-10-23/docker-sandbox-kvm/README.md); the thirteenth is new:

13. A nested VM runs to a `KVM_EXIT_IO` (port 0x10, byte 0x42) and then `KVM_EXIT_HLT`,
    both from the guest's own shell and from a container started with
    `--device /dev/kvm`, with the guest's `/usr`, `/lib` and `/lib64` bind-mounted
    read-only so the container can run the guest's python3.

So the one kernel carries Docker and KVM together, and a container can be given
`/dev/kvm`. Docker was the engine inside the guest; nothing on the host used it.

## NOT VERIFIED

Intel hosts (this is an AMD host under WSL2), a real guest OS inside the nested VM (a
toy real-mode program ran), a checkpoint or fork while a nested VM is running, and a
container image that carries its own KVM user (QEMU, Firecracker).
