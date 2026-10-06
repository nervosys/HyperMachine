"""Check recorded TCP evidence integrity; does not rerun KVM or score performance."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--staged", action="store_true", help="also verify Git index bytes")
parser.add_argument("--check-original-source", action="store_true",
                    help="also require the working tree to match the original TCP feature build")
args = parser.parse_args()
docs = Path(__file__).resolve().parent
root = docs.parents[2]
sha = lambda data: hashlib.sha256(data).hexdigest()
manifest = json.loads((docs / "tcp-evidence-manifest.json").read_text())
for name, expected in manifest["files"].items():
    assert sha((docs / name).read_bytes()) == expected, name
    if args.staged:
        path = (docs / name).relative_to(root).as_posix()
        assert sha(subprocess.check_output(["git", "show", ":" + path], cwd=root)) == expected, path

for index in range(1, 9):
    report = json.loads((docs / f"tcp-e2e-attempt{index}.json").read_text())
    assert sha((docs / f"tcp-e2e-attempt{index}-source.py").read_bytes()) == report["artifact_sha256"]["coordinator"]
    assert all(row["exit_code"] is not None for row in report["owned_processes_stopped"])

before = json.loads((docs / "tcp-e2e-attempt7.json").read_text())
after = json.loads((docs / "tcp-e2e-attempt8.json").read_text())
assert not before["success"] and before["idle_probe"]["observed_state"] == "paused"
assert after["success"] and after["idle_probe"]["observed_state"] == "running"
assert len(after["cases"]) == 15 and all(row["success"] for row in after["cases"])
assert after["remaining_sandboxes"] == 0 and not after["cleanup_errors"]
assert all(before["artifact_sha256"][name] == value for name, value in after["artifact_sha256"].items() if name != "daemon")
repetitions = next(row for row in after["cases"] if row["name"] == "CLI-graceful-interrupt-after-transfer-repeated")
assert len(repetitions["result"]) == 12
assert all(row["bytes_per_connection"] == 262144 and row["connections"] == 1 for row in repetitions["result"])

for number in range(1, 5):
    build = json.loads((docs / f"tcp-build{number}.json").read_text())
    assert build["success"] and build["canonical_scored_binary_unchanged"]
    for name, expected in build["source_sha256"].items():
        archived = docs / (f"tcp-build{number}-source-" + name.replace("/", "--") + ".txt")
        assert sha(archived.read_bytes()) == expected
        if number == 4 and args.check_original_source:
            assert sha((root / name).read_bytes()) == expected, name
            if args.staged:
                staged = subprocess.check_output(["git", "show", ":" + name], cwd=root)
                assert staged == archived.read_bytes().replace(b"\r\n", b"\n"), name
    for command, label in zip(build["commands"], ["host", "guest"]):
        assert command["exit_code"] == 0
        assert sha((docs / f"tcp-build{number}-{label}-output.txt").read_bytes()) == command["output_sha256"]

assert after["artifact_sha256"]["daemon"] == build["binary_sha256"]["hv2-sandboxd"]
assert after["artifact_sha256"]["cli"] == build["binary_sha256"]["hm"]
assert after["artifact_sha256"]["control-plane"] == build["binary_sha256"]["hv2-control-plane"]
image = json.loads((docs / "tcp-image2-build.json").read_text())
assert after["artifact_sha256"]["initrd"] == image["output_sha256"]
assert image["agent_sha256"] == build["binary_sha256"]["hv2-guest-agentd"]

counts = {}
for platform, daemon_count in [("windows", 32), ("linux", 36)]:
    checked = json.loads((docs / f"tcp-checks-{platform}.json").read_text())
    assert checked["success"] and len(checked["results"]) == 5
    assert sha((docs / "tcp-check-coordinator.py").read_bytes()) == checked["coordinator_sha256"]
    expected_counts = [[2], [16, 31], [8], [daemon_count], []]
    for result, expected in zip(checked["results"], expected_counts):
        assert result["exit_code"] == 0
        assert sha(result["output_utf8"].encode("utf8")) == result["output_sha256"]
        observed = sorted(map(int, re.findall(r"test result: ok\. (\d+) passed; 0 failed", result["output_utf8"])))
        assert observed == expected, (platform, observed, expected)
    assert checked["results"][-1]["command"][-3:] == ["--", "-D", "warnings"]
    counts[platform] = sum(sum(group) for group in expected_counts)

print(json.dumps({"archive_files_verified": len(manifest["files"]), "recorded_kvm_checks_passed": 15,
                  "matched_idle_states": ["paused", "running"], "recorded_platform_test_counts": counts,
                  "staged_bytes_verified": args.staged, "performance_claim": False}))
