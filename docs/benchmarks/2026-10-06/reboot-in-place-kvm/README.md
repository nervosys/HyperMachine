# Sandboxes rebooted in place on real KVM

**Before:** a guest that ran `reboot` hung. On the hardware-reduced ACPI platform MMIO
guests boot on (since #144), Linux reboots through EFI by default (`/sys/kernel/reboot/type`
read `efi`). With no EFI it falls through to a real-mode jump into a BIOS that is not there,
and spins. A host trace of the hung guest showed only interrupt exits, at `rip`
`0x35c6`–`0x35da`. Even when a guest did stop, nothing in `hv2-sandboxd` noticed: the
sandbox listed as `running` while every exec timed out, until its lifetime ran out.

**Change:**

- `hv2-core` boots hardware-reduced guests with `reboot=k` and answers the i8042 reset
  pulse (0xFE to port 0x64) by stopping the VM (`VM::i8042_reset_as_shutdown`), as
  Firecracker does.
- Sandbox guests get `panic=1`, so a kernel panic resets instead of hanging.
- `hv2-sandboxd` (`src/reboot.rs`) reboots a sandbox whose VM has stopped, found by the
  expiry loop within a second, or on `POST /sandboxes/{id}/reboot`. The sandbox keeps its
  ID, access token, URL, network policy, volumes, disk and `envVars`. After five reboots
  within 60 s it is ended as `sandbox-lost`.

Guide: [`docs/REBOOT.md`](../../../REBOOT.md).

## Method

`tools/check-reboot-kvm.py` runs an owned `hv2-sandboxd` with a template. Each step needs
the one before it:

1. A sandbox with `envVars` serves a page from a guest `busybox httpd` through the proxy at
   `8080-{sandboxID}`. It then runs `reboot -f` with a marker file in its RAM root. The
   check polls until a guest without the marker answers, then compares the access token
   and reads the variable.
2. The proxy URL answers 502 until the fresh guest starts a server, and then serves the new
   page.
3. A kernel panic (`echo c > /proc/sysrq-trigger`) comes back the same way.
4. A sandbox holding a [disk](../../../DISKS.md) writes 1 MiB, runs `reboot -f`, and reads
   the same SHA-256 back from the same mount. The disk is still attached to it.
5. `POST /sandboxes/{id}/reboot` replaces a running guest.
6. Further requested reboots are counted against the limit of five: the next two answer
   200, the one after that 409, and the sandbox is gone.
7. `hv2_node_transitions_total{kind="reboot"}` reads 6.

## Result: all seven pass

The full record is in [`report.json`](report.json). The daemon log shows three i8042 reset
pulses, three guest-initiated shutdowns and three reboots by the expiry loop: the two
self-reboots and the panic.

| Step | Observed |
|---|---|
| Same access token after reboot | yes |
| `envVars` after reboot and after panic | `kept-across-reboot` |
| Proxy URL: before, while the server is down, after | `before-reboot`, 502, `after-reboot` |
| Disk after reboot | `/dev/vda /data ext4`, SHA-256 `795e8802…74b8028f` both sides |
| Requested reboot, round trip | 0.02 s (template restore) |
| Reboot answers past the window's first three | 200, 200, 409; then 404 for the sandbox |

The checker reports `back_after_seconds` of about 6.3 s for the self-initiated cases.
That figure is mostly the checker's own 5 s exec timeout against the guest that has
gone; it is not a recovery-latency measurement. The daemon log shows the expiry loop
noticing each stop 0.4–0.9 s after the guest's reset pulse (the loop runs once a second).
The reboot's own duration goes to the sandbox's log, which this run did not capture.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release, branch `feat/reboot-in-place`) | `8a33652da0f939591733e7c792de2599f3b836cffecdcd0859b6dbaa3980fe21` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |

The host was WSL2 kernel 6.18.33.2 with real KVM.

## Not shown here

- Native public TCP/UDP ports (the control plane's managed ports) were not exercised. They
  are keyed by sandbox ID, which a reboot keeps, but no traffic through them was checked.
- PCI-transport guests were not rebooted here. They do not use hardware-reduced ACPI, so
  Linux takes its ACPI-then-keyboard path, which reaches the same i8042 handling.
- A sandbox resumed on another node, or after its daemon restarted, reboots without its
  `envVars`: the host keeps them in memory only.
