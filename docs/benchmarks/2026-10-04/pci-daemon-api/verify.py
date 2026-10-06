from pathlib import Path
import hashlib, json
root = Path(__file__).resolve().parent
for name, digest in json.loads((root / "manifest.json").read_text()).items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
for label in ["pci-pass", "default-mmio-pass"]:
    report = json.loads((root / (label + "-report.json")).read_text())
    assert report["passed"] and report["failure"] is None and len(report["checks"]) == 27
    assert report["checks"][-1] == "empty node inventory"
    assert sum(c.startswith("API deletion") for c in report["checks"]) == 4
assert json.loads((root / "recovery-cleanup.json").read_text())["passed"]
assert not json.loads((root / "pci-shell-shape-failure-report.json").read_text())["passed"]
assert not json.loads((root / "pci-fork-shape-failure-report.json").read_text())["passed"]
print("Verified payloads, both API lifecycle profiles and failed-fixture recovery cleanup.")
