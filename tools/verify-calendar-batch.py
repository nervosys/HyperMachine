"""Verify local calendar batch evidence; no competitor claims."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
manifest = json.loads((root / "calendar-batch-manifest.json").read_text())
for name, digest in manifest["files_sha256"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
data = json.loads((root / "calendar-batch-comparison.json").read_text())
assert len(set(data["binary_sha256"].values())) == 2
assert data["order"] == ["baseline", "candidate", "candidate", "baseline"]
assert [run["variant"] for run in data["runs"]] == data["order"]
reference = data["runs"][0]["report"]
assert len(reference["rows"]) == 6
for run in data["runs"]:
    report = run["report"]
    assert report["timezone_database_version"] == reference["timezone_database_version"]
    assert report["samples_per_case"] == 9
    for row, expected in zip(report["rows"], reference["rows"], strict=True):
        for key in ["expression", "timezone", "limit", "horizon_days", "scheduled_utc_ms"]:
            assert row[key] == expected[key], key
        times = row["scheduled_utc_ms"]
        assert len(times) == row["occurrences"] and 0 < len(times) <= row["limit"]
        assert all(a < b for a, b in zip(times, times[1:]))
        assert times[0] == row["first_occurrence_ms"] and times[-1] == row["last_occurrence_ms"]
        assert all(report["first_ms"] <= t <= report["first_ms"] + row["horizon_days"] * 86400000 for t in times)
        assert len(row["planning_ms"]) == 9 and all(t > 0 for t in row["planning_ms"])
print("Calendar batch hashes, ABBA order and all UTC timestamps verified.")
