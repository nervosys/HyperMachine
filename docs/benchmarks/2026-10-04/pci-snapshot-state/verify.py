from pathlib import Path
import hashlib, json, re
root = Path(__file__).resolve().parent
for name, digest in json.loads((root / "manifest.json").read_text()).items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
for i in (2, 3, 4):
    text = (root / f"hm-pci-snapshot-probe-runtime-v{i}.txt").read_text()
    assert '"restore_pings":16,"exact_restore_commands":32,"cleanup":true' in text
    assert "'pci-checkpoint-template' stopped" in text and "'pci-checkpoint-restored' stopped" in text
text = (root / "hm-pci-snapshot-mmio-probe-runtime-v2.txt").read_text()
for i in range(5):
    m = re.search(rf'restore {i}\s+:.*timer (\d+)->(\d+), clock skew (\d+)s, 6\*7=42', text)
    assert m and int(m[2]) > int(m[1]) and int(m[3]) <= 2
assert '5 distinct after' in text
print("Verified hashes, three PCI restore gates and five MMIO timer/clock/arithmetic/RNG gates.")
