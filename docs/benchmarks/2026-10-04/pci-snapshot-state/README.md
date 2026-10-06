# PCI checkpoint state acceptance

The unchanged owned PCI checkpoint gate that previously timed out now passes three fresh runs with the frozen corrected debug executable: 48 restored pings and 96 exact restored commands, including a marker captured before checkpoint. All guests stop and checkpoint outputs are removed. The original failure remains in ../pci-snapshot-failure.

PCI state now includes transport registers, accepted features, queue addresses/sizes/readiness/cursors, device counters, root-bus standard-function configuration (including BAR size-probe state), and the CONFIG_ADDRESS latch. Restore validates device identity, mapped BAR/IRQ, supported features, queue count/size, writable guest ring ranges/alignment, configuration identity/layout and unique function addresses before changing guest memory. External host connections reset; they are not resurrected.

PCI-bearing snapshots use wire version 3 so older version-2 readers refuse them. MMIO-only snapshots retain version 2 and omit the new empty fields. 110 PCI-filtered core tests, 14 snapshot-file tests, 47 VM tests (2 ignored), 531 agent tests, and 64 daemon tests (2 ignored) pass. Tests cover queue/cursor/IRQ roundtrips, invalid-state rejection without transport mutation, PCI command/BAR probe preservation and snapshot wire versions.

The existing MMIO guest restore example initially failed its health assertion because it only counted IRQ 0. Captured real guest counters show no IRQ 0 row and local APIC LOC advancing 144 to 154. The probe now counts legacy IRQ 0 and LOC across CPU columns, with a parser test; the timer advance, wall-clock skew, arithmetic and RNG uniqueness gates remain. All five MMIO restores pass with advancing timers, clock skew <=2 seconds, exact arithmetic and five distinct draws after reseeding. Original failure, diagnostic and explicit owned-output cleanup are retained.

Initial development failures are preserved: absent RAM in a transport test fixture, missing public config export, use of private I/O handle fields and test register offset type mismatches. The successful PCI test log is v5. These failed compile/test attempts are not passing evidence.

This establishes PCI checkpoint/restore operation for this owned kernel/guest, not performance superiority. Timings printed by the MMIO example are functional-check output, not a controlled benchmark. PCI hotplug, MSI-X, arbitrary bus/bridge topologies, other kernels/backends, independent hosts and managed migration remain unverified. The accepted earlier performance comparison remains tied to its original binaries.

source-context.json records eight permitted source hashes and exact guest/binary inputs; it is not a complete build closure. Protected root backend/boot sources were excluded. Builds used the accepted isolated tree with --locked.
