"""Verify archived transfer rows, artifact identities and exact evidence bytes."""
import hashlib
import json
import math
from pathlib import Path

root = Path(__file__).resolve().parent
manifest = json.loads((root / "tcp-perf-manifest.json").read_text())
for name, expected in manifest["files"].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
for path in root.glob("tcp-perf-*.json"):
    if path.name.endswith(("manifest.json", "build.json")):
        continue
    report = json.loads(path.read_text())
    if "rows" not in report:
        continue
    for row in report["rows"]:
        if row["success"]:
            assert row["transaction_ms"] >= row["transfer_ms"] > 0
            assert len(row["payload_sha256"]) == 64
        else:
            assert "failure_phase" in row and "aggregate_payload_mib_s" not in row
    if report["success"]:
        assert all(row["success"] for row in report["rows"])
        assert len(report["rows"]) == report["pairs"] * 2 * report["rounds_per_size_per_guest"] * 2
        assert report["remaining_sandboxes"] == 0 and report["node_stopped"]
        assert all(row["success"] and row["cleanup_success"] for row in report["preparation"])
        assert report["artifacts_unchanged"]
        assert len(report["preparation"]) == report["pairs"] * 2
        for key, summary in report["summaries"].items():
            engine, size = key.rsplit("-", 1)
            rows = [row for row in report["rows"] if row["engine"] == engine and row["payload_bytes"] == int(size)]
            for field, scores in summary.items():
                values = sorted(row[field] for row in rows)
                assert scores["n"] == len(values)
                for percentile in (50, 95, 99):
                    assert math.isclose(scores[f"p{percentile}"], values[math.ceil(percentile / 100 * len(values)) - 1])
        for pair in range(report["pairs"]):
            a = [row["payload_sha256"] for row in report["rows"] if row["pair"] == pair and row["engine"] == "hypermachine"]
            b = [row["payload_sha256"] for row in report["rows"] if row["pair"] == pair and row["engine"] == "firecracker"]
            assert a == b
print(f"Verified {len(manifest['files'])} evidence files and transfer accounting")
