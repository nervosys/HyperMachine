# PCI sibling and descendant lifecycle gate

Three fresh owned KVM runs pass with the same frozen debug executable and accepted kernel/initrd. Each creates two independent VMs from one PCI checkpoint, writes distinct markers and verifies sibling isolation. Five alternating pause cycles keep the sibling answering while each other guest is paused; both resume and retain their own exact marker. A checkpoint of the first restored guest creates a third guest, which retains the earlier marker after its parent writes a newer marker. The other sibling remains unchanged.

| Scope | Per run | Three runs |
| --- | ---: | ---: |
| Restored guests | 3 | 9 |
| Successful restored pings | 58 | 174 |
| Exact restored command responses | 126 | 378 |
| Explicit pause/resume pairs | 10 | 30 |

Each run stops the template and all three restored guests, and removes the original and descendant checkpoint/memory files. A separate default-profile run with this same executable passes the original 16-ping/32-command restore gate. Those default operations and template commands are excluded from the table.

This extends reproducible direct AgentVM/VM lifecycle verification. It does not test the sandbox API fork endpoint, arbitrary memory noninterference, cross-host migration, managed fleet operation, all kernels/backends or competitor latency/throughput. Only the probe changed; production source hashes match the preceding PCI snapshot acceptance. Timed operation budgets remain 15 seconds, with no retry widening.

Reproduce from the authorized isolated tree after building with `cargo build --locked -p hv2-agent --example pci_snapshot_probe`:

```sh
HV2_PCI_FORK_GATE=1 \
HV2_KERNEL=/var/tmp/hm-competitive/bzImage-known-uart-irq \
HV2_INITRD=/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz \
RUST_LOG=info timeout 180 /var/tmp/hm-pci-fork-probe-v1
```

The frozen binary was copied from target/debug/examples/pci_snapshot_probe. source-context.json records eight permitted source hashes and runtime inputs; prior production source snapshots remain in ../pci-snapshot-state. This is not a complete build closure. Protected root backend/boot sources remain excluded.
