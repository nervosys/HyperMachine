# /dev/kvm inside a sandbox, on KVM (2026-10-23)

`tools/check-kvm-sandbox-kvm.py` on WSL2's KVM (an AMD host, running as root): an
owned `hv2-sandboxd` on a Linux 6.6.52 kernel from `build-kernel.sh --docker --kvm`
and a systemd template with python3 (`build-tools-image.sh --systemd --packages
"ca-certificates procps python3"`), at 1536 MiB. 2 of 2 cases pass in each of 3 runs
in a row (`runs/run1..3.json`; `report.json` is run 1).

1. `/dev/kvm` exists in the sandbox and `/proc/cpuinfo` shows `svm` on both vCPUs.
2. A Python program in the guest opens `/dev/kvm`, reads the API version (12),
   creates a VM and a vCPU, and runs a real-mode program (`mov al,0x42; out 0x10,al;
   hlt`) to a `KVM_EXIT_IO` on port 0x10 with byte 0x42 and then `KVM_EXIT_HLT`.

The same program as the machine check
(`../machine-ubuntu-snapshot-kvm/README.md`); the sandbox's hypervisor passes the
host's SVM through as it does for a machine.

## NOT VERIFIED

An Intel host (VMX), a guest of its own inside the sandbox (only a toy real-mode
program ran), a checkpoint or fork of a sandbox while it has a VM running, the
default kernel (which has no KVM; `--kvm` is a separate build), and Docker with
`--device /dev/kvm`.
