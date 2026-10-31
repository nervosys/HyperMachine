# OpenBSD 8.0 installed to disk and booted, with `pf` running (KVM)

Follows [the boot spike](../openbsd-boot-spike/README.md). An owned `hv2-sandboxd` (WSL2, nested KVM,
the Rust Hypervisor Firmware), no Docker. Requested by SentryWall's session, whose `pf` code had never
run on a real pf system.

## What was done

1. `install.py` boots the miniroot (from the spike: `bsd.rd` and a `boot.conf` for the serial console) as
   a machine with a 4 GiB disk and a NIC with the Internet allowed, and answers the installer's prompts
   in order over the serial console: DHCP on `vio0`, a throwaway root password, no `sshd`, console on
   `com0`, UTC, whole-disk GPT with the auto layout (it makes a 260 MB EFI system partition), sets over
   HTTP from `mirror.leaseweb.com`, with `bsd`, `bsd.rd` and `base80.tgz` only. It ended at
   `CONGRATULATIONS! Your OpenBSD install has been successfully completed!`, **335 s** after the first
   prompt (`install-steps.log`, `install-console.txt`), and halted. The installed disk was copied out.
2. `boot-installed-and-pf.sh` boots that disk as a machine from its own EFI system partition (no
   miniroot), logs in as root on the serial console, and runs `uname`, `pfctl -s info`, `pfctl -sr`,
   `ifconfig vio0` (`boot-console.txt`).

## Result (1 install, 1 boot of the installed disk)

- The installed system boots to `login:` on `com0` under this firmware. `OpenBSD 8.0 (GENERIC) #82`,
  `starting early daemons: syslogd pflogd ntpd`, `rc.firsttime` ran.
- **`pfctl -s info`: `Status: Enabled`**, 41 state-table entries, 48 inserts, 4057 searches, all error
  counters 0. **`pfctl -sr`** shows the default ruleset (`block return all`, `pass all flags S/SA`, the
  X11 and uid-55 blocks). So `pf` is in the kernel, enabled, and carrying the guest's own traffic.
- `vio0` is up with a DHCP address, `10.0.2.15/24`, on the machine's NIC behind the node's gateway.
- The base set (530 MB) downloaded over that NIC and was written to the virtio disk without a disk or
  network error.

This needed the virtio queue-size fix described in the spike's README (commit `c223a09e`). The `hv2-sandboxd` used was built in WSL from that tree.

## Not verified

- Repeatability: one install, one boot of the installed disk.
- Anything about `pf` beyond "enabled with the default ruleset": SentryWall's `PfController`, its
  baseline anchor and the table-name check have not been run. Getting its code onto the machine is the
  open part: there is no guest agent for OpenBSD, so files would come in over the serial console (small
  text only) or the NIC (the gateway refuses private addresses, so a public location), and the Rust
  binaries would need building on OpenBSD (no compiler set was installed).
- A guest reboot or `halt` leaves the machine stopped or running with the screen "halted"; this check
  did not test restarting the machine.
- `sshd` is off, no user besides root, no `syspatch` (the guest reported 5 pending patches).
- The signature of the downloaded sets was verified by the installer's own `signify` as part of the
  install (it would have stopped otherwise); this document did not check that independently.
- The disk is a fixture, not an image to distribute: root password `swtest-pw1`, host keys generated at
  first boot.
