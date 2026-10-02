#!/usr/bin/env python3
"""Verify archived encrypted-backup evidence, including both failed fixtures."""
import argparse
import hashlib
import importlib.util
import json
import re
from pathlib import Path


def require(condition, message):
    if not condition: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    root = args.archive.resolve()
    catalog = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    for name, digest in catalog["sha256"].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root), "path escapes archive")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == digest, "hash mismatch: " + name)
    context = json.loads((root / "build-context.json").read_text(encoding="utf-8"))
    clean = {
        "crates/hv2-core/src/backends/kvm.rs": "05bb3d41799f72970df19dc3bced8991c758664bf73de95de2c15a0902079a43",
        "crates/hv2-core/src/boot/linux.rs": "f24a7aeb7f1a493188bad3f83df6d2f0ba5df5959ed7d185f13224872b5712a1",
        "crates/hv2-core/src/boot/source.rs": "4f269436b7bee48bebb2bcef87eb9d4cd7048ad8add634a7e4949764d8a3c23c",
    }
    require(context["clean_boot_sha256"] == clean, "provisional boot sources included")
    for name, digest in context["compiled_overlays"].items():
        require(catalog["sha256"]["compiled-" + name.replace("/", "--")] == digest, "compiled overlay mismatch")
    require(context["linux_tests"] == 41 and "41 passed; 0 failed" in (root / "tests.log").read_text(encoding="utf-8"), "Linux tests missing")
    require("Finished `release`" in (root / "build.log").read_text(encoding="utf-8") and "Finished `dev`" in (root / "clippy.log").read_text(encoding="utf-8"), "build/Clippy missing")
    require(context["windows_tests"] == 36 and "36 passed; 0 failed" in (root / "windows-tests.log").read_text(encoding="utf-8"), "Windows tests missing")
    require("Finished `dev`" in (root / "windows-clippy.log").read_text(encoding="utf-8"), "Windows Clippy missing")
    cohorts = {
        "synthetic": (True, "first", 12),
        "kvm": (False, "first", 12),
        "kvm-corrected": (False, "second", 13),
        "kvm-final": (True, "third", 14),
        "kvm-coverage": (True, "fourth", 14),
        "kvm-verified": (True, None, 15),
    }
    passed_recoveries = 0
    for name, (success, version, checks) in cohorts.items():
        report = json.loads((root / name / "report.json").read_text(encoding="utf-8"))
        require(report["success"] is success and report["performance_comparison"] is False, "cohort outcome changed")
        require(report["artifacts_unchanged"] and not report["cleanup_errors"], "fixture artifacts/cleanup failed")
        require(all(p["exit_code"] == 0 for p in report["processes_stopped"]), "owned process not stopped")
        require(len(report["checks"]) == checks, "checks missing")
        prefix = version + "-" if version else ""
        require(report["artifact_sha256"]["tool"] == catalog["sha256"][prefix + "backup-snapshot-store.py"], "tool provenance mismatch")
        require(report["artifact_sha256"]["coordinator"] == catalog["sha256"][prefix + "check-object-backup.py"], "fixture provenance mismatch")
        if name != "synthetic":
            require(report["artifact_sha256"]["daemon"] == context["daemon_sha256"], "daemon mismatch")
            require(report["artifact_sha256"]["kernel"] == "afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd"
                    and report["artifact_sha256"]["initrd"] == "d520964ff11dff3bc7090af9ee988eedd3c5c5ffb23a5a3708250e3248a2cb17", "guest inputs mismatch")
        if success and name != "synthetic":
            passed_recoveries += 1
            flags = report["kvm"]
            for field in ["same_id_and_access_token", "live_process_and_memory_marker", "filesystem_and_boot_id",
                          "volume_marker", "guest_9p_volume_roundtrip", "same_volume_id_and_token", "volume_inventory_empty",
                          "original_path_unavailable", "cleanup_inventory_empty", "receipt_checksum_verified", "layered_memory_base_relocated"]:
                require(flags[field] is True, "KVM recovery check failed: " + field)
            if name != "kvm-final":
                require(flags["named_snapshot_recovered"] and flags["ordinary_volume_snapshot_filename_preserved"], "named snapshot/application file evidence missing")
            receipt = flags["backup_receipt"]
            require(receipt["success"] and 0 < receipt["encrypted_bytes"] <= 5_000_000_000
                    and receipt["expanded_bytes"] > 1024**3 and len(receipt["sha256"]) == 64, "invalid KVM backup receipt")
            require({p["name"] for p in report["processes_stopped"]} == {"s3", "node-original", "node-recovered"}, "KVM processes missing")
            log = (root / name / "s3.log").read_text(encoding="utf-8")
            require('PUT /hm-owned-backup-fixture/kvm.hmb HTTP/1.1" 200' in log
                    and 'GET /hm-owned-backup-fixture/kvm.hmb HTTP/1.1" 200' in log, "KVM S3 wire evidence missing")
        elif name == "kvm":
            require(report["error"] == "daemon readiness timeout", "initial readiness failure hidden")
        elif name == "kvm-corrected":
            require("No such file or directory" in report["error"] and "volumes/backup-marker" in report["error"], "second fixture failure hidden")
    current = json.loads((root / "kvm-verified/report.json").read_text(encoding="utf-8"))
    names = {c["name"] for c in current["checks"]}
    require(names == {
        "active-store-refused", "encrypted-conditional-upload", "relocated-roundtrip-with-empty-directory-and-permissions",
        "wrong-key-tampered-truncated-refused", "expanded-byte-limit-refused", "unsafe-path", "symlink", "missing-file",
        "wrong-digest", "extra-file", "duplicate-file", "source-symlink-and-unfinished-claim-refused",
        "independent-receipt-mismatch-refused", "lost-upload-response-retains-verifiable-receipt",
        "real-kvm-state-recovered-through-s3-at-new-root",
    }, "final checks incomplete")
    lost = next(c for c in current["checks"] if c["name"] == "lost-upload-response-retains-verifiable-receipt")
    require(lost["passed"] and lost["injection"] == "client acknowledgement loss after real emulator commit"
            and lost["receipt"]["object"] == "lost-response.hmb" and len(lost["receipt"]["sha256"]) == 64, "uncertain-upload evidence missing")
    log = re.sub(r"\x1b\[[0-9;]*m", "", (root / "kvm-verified/s3.log").read_text(encoding="utf-8"))
    require('PUT /hm-owned-backup-fixture/synthetic.hmb HTTP/1.1" 412' in log, "conditional-write refusal missing")
    require('PUT /hm-owned-backup-fixture/lost-response.hmb HTTP/1.1" 200' in log
            and 'GET /hm-owned-backup-fixture/lost-response.hmb HTTP/1.1" 200' in log, "lost-response committed object missing")
    relocation = json.loads((root / "relocation-evidence.json").read_text(encoding="utf-8"))
    require(relocation["original_path_unavailable"], "original store availability changed")
    named = [(name, row) for name, row in relocation["snapshots"].items() if name.startswith("snapshots/")]
    require(named, "named snapshot payload evidence missing")
    for name, row in named:
        prefix = relocation["source_root"] + "/"
        require(row["original_base"].startswith(prefix), "original base outside store")
        expected = relocation["recovered_root"] + "/" + row["original_base"][len(prefix):]
        require(row["recovered_base"] == expected and row["original_payload_sha256"] == row["recovered_payload_sha256"], "snapshot relocation changed payload")
    regression = json.loads((root / "regression.json").read_text(encoding="utf-8"))
    require(regression["success"] and len(regression["cases"]) == 20 and all(c["success"] for c in regression["cases"])
            and not regression["cleanup_errors"] and regression["remaining_sandboxes"] == 0, "TLS regression incomplete")
    require(len(regression["owned_processes_stopped"]) == 22 and all(p["exit_code"] in [0, -15] for p in regression["owned_processes_stopped"]), "TLS processes not stopped")
    require(regression["artifact_sha256"]["daemon"] == context["daemon_sha256"]
            and regression["artifact_sha256"]["coordinator"] == catalog["sha256"]["e2e-tcp-tunnel.py"]
            and regression["artifact_sha256"]["audit_verifier"] == catalog["sha256"]["verify-access-audit.py"], "TLS provenance mismatch")
    spec = importlib.util.spec_from_file_location("audit", root / "verify-access-audit.py")
    audit = importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)
    records = audit.verify((root / "regression-access.jsonl").read_bytes(), bytes.fromhex("42" * 32))
    require(records["verified_records"] == 160 and records["uncompleted_admissions"] == 0
            and all(regression["access_audit"][k] == value for k, value in records.items()), "TLS audit mismatch")
    print(json.dumps({"verified_files": len(catalog["sha256"]), "passing_recovery_cohorts": passed_recoveries,
                      "failed_fixture_cohorts_retained": 2, "final_checks": 15, "tls_cases": 20,
                      "tls_audit_records": 160, "managed_store_durability_verified": False, "performance_comparison": False}))


if __name__ == "__main__": main()
