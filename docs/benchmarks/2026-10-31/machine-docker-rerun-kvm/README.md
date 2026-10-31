# Ubuntu as a machine with apt and Docker, re-run after the virtio queue-size change (KVM)

Re-runs the [2026-10-11 check](../../2026-10-11/machine-docker-kvm/README.md) with the same command
(`tools/check-machine-distro-kvm.py --user ubuntu --sudo --user-data FILE --network --disk-gib 10`) on an
`hv2-sandboxd` built from the tree with commit `c223a09e` (a queue whose size the driver never wrote uses
the device maximum). WSL2, nested KVM, Rust Hypervisor Firmware, the same Ubuntu 24.04 raw image.

## Result (1 run)

All nine cases printed `ok`, exit 0: boot to login, processors/systemd/ACPI, cloud-init seed, systemd
finished, DHCP and name resolution, `apt-get` installing Docker and its service starting, a container
pulled and run, `systemctl poweroff`, and start again with the disk's contents kept.
[`stdout.txt`](stdout.txt) is the run's output, [`report.json`](report.json) the per-case answers.

## Not verified

- One run. Timings were not compared with the 2026-10-11 run.
- The snapshot-route Ubuntu check (`check-machine-snapshot-ubuntu-kvm.py`, needs host root and loop
  devices) was not re-run after the change.
