# PCI guest queue discovery acceptance

The corrected owned KVM guest completed three fresh runs, each with 64 pings and 64 exact commands, then stopped cleanly (192 pings and 192 commands total). This is a functional gate, not a performance benchmark or competitor win.

PCI queue_size now advertises maximum capacity before negotiation, reports the selected size afterwards, and restores offered capacity on reset. The regression fails against the preceding implementation (red-v2); 106 PCI-filtered library tests pass (green-v2). Earlier unscoped test attempts failed because integration fixture binaries are absent (red-v1/green-v1); they are retained.

The combined candidate also includes serialized PCI INTx assertion/ack/reset, a minimal host bridge configuration header, and PCI bus-zero MP-table routing. The preceding bus-zero diagnostic successfully discovered PCI and mapped INT A to IRQ 11 but failed vsock initialization; its failure is retained. Queue discovery correction enabled the successful gate. Tests do not establish all kernels, migration compatibility, PCI hotplug, MSI-X, or managed fleet support.

Sources: [Virtio 1.2 common configuration and queue setup](https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html); [Linux PCI IRQ lookup](https://github.com/torvalds/linux/blob/v6.1/arch/x86/kernel/apic/io_apic.c).

Tests and guest execution used the isolated build tree. Protected root backend/boot files were excluded. source-context.json hashes the six allowed candidate files, not the complete build closure. The frozen debug binary hash is recorded separately.

Final regression: 47 VM tests pass (2 ignored), 531 agent tests pass, and 64 daemon binary tests pass (2 ignored). Incorrect daemon package/library-target invocations are preserved in daemon-tests and daemon-tests-v2; daemon-tests-v3 is the successful run.
