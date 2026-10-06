# PCI vsock and MMIO NIC lifecycle acceptance

Two fresh owned KVM node profiles pass 56 checks each with the same frozen daemon binary, kernel and initrd. The node runs `--guest-transport pci --network`; guest agent traffic uses PCI vsock, and the NIC remains MMIO. Each profile covers authenticated create, two-child and descendant forks, exact sibling/parent marker isolation, three disk pause/resume cycles and deletion of all four sandboxes to an empty inventory.

Each full profile verifies seven actual guest NIC HTTP requests to an owned host server, with unique exact response markers and a matching server receipt. The guest uses wget to the host's eth0 IPv4 address; the operator allows that exact reserved /32 and the sandbox allowOut contains only it under a deny default. No external destination is requested. This tests NIC data delivery, not enforcement against other destinations.

Fourteen separate guest HTTP requests per profile use the node's loopback proxy and check exact marker bodies after forks/resumes. This forwarding uses the guest channel, so those requests alone do not prove NIC operation. The earlier proxy-only profile passes 42 checks and is retained with that narrower scope. Across the two full profiles, there are 14 NIC requests and 28 proxy requests; their command responses are checked independently.

The owned host HTTP server closes, all sandboxes are deleted, the node inventory is empty, and the daemon stops. Reproduction driver v2 starts exclusive fresh fixture directories, ports and host server; it uses the frozen binary and accepted source guards. The frozen binary and nine permitted source hashes match ../pci-daemon-api; no production code changed. This source catalog is not a complete build closure. Protected root backend/boot sources remain excluded.

This closes the previously unverified network-enabled configuration for the owned guest. Throughput, negative egress-policy gates, Internet operation, other kernels/backends, mixed-transport node scheduling and migration, and managed fleet guarantees remain unverified. No performance advantage is claimed.
