from pathlib import Path
import hashlib, json
root = Path(__file__).resolve().parent
for name, digest in json.loads((root / "manifest.json").read_text()).items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
for label in ["full-1", "full-2"]:
    r = json.loads((root / (label + "-report.json")).read_text())
    assert r["passed"] and r["failure"] is None and len(r["checks"]) == 56
    assert r["owned_nic_http_requests"] == 7
    assert sum(c.startswith("exact NIC HTTP") for c in r["checks"]) == 7
    assert sum(c.startswith("exact guest HTTP") for c in r["checks"]) == 14
    assert sum(c.startswith("API deletion") for c in r["checks"]) == 4
    assert r["checks"][-1] == "empty node inventory"
print("Verified hashes, both NIC/proxy lifecycle profiles and deletion cleanup.")
