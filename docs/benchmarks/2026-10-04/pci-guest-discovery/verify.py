from pathlib import Path
import json,hashlib
p=Path(__file__).parent;c=json.loads((p/'source-context.json').read_text());s=(p/'probe-runtime.txt').read_text()
for marker in ['PCI: Fatal: No config space access function found','PCI: System does not support PCI','=== HV2 READY ===','hv2-guest-agentd 1.1.0 listening on vsock port 1024','PCI_GUEST_PROBE_FAILED: Timeout:',"VM 'pci-guest-probe' stopped"]:assert marker in s
assert 'exact_commands' not in s and len(c['permitted_root_isolated_sha256'])==147
for snap,rel in [('virtio-pci.rs','crates/hv2-core/src/devices/virtio_pci.rs'),('pci-guest-probe.rs','crates/hv2-agent/examples/pci_guest_probe.rs')]:assert hashlib.sha256((p/snap).read_bytes()).hexdigest()==c['permitted_root_isolated_sha256'][rel]
print('PCI discovery failure with actual guest userspace/agent readiness and VM stop independently verified; no successful PCI operation claimed.')
