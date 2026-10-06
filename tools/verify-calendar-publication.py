"""Verify local calendar batch evidence; no competitor claims."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "docs/benchmarks/2026-10-01"
manifest = json.loads((root / "calendar-publication-manifest.json").read_text())
for name, digest in manifest["files_sha256"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
data = json.loads((root / "calendar-publication-comparison.json").read_text())
assert len(set(data["binary_sha256"].values())) == 2
assert data["order"] == ["baseline", "candidate", "candidate", "baseline"]
assert [run["variant"] for run in data["runs"]] == data["order"]
reference = data["runs"][0]["report"]
assert len(reference["rows"]) == 4
for run in data["runs"]:
    report = run["report"]
    assert report["samples_per_case"] == 5
    for row, expected in zip(report["rows"], reference["rows"], strict=True):
        for key in ["timezone", "limit", "scheduled_utc_ms"]:
            assert row[key] == expected[key], key
        times = row["scheduled_utc_ms"]
        assert 0 < len(times) <= row["limit"]
        assert all(a < b for a, b in zip(times, times[1:]))
        assert all(report["first_ms"] <= t <= report["through_ms"] for t in times)
        assert len(row["publication_ms"]) == 5 and all(t > 0 for t in row["publication_ms"])
print("Calendar publication hashes, ABBA order and all UTC timestamps verified.")
