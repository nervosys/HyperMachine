from pathlib import Path
import json,hashlib
p=Path(__file__).parent;c=json.loads((p/'source-context.json').read_text());assert len(c['permitted_root_isolated_sha256'])==148
assert '84 passed; 0 failed' in (p/'pci-tests.txt').read_text() and '47 passed; 0 failed; 2 ignored' in (p/'vm-tests.txt').read_text()
for snap,rel in [('pci-bus.rs','crates/hv2-core/src/pci/bus.rs'),('vm.rs','crates/hv2-core/src/vm.rs'),('virtio-pci.rs','crates/hv2-core/src/devices/virtio_pci.rs'),('pci-guest-probe.rs','crates/hv2-agent/examples/pci_guest_probe.rs')]:assert hashlib.sha256((p/snap).read_bytes()).hexdigest()==c['permitted_root_isolated_sha256'][rel]
s=(p/'diagnostic-runtime.txt').read_text()
for marker in ['PCI: Using configuration type 1 for base access','pci 0000:00:00.0: [8086:1237]','pci 0000:00:03.0: [1af4:1053]',"can't find IRQ for PCI INT A",'probe of virtio0 failed with error -2','=== HV2 READY ===']:assert marker in s
for name in ['quiet-runtime.txt','diagnostic-runtime.txt']:
 s=(p/name).read_text();assert 'PCI_GUEST_PROBE_FAILED: Timeout:' in s and "VM 'pci-guest-probe' stopped" in s and 'exact_commands' not in s
print('84 PCI/47 selected core regressions, pinned snapshots, fixed configuration discovery and still-failing PCI interrupt route independently verified.')
