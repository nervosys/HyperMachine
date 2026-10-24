# A sandbox whose first process is systemd, on KVM (2026-10-23)

`tools/check-systemd-sandbox-kvm.py` on WSL2's KVM, running as root: an owned
`hv2-sandboxd` whose `base` template is the initramfs made by
`tools/guest-image/build-tools-image.sh --systemd --packages "ca-certificates procps"`
(Ubuntu 24.04, systemd 255.4, `tools/guest-image/init-systemd` as `/init`, 41 MB),
on the existing template kernel, at 1536 MiB. 5 of 5 cases pass in each of 3 runs
in a row (`runs/run1..3.json`; `report.json` is run 1).

1. PID 1 is `systemd`; `systemctl is-system-running` says `running` with no failed
   units; `exec` through the guest agent still works.
2. A unit written into the guest starts, runs its own process, is `active`, and its
   line reaches the journal.
3. A fork has systemd as PID 1 and the unit still active.
4. A checkpoint restores to systemd with the unit active, and a file written after
   the checkpoint is gone.
5. Cost: MemTotal 1509068 kB, MemAvailable about 1.34 GB; create 0.12 to 0.60 s
   (first) and 0.16 to 0.32 s (second); about 0.9 to 1.1 s after a create until
   systemd reports settled. These are the check's own stopwatch, not a benchmark.

The agent is a systemd unit (`hv2-agent.service`) so the template is snapshotted
after systemd has booted. Getty units are masked: with no udev, `serial-getty@ttyS0`
waits out systemd's 90 s for a device that never appears.

## NOT VERIFIED

Docker (the kernel's options were not checked), `/dev/kvm` in the guest, images
built by `hm sandbox vm build` from OCI images, other distributions, more than one
node, and a systemd image at sizes other than 1536 MiB.
