"""Verify archived process-level key rotation evidence."""
import hashlib
import json
from pathlib import Path
root = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
m = json.loads((root / "api-key-reload-manifest.json").read_text())
raw = (root / "api-key-reload-run-1.json").read_bytes()
assert hashlib.sha256(raw).hexdigest() == m["report_sha256"]
for name, digest in m["source_sha256"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest
r = json.loads(raw)
assert r["binary_sha256"] == m["binary_sha256"]
assert r["coordinator_sha256"] == m["source_sha256"]["api-key-reload-coordinator.py"]
assert r["success"] and r["owned_process_stopped"] and r["exit_code"] == -15
expected = {
    "initial key":200, "old key revoked":401, "new key active":200, "scope preserved":403,
    "malformed preserves active key":200, "malformed preserves revocation":401,
    "empty preserves active key":200, "empty preserves revocation":401,
    "admin collision preserves active key":200, "admin collision preserves revocation":401,
    "missing file preserves active key":200, "admin unchanged":200,
    "expired replacement revokes scoped access":401, "expired set keeps authentication required":401,
    "valid update after rejection":200, "restoration revokes previous key":401}
assert len(r["checks"]) == m["checks"] == len(expected)
assert {c["name"]:c["status"] for c in r["checks"]} == expected
assert all(c["expected"] == c["status"] for c in r["checks"])
print("Verified 16 Unix process key reload checks, archived hashes and owned cleanup.")
