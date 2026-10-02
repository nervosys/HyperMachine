#!/usr/bin/env python3
"""Verify the retained multipart recovery and failure evidence."""
import argparse
import hashlib
import json
from pathlib import Path


def require(value, message):
    if not value: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    root = parser.parse_args().archive.resolve()
    manifest = json.loads((root / "manifest.json").read_text())
    for name, expected in manifest["sha256"].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root), "archive path escaped")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, "hash mismatch: " + name)
    for name in ["small", "checksum-inspection", "small-corrected", "large"]:
        report = json.loads((root / name / "report.json").read_text())
        require(report["success"] == (name in ["small-corrected", "large"]), "outcome changed")
        require(report["artifacts_unchanged"] and report["emulator_exit_code"] == 0
                and not report["performance_comparison"], "artifact/process/comparison mismatch")
        require(report["tool_sha256"] == manifest["sha256"]["backup-snapshot-store.py"], "tool provenance mismatch")
        coordinator = "check-multipart-backup.py" if report["success"] else name + "-coordinator.py"
        require(report["coordinator_sha256"] == manifest["sha256"][coordinator], "coordinator provenance mismatch")
        if not report["success"]:
            require(report["error"] == "composite checksum confused with whole-object hash", "failed fixture omitted")
            continue
        require(len(report["checks"]) == 9 and all(c["passed"] for c in report["checks"]), "failure/roundtrip coverage missing")
        require(report["emulator_limitations"], "emulator limitation omitted")
        receipt = report["checks"][0]["receipt"]
        require(receipt["parts"] == 3 and receipt["upload_method"] == "multipart", "small multipart missing")
        log = (root / name / "s3.log").read_text()
        require(' 412 ' in log and 'uploadId=' in log, "conditional multipart wire evidence missing")
        if name == "large":
            receipt = report["large"]
            require(receipt["source_bytes"] == 5_001_000_000 and receipt["encrypted_bytes"] > 5_000_000_000
                    and receipt["parts"] >= 75 and receipt["recovered_hash_identical"], "large recovery missing")
    report = json.loads((root / "kvm/report.json").read_text())
    require(report["success"] and report["artifacts_unchanged"] and not report["cleanup_errors"]
            and len(report["checks"]) == 15 and not report["performance_comparison"], "KVM fixture failed")
    require(all(p["exit_code"] == 0 for p in report["processes_stopped"]), "KVM processes not stopped")
    require(report["artifact_sha256"]["tool"] == manifest["sha256"]["backup-snapshot-store.py"]
            and report["artifact_sha256"]["coordinator"] == manifest["sha256"]["check-object-backup.py"], "KVM provenance mismatch")
    require(report["artifact_sha256"]["daemon"] == "2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f", "daemon changed")
    flags = report["kvm"]
    require(all(value is True for name, value in flags.items() if name != "backup_receipt"), "KVM recovery incomplete")
    require(flags["backup_receipt"]["upload_method"] == "multipart" and flags["backup_receipt"]["parts"] >= 2, "KVM multipart missing")
    print(json.dumps({"success": True, "hashed_files": len(manifest["sha256"]), "performance_comparison": False}))


if __name__ == "__main__": main()
