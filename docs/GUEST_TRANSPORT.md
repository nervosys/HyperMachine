# Guest agent transport

`hv2-sandboxd` uses MMIO vsock by default. An operator can select discoverable PCI vsock for a node:

```sh
HV2_KERNEL=/path/to/bzImage HV2_INITRD=/path/to/guest.cpio.gz \
hv2-sandboxd --guest-transport pci --snapshot-store /path/to/store
```

The selection applies to every template and sandbox on that node, including forks and disk pause/resume. Supported values are `mmio` and `pci`; invalid values fail startup. PCI and MMIO templates have different cache identities. PCI-containing snapshots use version 3; older readers reject them. MMIO-only snapshots remain version 2.

Keep the transport setting consistent when reopening a node's snapshot store. Restoring into an incompatible transport is refused. Mixed-transport node scheduling and migration have not been verified; use the same transport on nodes expected to exchange snapshots. Selecting PCI changes the guest agent channel; configured NICs still use MMIO and retain their kernel/device requirements.

The accepted owned KVM guest passes authenticated API create, fork, exact parent/sibling write isolation, three disk pause/resume cycles, descendant fork and deletion. The default MMIO profile passes the same checks. This establishes neither all-kernel support nor a performance advantage. [Reproducible evidence](benchmarks/2026-10-04/pci-daemon-api/README.md).

Network-enabled PCI nodes are also verified in the owned guest: two fresh profiles pass 56 checks each, including seven actual NIC HTTP requests and fourteen separate guest-proxy HTTP requests per profile, fork isolation and three disk pause/resume cycles. The NIC remains MMIO. The owned egress fixture permits one exact host /32; Internet and negative-policy enforcement are outside this gate. [Networking evidence](benchmarks/2026-10-04/pci-network-api/README.md).

A matched release prepared-create ABBA run observes PCI create P50/P95 19.87/24.31 ms versus MMIO 22.50/31.40 ms, with lower sampled CPU but 1.79% higher held process PSS. Small same-host cohorts and differing configured boot arguments limit this comparison; it does not establish a universal advantage. [Measurements and limits](benchmarks/2026-10-04/pci-mmio-release-abba/README.md).

Cold creation has a different tradeoff: the matched release --no-template run observes PCI create P50 753.93 ms versus MMIO 425.68 ms, while PCI has lower pooled P95, sampled CPU and immediately held process PSS. The default remains MMIO. [Cold measurements and limits](benchmarks/2026-10-04/pci-mmio-cold-release-abba/README.md).

PCI now uses the one-UART/headless keyboard boot bundle while preserving PCI/APIC discovery. Template identity explicitly includes PCI transport; existing MMIO identity bytes are unchanged. New PCI templates use the new key. Same-path adoption of a paused guest from the prior frozen release is verified for PCI and MMIO, including saved guest state, identity, fork isolation and further disk resumes; MMIO reuses its existing template key. Keep prior template directories while paused guests can reference them. Other versions, hosts, paths and mixed transports remain unverified. [Upgrade evidence](benchmarks/2026-10-04/pci-fastboot-store-upgrade/README.md). A baseline/candidate cold ABBA run observes PCI create P50/P95 654.58/765.21 ms versus 378.10/428.37 ms, with slightly higher held PSS. The candidate also passes prepared PCI/MMIO lifecycle and network-enabled PCI checks. This refresh is a same-host binary comparison, not a PCI-versus-MMIO or managed competitor win. [Current candidate and verification](benchmarks/2026-10-04/pci-fastboot/README.md).
