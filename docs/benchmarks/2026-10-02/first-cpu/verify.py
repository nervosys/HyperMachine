"""Verify preserved first backend-call CPU measurements and missing-clock failure."""
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
    require(path.resolve().is_relative_to(root), "manifest escaped root")
    require(digest(path) == expected, "hash mismatch: " + name)
matrix = json.loads((root / "matrix.json").read_text())
context = json.loads((root / "build-context.json").read_text())
require(matrix["success"] and matrix["artifacts_unchanged"] and matrix["diagnostic_only"], "matrix failed")
require([row["concurrency"] for row in matrix["profiles"]] == [1,8,100], "profile mismatch")
require(len(matrix["driver_cpu_affinity"]) == 8 and not matrix["added_CPU_load"], "host/load mismatch")
require(digest(root / "compiled-vm.rs") == context["overlay_sha256"] and context["provisional_boot_excluded"], "source mismatch")
require(context["clean_boot_sha256"] == context["base_context"]["clean_boot_sha256"], "boot mismatch")
parser = module("diagnostic", "diagnose-concurrent-startup.py")
analyzer = module("analyzer", "analyze-first-cpu.py")
analysis = json.loads((root / "analysis.json").read_text())
attempts, cpu_count = 0, 0
for item in matrix["profiles"]:
    path = root / item["report"]
    require(digest(path) == item["sha256"] and item["exit_code"] == 0 and not item["timed_out"], "profile did not finish")
    report = json.loads(path.read_text())
    n = item["concurrency"]
    require(report["success"] and report["diagnostic_only"] and report["artifacts_unchanged"], "cohort failed")
    require(not report["setup_error"] and not report["cleanup_errors"] and report["remaining_sandbox_count"] == 0, "cleanup failed")
    require(report["cpu_count"] == 1 and report["memory_mb"] == 1024, "resources changed")
    require(report["guest_readiness_timeout_s"] == {"hypermachine":15,"firecracker":15}, "deadline changed")
    require(report["artifact_sha256"]["hypermachine"] == context["daemon_sha256"], "binary mismatch")
    require(report["diagnostic_sha256"] == manifest["diagnose-concurrent-startup.py"], "wrapper mismatch")
    require([(batch["engine"],batch["pair"]) for batch in report["batches"]] == [
        ("hypermachine",0),("firecracker",0),("firecracker",1),("hypermachine",1)], "batch order changed")
    rows = [row for batch in report["batches"] for row in batch["samples"]]
    require(len(rows) == 4*n and all(row["success"] and row["cleanup_success"] for row in rows), "attempt failed")
    require(all(len(batch["samples"]) == n and batch["success"] for batch in report["batches"]), "batch incomplete")
    names = {row["sandbox_id"] for batch in report["batches"] if batch["engine"] == "hypermachine" for row in batch["samples"]}
    require(len(names) == 2*n and names == set(report["first_backend_cpu"]) == set(report["first_exit_kinds"]), "clock IDs incomplete")
    require(names == set(report["dispatch_stages_ms"]) == set(report["cold_readiness_stages_ms"]), "phase IDs incomplete")
    log = report["cold_readiness_log"]
    require(parser.first_backend_cpu(log) == report["first_backend_cpu"], "CPU parser mismatch")
    require(parser.first_exit_kinds(log) == report["first_exit_kinds"], "exit parser mismatch")
    require(parser.dispatch_stages(log) == report["dispatch_stages_ms"], "dispatch parser mismatch")
    require(parser.cold_stages(log) == report["cold_readiness_stages_ms"], "readiness parser mismatch")
    require(all(value == {"kind":"io_out","io_port":3320} for value in report["first_exit_kinds"].values()), "first exit changed")
    require(analyzer.analyze(path) == analysis[str(n)], "analysis mismatch")
    require(analysis[str(n)]["matched_cpu_ids"] == 2*n and not analysis[str(n)]["unmatched_cpu_ids"], "analysis incomplete")
    attempts += len(rows)
    cpu_count += len(names)
negative_path = root / "missing-clock/c1.json"
negative = json.loads(negative_path.read_text())
negative_matrix = json.loads((root / "missing-clock/matrix.json").read_text())
require(not negative_matrix["success"] and negative_matrix["artifacts_unchanged"], "negative collection unexpectedly passed")
require(negative_matrix["profiles"][0]["exit_code"] == 1 and not negative_matrix["profiles"][0]["timed_out"], "negative collection unfinished")
require(digest(negative_path) == negative_matrix["profiles"][0]["sha256"], "negative cohort mismatch")
require(not negative["success"] and negative["first_cpu_parse_error"] == "missing or invalid first backend CPU clock", "missing clock not refused")
require(negative["first_backend_cpu"] == {} and negative["artifacts_unchanged"], "missing clock substituted")
require(not negative["cleanup_errors"] and negative["remaining_sandbox_count"] == 0, "negative cleanup failed")
require(negative["artifact_sha256"]["hypermachine"] == context["base_context"]["daemon_sha256"], "negative binary mismatch")
negative_rows = [row for batch in negative["batches"] for row in batch["samples"]]
require(len(negative_rows) == 4 and all(row["success"] and row["cleanup_success"] for row in negative_rows), "negative guest attempt failed")
try:
    parser.first_backend_cpu(negative["cold_readiness_log"])
except RuntimeError:
    pass
else:
    raise ValueError("raw missing-clock log accepted")
require("2294 passed; 0 failed; 2 ignored" in (root / "core-tests.log").read_text(), "core tests incomplete")
require("Finished `release`" in (root / "build.log").read_text(), "release build incomplete")
require("Finished `dev`" in (root / "windows-check.log").read_text(), "Windows check incomplete")
require("Finished `dev`" in (root / "clippy.log").read_text(), "Clippy incomplete")
require(attempts == 436 and cpu_count == 218, "study totals changed")
print(json.dumps({"archive_verified":True,"positive_attempts":attempts,"cpu_pairs":cpu_count,"negative_attempts":4}))
