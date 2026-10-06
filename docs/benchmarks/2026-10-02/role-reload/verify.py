"""Verify preserved live process role-reload evidence."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


manifest = json.loads((root / "manifest.json").read_text())
for name, digest in manifest.items():
    path = root / name
    require(path.resolve().is_relative_to(root), "archive path escaped root")
    require(hashlib.sha256(path.read_bytes()).hexdigest() == digest, "hash mismatch: " + name)

expected = {
    "initial key": 200, "old key revoked": 401, "new key active": 200,
    "scope preserved": 403, "missing file preserves active key": 200,
    "admin unchanged": 200, "expired replacement revokes scoped access": 401,
    "expired set keeps authentication required": 401,
    "valid update after rejection": 200, "restoration revokes previous key": 401,
    "operator volume inventory": 200, "same-key downgrade preserves inventory": 200,
    "same-key downgrade blocks capabilities": 403, "observer blocks mutation": 403,
    "observer blocks GET tunnel": 403, "same-key upgrade restores capability": 200,
    "observer scope allows sandbox inventory": 200,
    "observer scope excludes template inventory": 403, "role changes keep legacy admin": 200,
}
for name in ["malformed", "empty", "admin collision", "oversized", "invalid UTF-8"]:
    expected[name + " preserves active key"] = 200
    expected[name + " preserves revocation"] = 401
for name in ["unknown role", "null role"]:
    expected[name + " preserves inventory"] = 200
    expected[name + " preserves capability denial"] = 403
for stem, coordinator in [("role-reload", "e2e-api-key-reload.py"),
                           ("role-reload-final", "e2e-api-key-reload-final.py")]:
    report = json.loads((root / (stem + ".json")).read_text())
    require(report["success"] and report["owned_process_stopped"] and report["exit_code"] == -15, "process check failed")
    require(report["binary_unchanged"] and report["credentials_absent"], "input/privacy check failed")
    require(report["binary_sha256"] == "d0757724cfcbacaeb80e64dade681a6299e7443f39bdda16bf458dd251bd00ef", "unexpected binary")
    require(report["coordinator_sha256"] == manifest[coordinator], "coordinator mismatch")
    rows = report["checks"]
    require(len(rows) == len(expected) == 33, "missing checks")
    require({row["name"]: row["status"] for row in rows} == expected, "HTTP result mismatch")
    require(all(row["status"] == row["expected"] for row in rows), "failed HTTP expectation")
    log = (root / (stem + ".log")).read_bytes()
    require(hashlib.sha256(log).hexdigest() == report["service_log_sha256"], "log mismatch")
    require(log.count(b"API key policies reloaded") == 7, "successful reload acknowledgements missing")
    require(log.count(b"API key reload rejected") == 8, "rejected reload acknowledgements missing")
print(json.dumps({"archive_verified": True, "processes": 2, "http_checks": 66}))
