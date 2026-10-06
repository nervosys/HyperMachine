"""Verify frozen TCP socket investigation, benchmark and lifecycle evidence."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--staged", action="store_true")
parser.add_argument("--current", action="store_true", help="also check current production source against the final release build")
args = parser.parse_args()
docs = Path(__file__).resolve().parent
root = docs.parents[2]
sha = lambda data: hashlib.sha256(data).hexdigest()
manifest = json.loads((docs / "tcp-flow-manifest.json").read_text())
for name, expected in manifest["files"].items():
    assert sha((docs / name).read_bytes()) == expected, name
    if args.staged:
        assert sha(subprocess.check_output(["git", "show", ":" + (docs / name).relative_to(root).as_posix()], cwd=root)) == expected

def load(name):
    return json.loads((docs / name).read_text())

def scored(report, count):
    assert report["success"] and len(report["rows"]) == count
    assert report["remaining_sandboxes"] == 0 and report["node_stopped"] and report["artifacts_unchanged"]
    assert all(row["success"] and row["cleanup_success"] for row in report["preparation"])
    assert all(row["success"] for row in report["rows"])
    assert count == report["pairs"] * 2 * report["rounds_per_size_per_guest"] * 2
    for pair in range(report["pairs"]):
        hashes = [[row["payload_sha256"] for row in report["rows"] if row["pair"] == pair and row["engine"] == engine] for engine in ("hypermachine", "firecracker")]
        assert hashes[0] == hashes[1]
    for key, fields in report["summaries"].items():
        engine, size = key.rsplit("-", 1)
        rows = [row for row in report["rows"] if row["engine"] == engine and row["payload_bytes"] == int(size)]
        for field, summary in fields.items():
            values = sorted(row[field] for row in rows)
            assert summary["n"] == len(values)
            for percentile in (50, 95, 99):
                assert math.isclose(summary[f"p{percentile}"], values[math.ceil(percentile / 100 * len(values)) - 1])

fixture_checks = load("tcp-flow-fixture-checks.json")
assert fixture_checks["success"] and len(fixture_checks["results"]) == 4
assert fixture_checks["coordinator_sha256"] == sha((docs / "tcp-flow-diagnose-tcp-fixture.py").read_bytes())
for image_name, source_name in (("measured-image-build", "measured-fixture.c"), ("image-build", "tcp-fixture.c")):
    image = load(f"tcp-flow-fixture-{image_name}.json")
    assert image["fixture_source_sha256"] == sha((docs / ("tcp-flow-" + source_name)).read_bytes())
    assert image["source_initrd_unchanged"]
    assert image["builder_sha256"] == sha((docs / "tcp-flow-build-tcp-fixture-diagnostic.py").read_bytes())
original = load("tcp-flow-api-original-tcp-evidence-manifest.json")
assert original["files"]["verify-tcp.py"] == sha((docs / "tcp-flow-original-verify-tcp.py").read_bytes())
for name, expected in original["files"].items():
    if name != "verify-tcp.py":
        assert sha((docs / name).read_bytes()) == expected, name
for index, mode in enumerate(("default", "nodelay", "nodelay", "default"), 1):
    report = load(f"tcp-flow-fixture-run-{index}-{mode}.json")
    scored(report, 400)
    assert report["fixture_tcp_nodelay"] == (mode == "nodelay")
    assert report["artifact_sha256"]["harness"] == sha((docs / "tcp-flow-measured-harness.py").read_bytes())
mode_checks = load("tcp-flow-fixture-mode-checks.json")
assert mode_checks["success"]
assert mode_checks["coordinator_sha256"] == sha((docs / "tcp-flow-check-tcp-fixture-mode.py").read_bytes())
for mode in ("default", "nodelay"):
    scored(load(f"tcp-flow-fixture-mode-{mode}.json"), 16)
unsupported = load("tcp-flow-fixture-mode-unsupported.json")
assert not unsupported["success"] and not unsupported["rows"]
assert unsupported["remaining_sandboxes"] == 0 and unsupported["node_stopped"]
assert all(not row["success"] and row["cleanup_success"] and "mode unsupported" in row["error"] for row in unsupported["preparation"])
scored(load("tcp-flow-fixture-trace.json"), 8)

build = load("tcp-flow-final-release-build.json")
assert build["success"] and build["canonical_scored_binary_restored"]
for name, expected in build["source_sha256"].items():
    archived = docs / ("tcp-flow-final-compiled-" + name.replace("/", "--"))
    assert sha(archived.read_bytes()) == expected, name
    if args.current:
        assert (root / name).read_bytes().replace(b"\r\n", b"\n") == archived.read_bytes().replace(b"\r\n", b"\n"), name

reports = []
for index, mode in enumerate(("baseline", "candidate", "candidate", "baseline"), 1):
    report = load(f"tcp-flow-final-run-{index}-{mode}.json")
    scored(report, 400)
    assert report["artifact_sha256"]["harness"] == sha((docs / "tcp-flow-bench-tcp-local.py").read_bytes())
    assert report["host_affinity"] == list(range(8)) and not report["fixture_tcp_nodelay"]
    if mode == "candidate":
        assert report["artifact_sha256"]["hypermachine"] == build["binary_sha256"]
    reports.append(report)
assert all({k:v for k,v in report["artifact_sha256"].items() if k != "hypermachine"} == {k:v for k,v in reports[0]["artifact_sha256"].items() if k != "hypermachine"} for report in reports)

e2e = load("tcp-flow-final-e2e.json")
assert e2e["success"] and len(e2e["cases"]) == 15 and all(row["success"] for row in e2e["cases"])
assert e2e["remaining_sandboxes"] == 0 and not e2e["cleanup_errors"]
assert all(row["exit_code"] is not None for row in e2e["owned_processes_stopped"])
assert e2e["artifact_sha256"]["daemon"] == build["binary_sha256"]
assert e2e["artifact_sha256"]["control-plane"] == build["control_plane_sha256"]
assert e2e["artifact_sha256"]["kernel"] == reports[0]["artifact_sha256"]["kernel"]
assert e2e["artifact_sha256"]["initrd"] == reports[0]["artifact_sha256"]["initrd"]
inputs = load("tcp-flow-final-verification-inputs.json")
for name, expected in inputs["source_sha256"].items():
    assert sha((docs / ("tcp-flow-" + Path(name).name)).read_bytes()) == expected, name
for platform, daemon_count in (("windows", 32), ("linux", 36)):
    checks = load(f"tcp-flow-checks-{platform}.json")
    assert checks["success"] and len(checks["results"]) == 5
    assert checks["coordinator_sha256"] == sha((docs / "tcp-flow-check-tcp-final.py").read_bytes())
    for row, expected in zip(checks["results"], ([2], [16, 31], [8], [daemon_count], [])):
        assert row["exit_code"] == 0 and sha(row["output_utf8"].encode()) == row["output_sha256"]
        assert sorted(map(int, re.findall(r"test result: ok\. (\d+) passed; 0 failed", row["output_utf8"]))) == expected
verification = load("tcp-flow-final-verification.json")
assert verification["success"] and len(verification["results"]) == 7
assert verification["coordinator_sha256"] == sha((docs / "tcp-flow-verify-tcp-api-nodelay.py").read_bytes())
for row in verification["results"] + fixture_checks["results"] + mode_checks["results"]:
    assert sha(row["output_utf8"].encode()) == row["output_sha256"]
print(json.dumps({"evidence_files":len(manifest["files"]), "fixture_diagnostic_transfers":1600, "final_matched_transfers":1600, "TLS_lifecycle_cases":15, "current_source_checked":args.current}))
