# PCI checkpoint restore failure

The owned Linux/KVM template boots over PCI vsock, pings the agent and writes/reads an exact persistent marker. Snapshot capture succeeds but explicitly warns that PCI transport state is omitted. A separately provisioned VM launches from the snapshot; its first ping times out at the original 15-second budget, with zero refusals. Both VMs stop and the snapshot and memory image are removed. No restore command succeeds.

This is a reproducible missing lifecycle capability, not a performance result. The gate will require 16 restored pings and 32 exact restored commands (including the checkpoint marker) before reporting success. Current PCI cold guest acceptance does not establish PCI checkpoint/restore support. Existing default MMIO checkpoint results remain separately scoped.

Reproduction from the authorized isolated tree:

```sh
CARGO_TARGET_DIR=/var/tmp/hm-object-backup/target cargo build --locked -p hv2-agent --example pci_snapshot_probe
HV2_KERNEL=/var/tmp/hm-competitive/bzImage-known-uart-irq \
HV2_INITRD=/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz \
RUST_LOG=info timeout 180 /var/tmp/hm-pci-snapshot-probe-v1
```

The frozen debug executable was copied from target/debug/examples/pci_snapshot_probe. Its hash and the exact kernel/initrd hashes are in source-context.json. Source snapshots cover five permitted files, not a complete build closure. Protected root backend/boot sources were excluded.

Required correction: capture/restore PCI transport registers, queue addresses/size/readiness/progress, negotiated device features/counters, and guest-writable PCI configuration; validate device/layout compatibility before applying state. Snapshot version handling must ensure old readers reject new PCI state rather than silently drop it. The failure alone does not prove that missing PCI state is the sole cause of the observed timeout; the unchanged gate must pass after correction.
