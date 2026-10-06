# First PCI MP-route attempt (failed)

Standard PCI bus and active-low/level INTx entries were appended to the reserved MP-table page after backend boot loading, through permitted VM/PCI code. Bounds, table/entry count, checksum, existing-route conflicts and occupied padding are checked before writing. Protected backend/boot sources were excluded. Source context pins 150 permitted pairs and accepted isolated core; 87 PCI and 47 selected core tests pass.

The original quiet real guest gate retains 15-second budgets and no PCI kernel override but fails before the first ping (one refusal). A separate same-binary boot diagnostic is preserved. No successful PCI operation is claimed, and all changes remain unstaged.

The first attempt chose a free MP bus identifier (1), preserving ISA at 0. Upstream Linux IO_APIC_get_PCI_irq_vector matches srcbus directly against the actual enumerated PCI bus number and rejects a bus marked non-PCI. Root PCI is bus 0, so a free identifier is not sufficient. The next correction must describe PCI at 0 and consistently renumber legacy bus 0 plus every legacy source reference. The existing byte-preservation unit assertion was too weak to validate this kernel bus-number invariant; it must be replaced with preservation of legacy routing semantics. Raw failure and initial snapshots are frozen here rather than rewritten.

Reference: [Linux PCI IRQ lookup](https://github.com/torvalds/linux/blob/v6.1/arch/x86/kernel/apic/io_apic.c).
