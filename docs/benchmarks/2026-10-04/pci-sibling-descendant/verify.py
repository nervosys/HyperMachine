from pathlib import Path
import hashlib, json
root = Path(__file__).resolve().parent
for name, digest in json.loads((root / "manifest.json").read_text()).items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
for i in (1, 2, 3):
    text = (root / f"hm-pci-fork-probe-runtime-v{i}.txt").read_text()
    for marker in ["PCI_FORK_INITIAL_ISOLATION_PASS", "PCI_FORK_SECOND_GENERATION_ISOLATION_PASS", "PCI_FORK_OPERATIONS pings=58 exact_commands=126", "PCI_SNAPSHOT_OUTPUT_CLEANUP_PASS"]:
        assert marker in text
    for cycle in range(5): assert f"PCI_FORK_PAUSE_CYCLE_PASS {cycle}" in text
    for name in ["pci-checkpoint-template", "pci-fork-first", "pci-fork-second", "pci-fork-grandchild"]:
        assert f"'{name}' stopped" in text
assert '"restore_pings":16,"exact_restore_commands":32,"cleanup":true' in (root / "hm-pci-fork-default-regression-v1.txt").read_text()
print("Verified payloads, three sibling/descendant lifecycle gates and default restore regression.")
