from pathlib import Path
import json,hashlib
p=Path(__file__).parent;c=json.loads((p/'source-context.json').read_text());red=(p/'old-order-tests.txt').read_text()
assert '3 failed' in red and all(x in red for x in ['status lock released before line assertion','status lock released before line deassertion','reset left IRQ asserted'])
for name,text in [('pci','16 passed; 0 failed'),('vm','47 passed; 0 failed; 2 ignored'),('agent','531 passed; 0 failed'),('daemon','64 passed; 0 failed; 2 ignored')]:assert text in (p/(name+'-tests.txt')).read_text()
for snap,rel in [('virtio-pci.rs','crates/hv2-core/src/devices/virtio_pci.rs'),('pci-guest-probe.rs','crates/hv2-agent/examples/pci_guest_probe.rs')]:assert hashlib.sha256((p/snap).read_bytes()).hexdigest()==c['permitted_root_isolated_sha256'][rel]
s=(p/'probe-runtime.txt').read_text();assert 'PCI_GUEST_PROBE_FAILED: Timeout:' in s and "VM 'pci-guest-probe' stopped" in s and 'exact_commands' not in s
print('Three red/green PCI regressions, affected suites and failed first-ping runtime with VM stop independently verified; no passing PCI guest claimed.')
