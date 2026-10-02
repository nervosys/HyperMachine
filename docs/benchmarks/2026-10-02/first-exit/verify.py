"""Verify current-kernel symbols and diagnostic first-exit evidence."""
import hashlib
import importlib.util
import json
from pathlib import Path

root = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, root / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


manifest = json.loads((root / "manifest.json").read_text())
for name, expected in manifest.items():
    path = root / name
    require(path.resolve().is_relative_to(root), "manifest path escaped archive")
    require(digest(path) == expected, "hash mismatch: " + name)
report = json.loads((root / "c100.json").read_text())
matrix = json.loads((root / "matrix.json").read_text())
context = json.loads((root / "build-context.json").read_text())
require(matrix["success"] and matrix["diagnostic_only"] and matrix["artifacts_unchanged"], "matrix failed")
require(len(matrix["profiles"]) == 1 and matrix["profiles"][0]["sha256"] == digest(root / "c100.json"), "matrix mismatch")
require(matrix["profiles"][0]["exit_code"] == 0 and not matrix["profiles"][0]["timed_out"], "diagnostic did not finish")
require(len(matrix["driver_cpu_affinity"]) == 8 and not matrix["added_CPU_load"], "affinity/load mismatch")
require(report["success"] and report["diagnostic_only"] and report["artifacts_unchanged"], "cohort failed")
require(not report["setup_error"] and not report["cleanup_errors"] and report["remaining_sandbox_count"] == 0, "cleanup failed")
require(report["cpu_count"] == 1 and report["memory_mb"] == 1024, "resource mismatch")
require(report["guest_readiness_timeout_s"] == {"hypermachine":15, "firecracker":15}, "deadline mismatch")
require([(b["engine"], b["pair"]) for b in report["batches"]] == [("hypermachine",0),("firecracker",0),("firecracker",1),("hypermachine",1)], "batch order mismatch")
rows = [row for batch in report["batches"] for row in batch["samples"]]
require(len(rows) == 400 and all(row["success"] and row["cleanup_success"] for row in rows), "attempt failure")
require(all(len(batch["samples"]) == 100 and batch["success"] for batch in report["batches"]), "batch incomplete")
require(report["artifact_sha256"]["hypermachine"] == context["daemon_sha256"], "daemon mismatch")
require(digest(root / "compiled-vm.rs") == context["overlay_sha256"], "source mismatch")
require(report["diagnostic_sha256"] == manifest["diagnose-concurrent-startup.py"], "wrapper mismatch")
parser = module("diagnostic", "diagnose-concurrent-startup.py")
log = report["cold_readiness_log"]
require(parser.first_exit_kinds(log) == report["first_exit_kinds"], "first-exit parser mismatch")
require(parser.dispatch_stages(log) == report["dispatch_stages_ms"], "dispatch parser mismatch")
require(parser.cold_stages(log) == report["cold_readiness_stages_ms"], "readiness parser mismatch")
names = {row["sandbox_id"] for batch in report["batches"] if batch["engine"] == "hypermachine" for row in batch["samples"]}
require(len(names) == 200 and names == set(report["first_exit_kinds"]), "first exit IDs incomplete")
require(names == set(report["dispatch_stages_ms"]) == set(report["cold_readiness_stages_ms"]), "phase IDs incomplete")
require(all(value == {"kind":"io_out", "io_port":3320} for value in report["first_exit_kinds"].values()), "unexpected first exit")
analyzer = module("analysis", "analyze-first-exit.py")
require(analyzer.analyze(root / "c100.json") == json.loads((root / "analysis.json").read_text()), "analysis mismatch")
symbol = json.loads((root / "kernel-symbol.json").read_text())
failure_path = root.parent / "current-native-engines/c100.json"
require(digest(failure_path) == symbol["artifact_sha256"]["failure_report"], "linked failure changed")
failure = json.loads(failure_path.read_text())
old_rows = [row for batch in failure["batches"] if batch["engine"] == "hypermachine" for row in batch["samples"]]
require(sum(not row["success"] for row in old_rows) == 100 and symbol["address"] in failure_path.read_text(), "retained failure mismatch")
for name in ["kernel", "initrd"]:
    require(symbol["artifact_sha256"][name] == failure["artifact_sha256"][name] == report["artifact_sha256"][name], "symbol provenance mismatch")
require(symbol["success"] and symbol["artifacts_unchanged"] and symbol["sample"]["cleanup_success"], "symbol probe failed")
require(symbol["artifact_sha256"]["coordinator"] == manifest["diagnose-kernel-symbol.py"], "symbol coordinator mismatch")
require(symbol["artifact_sha256"]["harness"] == manifest["bench-firecracker-local.py"], "symbol harness mismatch")
require(symbol["mapping"] == {"symbol":"default_idle", "symbol_address":"ffffffff81eda950", "offset":15,
    "next_symbol":"__pfx_mwait_idle", "next_address":"ffffffff81eda970"}, "symbol mapping mismatch")
require(symbol["symbol_query"]["response"]["stdout"] == "ffffffff81eda950 T default_idle\nffffffff81eda970 t __pfx_mwait_idle\n", "raw symbols mismatch")
require(context["provisional_boot_excluded"], "provisional boot used")
require(context["clean_boot_sha256"] == context["base_context"]["committed_core_sha256"], "boot source mismatch")
require("2294 passed; 0 failed; 2 ignored" in (root / "core-tests-port.log").read_text(), "core tests incomplete")
require("Finished `release`" in (root / "build-final.log").read_text(), "release build incomplete")
require("Finished `dev`" in (root / "windows-check.log").read_text(), "Windows check incomplete")
require("Finished `dev`" in (root / "clippy.log").read_text(), "Clippy incomplete")
print(json.dumps({"archive_verified":True, "attempts":400, "first_exit_records":200, "current_kernel_symbol":"default_idle+15"}))
