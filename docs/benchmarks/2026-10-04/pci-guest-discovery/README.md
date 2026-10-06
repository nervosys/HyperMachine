# PCI guest discovery diagnostic (failed)

Separate diagnostic on the PCI IRQ candidate, retaining the same kernel, guest and 15-second agent deadline. The probe enables full guest boot logging and captures the console on failure. The frozen debug executable and formatted probe source are pinned; 147 permitted root/isolated pairs and accepted isolated core were verified before build. This is not a performance run or passing guest gate.

Linux reports `PCI: Fatal: No config space access function found` and `PCI: System does not support PCI`. It nevertheless reaches `/init`, prints `HV2 READY`, and starts hv2-guest-agentd listening on vsock port 1024. The first ping times out with zero refusals, and the vCPU owner and VM stop. This rules out failure to reach guest userspace for this diagnostic, and points investigation toward PCI configuration access discovery. It does not prove an interrupt delivery cause or a specific host-bridge fix. Current VM provisioning already attaches the legacy PC set, so adding those devices is not justified by this failure.

The original quiet first-ping failure and three red/green IRQ/reset tests remain in ../pci-irq-order/. PCI production/probe sources remain unstaged pending real guest operation. The completed MMIO ABBA comparison is separate and immutable.
