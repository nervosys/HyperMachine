#!/usr/bin/env python3
"""Verify archived opt-in sandbox attribution and retained failed fixture run."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    root = args.archive.resolve()
    manifest = json.loads((root / "manifest.json").read_text())
    for name, digest in manifest["sha256"].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root), "path escapes archive")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == digest, "hash mismatch: " + name)
    spec = importlib.util.spec_from_file_location("audit", root / "verify-access-audit.py")
    audit = importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)
    context = json.loads((root / "build-context.json").read_text())
    for name, digest in context["compiled_overlays"].items():
        require(manifest["sha256"]["compiled-" + name.replace("/", "--")] == digest, "compiled source mismatch")
    native = json.loads((root / "native/report.json").read_text())
    require(native["success"] and native["owned_processes_stopped"] and native["artifact_unchanged"] and not native["cleanup_errors"], "native failure")
    require(native["control_plane_sha256"] == context["control_plane_sha256"], "native binary mismatch")
    require(native["coordinator_sha256"] == manifest["sha256"]["check-resource-audit.py"] and native["verifier_sha256"] == manifest["sha256"]["verify-access-audit.py"], "native coordinator mismatch")
    require(len(native["checks"]) == 8 and "four-resigned-invalid-reference-records-rejected" in native["checks"], "native checks missing")
    for label in ["default", "false", "true"]:
        raw = (root / "native" / (label + ".jsonl")).read_bytes()
        require(audit.verify(raw, bytes([42]) * 32) == native[label], "native analysis mismatch")
        require(native[label]["verified_records"] == 24 and native[label]["uncompleted_admissions"] == 0, "missing native requests")
        target = audit.verify(raw, bytes([42]) * 32, "target-private")["sandbox_target"]
        require(target["admissions"] == target["completions"] == (6 if label == "true" else 0), "target counts mismatch")
        events = [json.loads(line)["event"] for line in raw.splitlines()]
        require(sum("sandbox_ref" in e for e in events) == (16 if label == "true" else 0), "reference scope mismatch")
        require(all(secret not in raw for secret in [b"target-private", b"other-private", b"private-name", b"unknown-secret", b"query-secret"]), "raw target leaked")
    kvm = json.loads((root / "kvm-corrected.json").read_text())
    require(kvm["success"] and not kvm["cleanup_errors"] and kvm["remaining_sandboxes"] == 0, "KVM failure")
    require(all(c["success"] for c in kvm["cases"]) and len(kvm["cases"]) == 20, "KVM cases missing")
    require(len(kvm["owned_processes_stopped"]) == 22 and {p["name"] for p in kvm["owned_processes_stopped"] if not p["name"].startswith("cli-")} == {"control", "node", "redis"} and all(p["exit_code"] in [0, -15] for p in kvm["owned_processes_stopped"]), "KVM processes missing")
    for name in ["daemon", "control-plane", "kernel", "initrd", "cli"]:
        require(kvm["artifact_sha256"][name] == context["runtime_sha256"][name], "KVM runtime identity mismatch")
    require(kvm["artifact_sha256"]["coordinator"] == manifest["sha256"]["e2e-tcp-tunnel.py"] and kvm["artifact_sha256"]["audit_verifier"] == manifest["sha256"]["verify-access-audit.py"], "KVM coordinator mismatch")
    raw = (root / "kvm-corrected-access.jsonl").read_bytes()
    result = audit.verify(raw, bytes.fromhex("42" * 32))
    require(all(kvm["access_audit"][k] == v for k, v in result.items()), "KVM audit mismatch")
    require(result["uncompleted_admissions"] == 0 and kvm["resource_audit"]["raw_id_absent"] and kvm["resource_audit"]["target_events"] >= 4, "KVM target evidence missing")
    require(sum("sandbox_ref" in json.loads(line)["event"] for line in raw.splitlines()) == kvm["resource_audit"]["referenced_events"] == 102, "KVM reference count mismatch")
    failed = json.loads((root / "kvm.json").read_text())
    require(not failed["success"] and failed["error"] == "guest fixture did not start" and failed["remaining_sandboxes"] == 0, "initial fixture failure hidden")
    print(json.dumps({"verified_files": len(manifest["sha256"]), "native_records": 72, "kvm_cases": 20, "kvm_records": result["verified_records"], "failed_fixture_run_retained": True, "performance_comparison": False}))


if __name__ == "__main__": main()
