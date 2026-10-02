#!/usr/bin/env python3
"""Verify every audit benchmark request, chain, order and functional result."""
import hashlib
import importlib.util
import itertools
import json
import math
from pathlib import Path


def load_module(root, name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), root / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / "manifest.json").read_text())["files_sha256"]
    for name, expected in manifest.items():
        assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
    analyzer = load_module(root, "analyze-access-audit-cost")
    verifier = load_module(root, "verify-access-audit")
    context = json.loads((root / "build-context.json").read_text())
    prior = json.loads((root.parent / "access-audit/build-context.json").read_text())
    assert context["provisional_boot_changes_excluded"] and context["artifact_hashes_rechecked_after_all_runs"]
    assert context["clean_boot_sha256"] == prior["clean_boot_sha256"]
    assert context["parent_sha256"] == prior["control_plane_sha256"]
    assert context["runtime_change_adopted"]
    for relative, expected in context["source_overlays"].items():
        assert manifest["compiled-" + relative.replace("/", "--")] == expected
    aggregate = json.loads((root / "aggregate.json").read_text())
    totals = {"total_attempted": 0, "total_passed": 0, "audit_records_verified": 0}
    summaries = {}
    for name in ("initial", "audited", "default", "single", "single-default", "repeat"):
        directory = root / name
        matrix = json.loads((directory / "matrix.json").read_text())
        assert matrix["success"] and matrix["artifacts_unchanged"] and not matrix["added_CPU_load"]
        assert len(matrix["driver_cpu_affinity"]) == 8
        assert matrix["orders"] == [list(order) for order in itertools.permutations(("previous", "audit-off", "audit-on"))]
        assert len(matrix["runs"]) == len(matrix["profiles"]) * 18
        assert matrix["artifact_sha256"]["harness"] == manifest[name + "/bench-access-audit.py"]
        assert matrix["artifact_sha256"]["verifier"] == manifest["verify-access-audit.py"]
        if name != "initial":
            assert matrix["artifact_sha256"]["previous"] == context["parent_sha256"]
            assert matrix["artifact_sha256"]["current"] == context["candidate_sha256"]
        else:
            assert matrix["artifact_sha256"]["current"] == context["parent_sha256"]
        identities = set()
        records = 0
        for run in matrix["runs"]:
            assert run["success"] and not run["cleanup_errors"] and run["daemon_exit_code"] is not None
            identity = (run["concurrency"], run["block"], run["position"])
            assert identity not in identities
            identities.add(identity)
            assert run["variant"] == matrix["orders"][run["block"]][run["position"]]
            assert [batch["phase"] for batch in run["batches"]] == ["warmup", "measured"]
            attempted = 0
            for batch in run["batches"]:
                count = matrix["warmup_per_worker"] if batch["phase"] == "warmup" else matrix["requests_per_worker"]
                expected = set(itertools.product(range(run["concurrency"]), range(count)))
                assert batch["success"] and batch["error"] is None
                assert batch["planned"] == batch["passed"] == len(batch["samples"]) == len(expected)
                assert {(sample["worker"], sample["sequence"]) for sample in batch["samples"]} == expected
                assert batch["elapsed_seconds"] > 0
                for sample in batch["samples"]:
                    assert sample["success"] and sample["status"] == 200 and sample["phase"] == batch["phase"]
                    assert math.isfinite(sample["latency_ms"]) and sample["latency_ms"] >= 0
                attempted += len(batch["samples"])
            audited = run["variant"] == "audit-on" or (run["variant"] == "previous" and matrix.get("previous_audit", False))
            assert run.get("audited", audited) == audited
            if audited:
                folder = f"c{run['concurrency']}-b{run['block']}-{run['position']}-{run['variant']}"
                path = directory / folder / "access.jsonl"
                result = verifier.verify(path.read_bytes(), bytes.fromhex("42" * 32))
                assert result == {field: run["audit"][field] for field in result}
                assert result["verified_records"] == attempted * 2 and result["uncompleted_admissions"] == 0
                assert run["audit"]["credentials_absent"]
                assert hashlib.sha256(path.read_bytes()).hexdigest() == run["audit"]["sha256"]
                records += result["verified_records"]
        summary = analyzer.analyze(matrix)
        assert summary == json.loads((directory / "summary.json").read_text())
        assert summary["cleanup_verified"] and summary["cohort_success"]
        assert not summary["competitor_win_established"] and summary["audit_records_verified"] == records
        summaries[name] = summary
        for field in totals:
            totals[field] += summary[field]
    assert totals == {"total_attempted": 204516, "total_passed": 204516, "audit_records_verified": 187416}
    assert all(aggregate[field] == value for field, value in totals.items())
    assert aggregate["runtime_change_adopted"] and not aggregate["competitor_win_established"]
    for name in ("audited", "repeat"):
        for profile in summaries[name]["profiles"]:
            comparison = profile["comparisons"]["audit-on_versus_previous"]
            assert comparison["complete_pairs"] == 6
            if profile["concurrency"] > 1:
                assert comparison["candidate_faster_mean_pairs"] == 6
                assert comparison["median_paired_throughput_ratio"] > 3
    for name in ("audited", "single"):
        assert summaries[name]["profiles"][0]["comparisons"]["audit-on_versus_previous"]["candidate_faster_mean_pairs"] == 2
    for name in ("windows-core-tests.log", "core-tests.log"):
        assert "test result: ok. 14 passed; 0 failed;" in (root / name).read_text()
    for name in ("windows-cluster-tests.log", "cluster-tests.log"):
        text = (root / name).read_text()
        assert "test result: ok. 41 passed; 0 failed;" in text and "test result: ok. 23 passed; 0 failed;" in text
    for name in ("windows-clippy.log", "clippy.log"):
        text = (root / name).read_text()
        assert "Finished" in text and "error:" not in text
    native = json.loads((root / "native/report.json").read_text())
    assert native["success"] and len(native["checks"]) == 13 and native["owned_processes_stopped"]
    assert native["artifact_unchanged"] and not native["cleanup_errors"]
    assert native["control_plane_sha256"] == context["candidate_sha256"]
    assert native["fixture_sha256"] == manifest["check-access-audit.py"]
    assert verifier.verify((root / "native/access.jsonl").read_bytes(), bytes.fromhex("42" * 32)) == native["audit"]
    partial = (root / "native/partial-storage-access.jsonl").read_bytes()
    assert hashlib.sha256(partial).hexdigest() == native["partial_storage_fault"]["sha256"]
    assert partial[:-64] == (root / "native/access.jsonl").read_bytes()
    try:
        verifier.verify(partial, bytes.fromhex("42" * 32))
    except ValueError:
        pass
    else:
        raise AssertionError("partial audit tail verified")
    kvm = json.loads((root / "kvm.json").read_text())
    assert kvm["success"] and len(kvm["cases"]) == 22 and all(case["success"] for case in kvm["cases"])
    assert not kvm["cleanup_errors"] and kvm["remaining_sandboxes"] == 0
    assert len(kvm["owned_processes_stopped"]) == 22
    assert all(process["exit_code"] is not None for process in kvm["owned_processes_stopped"])
    assert kvm["artifact_sha256"]["control-plane"] == context["candidate_sha256"]
    assert kvm["artifact_sha256"]["coordinator"] == manifest["e2e-tcp-tunnel.py"]
    result = verifier.verify((root / "kvm-access.jsonl").read_bytes(), bytes.fromhex("42" * 32))
    assert result == {field: kvm["access_audit"][field] for field in result}
    assert result["verified_records"] == 202 and result["uncompleted_admissions"] == 0
    print(json.dumps(dict(archive_verified=True, **totals, runtime_change_adopted=True, competitor_win_established=False)))


if __name__ == "__main__":
    main()
