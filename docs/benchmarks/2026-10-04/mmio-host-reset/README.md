# Host MMIO reset interrupt invariant

The host Device::reset path previously cleared interrupt status without releasing an asserted level IRQ. It now resets registers and releases the line while holding the status lock, ordering reset with assertion and acknowledgement. Register reset already released the line.

The new regression raises both queue and configuration causes, checks the line, then exercises register reset and host reset. It fails against the prior host implementation with `reset left IRQ asserted`; all 17 MMIO transport tests and 47 VM tests pass (2 VM tests ignored). The existing guest_exec_probe also boots an owned KVM/MMIO guest, pings its agent and runs uname with exit 0. That runtime smoke does not directly exercise host reset; the deterministic test proves the reset line invariant.

Builds and tests used the accepted isolated tree; protected root backend/boot files were excluded. context.json records isolated protected hashes and the smoke binary hash. This is a lifecycle correctness correction, with no new performance or competitor superiority claim. Previously measured ABBA results remain tied to their original frozen binary.
