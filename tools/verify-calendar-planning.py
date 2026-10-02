#!/usr/bin/env python3
"""Verify isolated calendar-planning samples, not a competitor performance win."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", type=Path)
parser.add_argument("--candidate", type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
m = json.loads((root / "calendar-planning-manifest.json").read_text())
for name, key in [("calendar-planning-run-1.json", "report_sha256"),
                  ("calendar-planning-comparison.json", "comparison_sha256"),
                  ("calendar-planning-source.rs", "source_sha256"),
                  ("calendar-planning-utc.patch", "candidate_patch_sha256")]:
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == m[key]
r = json.loads((root / "calendar-planning-comparison.json").read_text())
assert r["binary_sha256"] == m["binary_sha256"]
assert r["binary_sha256"]["baseline"] != r["binary_sha256"]["candidate"]
assert r["order"] == ["baseline", "candidate", "candidate", "baseline"]
assert len(r["runs"]) == m["runs"] == 4
expected = None
for run in r["runs"]:
    report = run["report"]
    assert report["timezone_database_version"] == m["timezone_database_version"]
    assert report["samples_per_case"] == 9 and len(report["rows"]) == 6
    signature = []
    for row in report["rows"]:
        assert len(row["planning_ms"]) == 9 and all(v > 0 for v in row["planning_ms"])
        assert 0 < row["occurrences"] <= row["limit"]
        assert report["first_ms"] <= row["first_occurrence_ms"] <= row["last_occurrence_ms"]
        signature.append([row[k] for k in ["expression", "timezone", "limit", "occurrences", "first_occurrence_ms", "last_occurrence_ms"]])
    if expected is None:
        expected = signature
    assert signature == expected
for name in ["baseline", "candidate"]:
    path = getattr(args, name)
    if path:
        assert hashlib.sha256(path.read_bytes()).hexdigest() == m["binary_sha256"][name]
print("Verified four matched calendar planner runs and archive hashes; no competitor score.")
