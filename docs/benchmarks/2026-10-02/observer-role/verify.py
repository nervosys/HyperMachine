"""Verify preserved functional evidence without executing product binaries."""
import hashlib
import importlib.util
import json
from pathlib import Path

root = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


manifest = json.loads((root / "manifest.json").read_text())
for name, digest in manifest.items():
    path = root / name
    require(path.resolve().is_relative_to(root), "manifest path escaped archive")
    require(hashlib.sha256(path.read_bytes()).hexdigest() == digest, "hash mismatch: " + name)
report = json.loads((root / "kvm.json").read_text())
require(report["success"] and len(report["cases"]) == 23, "incomplete KVM run")
require(all(row["success"] for row in report["cases"]), "failed case")
require(not report["cleanup_errors"] and report["remaining_sandboxes"] == 0, "cleanup failed")
require(len(report["owned_processes_stopped"]) == 22, "missing process cleanup")
require(all(row["exit_code"] is not None for row in report["owned_processes_stopped"]), "live process")
roles = [row for row in report["cases"] if row["name"] == "observer-role-caps-admin-with-real-guest"]
require(len(roles) == 1 and all(roles[0]["result"].values()), "observer checks missing")
spec = importlib.util.spec_from_file_location("audit_verifier", root / "verify-access-audit.py")
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)
audit = verifier.verify((root / "kvm-access.jsonl").read_bytes(), bytes([0x42]) * 32)
require(audit["verified_records"] == 222 and audit["uncompleted_admissions"] == 0, "audit incomplete")
require(audit["verified_records"] == report["access_audit"]["verified_records"], "report mismatch")
context = json.loads((root / "build-context.json").read_text())
require(context["control_plane_sha256"] == report["artifact_sha256"]["control-plane"], "binary mismatch")
for name, digest in context["compiled_overlays"].items():
    require(hashlib.sha256((root / ("compiled-" + Path(name).name)).read_bytes()).hexdigest() == digest, "source mismatch")
print(json.dumps({"archive_verified": True, "cases": 23, "audit_records": 222}))
