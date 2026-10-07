# Block disks moved between real KVM sandboxes

**Change:** a file-backed virtio-blk device a guest can drive
(`crates/hv2-core/src/devices/virtio_blk_mmio.rs`, attached by `VM::attach_block` at
`0xd003_0000`, IRQ 7), and node-level disks in `hv2-sandboxd`
(`crates/hv2-sandboxd/src/disks.rs`): `POST/GET/DELETE /disks` and `"diskMount"` on
sandbox create. Guide: [`docs/DISKS.md`](../../../DISKS.md).

## Method

`tools/check-block-disk-kvm.py` runs an owned `hv2-sandboxd`, with a template so that
pause, fork and the other refused operations are reachable. Each step needs the one
before it to pass:

1. Create a 64 MiB disk; the image is sparse and ext4-formatted on the host.
2. A sandbox without a disk still restores from the template.
3. Sandbox A asks for the disk at `/data`, finds `/dev/vda` mounted there as ext4, and
   writes 4 MiB from `/dev/urandom`, then `sync`.
4. While A holds it, a second sandbox asking for the disk and a delete of the disk are
   both refused with 409.
5. Fork, snapshot, checkpoint and pause of A are refused with 409. A create asking for
   the disk with `autoPause` is refused with 400.
6. A is deleted, the disk shows free, sandbox B gets it, and the file's SHA-256 *in
   B's guest* matches what A's guest computed.
7. The daemon is killed with `SIGKILL` while B holds the disk, and a new daemon
   starts on the same disk directory. It reports the disk free; sandbox C gets it and
   reads the same SHA-256.
8. C is deleted, then the disk; its image is gone.

## Result: all eight pass

Every refusal message is the disk check's own, not an earlier "needs a template" or
"not found". The full record is [`report.json`](report.json).

| Step | Observed |
|---|---|
| Disk on host after `mkfs.ext4` | 356,352 bytes allocated of 64 MiB |
| A's mount | `/dev/vda /data ext4 rw,relatime 0 0` |
| SHA-256 written by A, read by B and by C | `3e41331e…a2dd34d3bb` (all three equal) |
| Create, no disk (template restore) | 0.021 s |
| Create with the disk (cold boot), A and B | 0.293 s, 0.253 s |

The create times are single samples taken while the host was at about 100% CPU from
other work. They show only that a sandbox with a disk cold-boots. They are not a
latency measurement.

## Inputs

| Artifact | SHA-256 |
|---|---|
| `hv2-sandboxd` (release, branch `feat/block-volumes` on `cf2e034d`) | `5cf096e166c4eb17bcb9772311ddd386659c3ec637f3b77f75569a8fd82830da` |
| Kernel `bzImage-known-uart-irq` | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Initrd `guest-output-drain.cpio.gz` | `1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c` |

The host was WSL2 kernel 6.18.33.2 with real KVM and mke2fs 1.47.2.

```sh
python3 -I tools/check-block-disk-kvm.py --output out/report.json --daemon <hv2-sandboxd> \
  --kernel bzImage-known-uart-irq --initrd guest-output-drain.cpio.gz
```

## Not shown here

- No power-loss or host-crash test. A guest flush is an `fsync` of the image, and
  nothing beyond that was checked.
- No control-plane routing; disks are node-local (see the guide's Limits).
- No throughput numbers. Requests are served synchronously on the vCPU thread, and
  performance was not measured.
