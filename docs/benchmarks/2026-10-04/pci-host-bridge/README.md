# PCI host-bridge discovery follow-up (guest gate still failed)

The candidate ensures a minimal 82441FX-compatible host-bridge configuration header before PCI-vsock attachment. This is a standard host-bridge header, not a vendor-specific chipset implementation. Existing bus-zero host bridges are preserved, an occupied slot zero is never overwritten, and endpoint configuration is retained. MMIO-only attachment does not invoke this helper. The upstream Linux v6.1 x86 direct-access sanity check is linked and hashed in source-context.json.

Isolated regressions pass: 84 PCI/root/config/transport tests and 47 selected core tests (2 ignored). The root tests cover immutable identity/class, repeat calls, custom host bridges, collision refusal and endpoint preservation. Initial root source pairing differed only by CRLF/LF; logical contents were checked before editing and formatted root/isolated bytes are pinned. The candidate catalog covers 148 permitted source pairs and accepted isolated protected core.

The original quiet gate retains the same guest/kernel, no PCI override and 15-second agent budgets, but times out before the first ping (one refusal). A separate full-console diagnostic on the same frozen debug probe confirms Linux selects type-1 configuration access, discovers host bridge 8086:1237 and virtio-vsock 1af4:1053 at slot 3, then reports it cannot find IRQ for PCI INT A. The virtio-vsock driver probe fails with -2. Guest userspace and agent start, but no passing PCI guest operation is claimed. Both runs stop the VM and owner; raw logs are preserved.

The discovery fix is supported, but PCI interrupt routing is the next unresolved gate. Production host-bridge/PCI changes and probe remain unstaged until real operation is verified. Prior failed discovery/IRQ archives and completed MMIO performance evidence remain immutable. PCI snapshots and performance superiority are not established.

Reference: [Linux x86 direct PCI discovery](https://github.com/torvalds/linux/blob/v6.1/arch/x86/pci/direct.c).
