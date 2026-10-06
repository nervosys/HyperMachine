"""Verify concurrent TCP rows, retained failures and guest backlog controls."""
import argparse
from functools import lru_cache
import hashlib
import itertools
import json
import math
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--staged", action="store_true")
parser.add_argument("--current", action="store_true", help="also verify current guest source")
args = parser.parse_args()
docs = Path(__file__).resolve().parent
root = docs.parents[2]
sha = lambda data: hashlib.sha256(data).hexdigest()
load = lambda name: json.loads((docs / name).read_text())
manifest = load("tcp-concurrent-manifest.json")
for name, expected in manifest["files"].items():
    assert sha((docs / name).read_bytes()) == expected, name
    if args.staged:
        assert sha(subprocess.check_output(["git", "show", ":" + (docs / name).relative_to(root).as_posix()], cwd=root)) == expected

payloads = {size: bytes(range(256)) * (size // 256) for size in (4096, 1048576)}
@lru_cache(maxsize=4096)
def payload_hash(pair, round_index, size, client):
    marker = f"pair={pair},round={round_index},size={size},client={client}\n".encode()
    return sha(marker + payloads[size][len(marker):])

def check(report):
    expected = set(itertools.product(("hypermachine", "firecracker"), range(report["pairs"]), range(report["rounds_per_size_per_guest"]), (4096, 1048576), range(report["concurrency"])))
    keys = [(row["engine"], row["pair"], row["round"], row["payload_bytes"], row["client"]) for row in report["rows"]]
    assert len(keys) == len(expected) and set(keys) == expected
    assert report["remaining_sandboxes"] == 0 and report["node_stopped"] and report["artifacts_unchanged"]
    assert not report["cleanup_errors"]
    assert all(row["success"] and row["cleanup_success"] for row in report["preparation"])
    assert report["success"] == all(row["success"] for row in report["rows"])
    for row in report["rows"]:
        if row["success"]:
            assert row["payload_sha256"] == payload_hash(row["pair"], row["round"], row["payload_bytes"], row["client"])
        else:
            assert row["attempted"] and row["failure_phase"] == "handshake"
            assert row["error"] == "connection closed during handshake"
            assert "transaction_ms" not in row and "aggregate_payload_mib_s" not in row
    for key, fields in report["summaries"].items():
        engine, size = key.rsplit("-", 1)
        rows = [row for row in report["rows"] if row["success"] and row["engine"] == engine and row["payload_bytes"] == int(size)]
        for field, summary in fields.items():
            values = sorted(row[field] for row in rows)
            assert summary["n"] == len(values)
            for percentile in (50, 95, 99):
                assert math.isclose(summary[f"p{percentile}"], values[math.ceil(percentile / 100 * len(values)) - 1])

for path in sorted(docs.glob("tcp-concurrent-c*-run-*.json")):
    report = json.loads(path.read_text())
    check(report)
    assert report["artifact_sha256"]["harness"] == sha((docs / "tcp-concurrent-bench-tcp-local.py").read_bytes())
check(load("tcp-concurrent-concurrent-smoke.json"))
initial = load("tcp-concurrent-verification.json")
completion = load("tcp-concurrent-completion.json")
assert not initial["success"] and len(initial["results"]) == 6
assert completion["success"] and len(completion["results"]) == 2
assert initial["coordinator_sha256"] == sha((docs / "tcp-concurrent-verify-tcp-combined.py").read_bytes())
assert completion["coordinator_sha256"] == sha((docs / "tcp-concurrent-complete-tcp-combined.py").read_bytes())
for row in initial["results"] + completion["results"]:
    assert sha(row["output_utf8"].encode()) == row["output_sha256"]
candidate = load("tcp-concurrent-release-build.json")
assert candidate["success"] and candidate["canonical_scored_binary_restored"]
for name, expected in candidate["source_sha256"].items():
    assert sha((docs / ("tcp-concurrent-candidate-source-" + name.replace("/", "--"))).read_bytes()) == expected

build = load("tcp-backlog-build.json")
assert build["success"] and build["source_initrd_unchanged"] and build["exit_code"] == 0
assert build["coordinator_sha256"] == sha((docs / "tcp-concurrent-build-tcp-backlog.py").read_bytes())
for name, expected in build["source_sha256"].items():
    archived = docs / ("tcp-backlog-source-" + name.replace("/", "--"))
    assert sha(archived.read_bytes()) == expected
    if args.current:
        assert (root / name).read_bytes().replace(b"\r\n", b"\n") == archived.read_bytes().replace(b"\r\n", b"\n")
image = load("tcp-backlog-image-verification.json")
assert image["success"] and image["entry_sets_equal"] and image["changed_contents"] == ["bin/hv2-guest-agentd"]
assert not image["changed_metadata"] and image["output_sha256"] == build["output_sha256_initrd"]
assert image["verifier_sha256"] == sha((docs / "tcp-concurrent-verify-tcp-backlog-image.py").read_bytes())
reports = []
for index, mode in enumerate(("backlog4", "backlog128", "backlog128", "backlog4"), 1):
    report = load(f"tcp-backlog-run-{index}-{mode}.json")
    check(report)
    assert report["concurrency"] == 8 and len(report["rows"]) == 960
    assert report["artifact_sha256"]["initrd"] == (build["source_initrd_sha256"] if mode == "backlog4" else build["output_sha256_initrd"])
    assert report["success"] == (mode == "backlog128")
    reports.append(report)
assert all({k:v for k,v in r["artifact_sha256"].items() if k != "initrd"} == {k:v for k,v in reports[0]["artifact_sha256"].items() if k != "initrd"} for r in reports)
assert sum(not row["success"] for r in (reports[0], reports[3]) for row in r["rows"]) == 37
assert all(row["success"] for r in (reports[1], reports[2]) for row in r["rows"])
sequential = load("tcp-backlog-sequential.json")
check(sequential)
assert sequential["success"] and sequential["concurrency"] == 1 and len(sequential["rows"]) == 400
assert sequential["artifact_sha256"] == reports[1]["artifact_sha256"]
e2e = load("tcp-backlog-e2e.json")
assert e2e["success"] and len(e2e["cases"]) == 15 and all(row["success"] for row in e2e["cases"])
assert e2e["artifact_sha256"]["initrd"] == build["output_sha256_initrd"]
assert not e2e["cleanup_errors"] and e2e["remaining_sandboxes"] == 0
assert all(row["exit_code"] is not None for row in e2e["owned_processes_stopped"])
verification = load("tcp-backlog-verification.json")
assert verification["success"] and not verification["all_benchmarks_passed"]
assert verification["coordinator_sha256"] == sha((docs / "tcp-concurrent-verify-tcp-backlog.py").read_bytes())
for row in verification["results"]:
    assert sha(row["output_utf8"].encode()) == row["output_sha256"]
    if row["name"] == "guest-tests":
        assert sorted(map(int,re.findall(r"test result: ok\. (\d+) passed; 0 failed", row["output_utf8"]))) == [0, 2, 15]
windows = load("tcp-backlog-windows.json")
assert windows["success"] and all(row["exit_code"] == 0 for row in windows["results"])
assert windows["coordinator_sha256"] == sha((docs / "tcp-concurrent-check-tcp-backlog-windows.py").read_bytes())
for row in windows["results"]:
    assert sha(row["output_utf8"].encode()) == row["output_sha256"]
print(json.dumps({"archive_files":len(manifest["files"]),"larger_backlog_verified_transfers":1920,"old_backlog_failures_retained":37,"TLS_lifecycle_cases":15,"current_guest_source_checked":args.current}))
