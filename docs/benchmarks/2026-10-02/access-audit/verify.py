#!/usr/bin/env python3
"""Verify complete audited control-plane functional evidence and source provenance."""
import hashlib
import importlib.util
import json
from pathlib import Path


def main():
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
    spec = importlib.util.spec_from_file_location("audit_verifier", root / "verify-access-audit.py")
    verifier = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verifier)
    context = json.loads((root / "build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and context["artifacts_unchanged_after_final_kvm"]
    assert not context["performance_win_established"]
    prior = json.loads((root.parent / "registration-mutations/build-context.json").read_text())
    assert context["clean_boot_sha256"] == prior["committed_core_sha256"]
    for name in ("windows-core-tests.log", "core-tests.log"):
        assert "test result: ok. 13 passed; 0 failed;" in (root / name).read_text()
    for name in ("windows-cluster-tests.log", "cluster-tests-final.log"):
        text = (root / name).read_text()
        assert "test result: ok. 40 passed; 0 failed;" in text
        assert "test result: ok. 23 passed; 0 failed;" in text
    for name in ("windows-clippy.log", "clippy.log"):
        text = (root / name).read_text()
        assert "Finished" in text and "error:" not in text
    for relative, expected in context["source_overlays"].items():
        assert manifest["files_sha256"]["compiled-" + relative.replace("/", "--")] == expected
    native = json.loads((root / "native-verified-report.json").read_text())
    assert native["success"] and native["owned_processes_stopped"] and native["artifact_unchanged"]
    assert not native["cleanup_errors"] and len(native["checks"]) == 11
    assert native["control_plane_sha256"] == context["control_plane_sha256"]
    assert native["fixture_sha256"] == manifest["files_sha256"]["check-access-audit.py"]
    assert native["verifier_sha256"] == manifest["files_sha256"]["verify-access-audit.py"]
    assert verifier.verify((root / "native-verified-access.jsonl").read_bytes(), bytes.fromhex("42" * 32)) == native["audit"]
    kvm = json.loads((root / "kvm-verified.json").read_text())
    assert kvm["success"] and not kvm["cleanup_errors"] and kvm["remaining_sandboxes"] == 0
    assert len(kvm["cases"]) == 22 and all(case["success"] for case in kvm["cases"])
    assert len(kvm["owned_processes_stopped"]) == 22
    assert all(row["exit_code"] is not None for row in kvm["owned_processes_stopped"])
    assert kvm["artifact_sha256"]["control-plane"] == context["control_plane_sha256"]
    assert kvm["artifact_sha256"]["coordinator"] == manifest["files_sha256"]["e2e-tcp-tunnel.py"]
    assert kvm["artifact_sha256"]["audit_verifier"] == manifest["files_sha256"]["verify-access-audit.py"]
    result = verifier.verify((root / "kvm-verified-access.jsonl").read_bytes(), bytes.fromhex("42" * 32))
    assert result == {name: kvm["access_audit"][name] for name in result}
    assert result["verified_records"] == 204 and result["uncompleted_admissions"] == 0
    assert kvm["access_audit"]["credentials_absent"]
    first = json.loads((root / "kvm.json").read_text())
    assert first["success"] and first["access_audit"]["verified_records"] == 204
    assert first["artifact_sha256"]["audit_verifier"] == manifest["files_sha256"]["verify-access-audit-first.py"]
    assert "BadMac" in (root / "windows-float-roundtrip-failure.log").read_text()
    print(json.dumps({"archive_verified": True, "final_process_checks": 11,
        "final_kvm_checks": 22, "final_audit_records": 224, "performance_win_established": False}))


if __name__ == "__main__":
    main()
