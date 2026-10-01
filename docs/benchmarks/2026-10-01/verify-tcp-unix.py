"""Verify the rejected Unix relay experiment and its frozen runtime evidence."""
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
parser.add_argument("--production", action="store_true", help="check that production still uses the accepted TCP adapter")
args = parser.parse_args()
docs = Path(__file__).resolve().parent
root = docs.parents[2]
sha = lambda data: hashlib.sha256(data).hexdigest()
load = lambda name: json.loads((docs / name).read_text())
manifest = load("tcp-unix-manifest.json")
for name, expected in manifest["files"].items():
    assert sha((docs / name).read_bytes()) == expected, name
    if args.staged:
        assert sha(subprocess.check_output(["git", "show", ":" + (docs / name).relative_to(root).as_posix()], cwd=root)) == expected

base_payloads = {size:bytes(range(256)) * (size // 256) for size in (4096,1048576)}
@lru_cache(maxsize=4096)
def payload_hash(pair, round_index, size, client):
    marker = f"pair={pair},round={round_index},size={size},client={client}\n".encode()
    return sha(marker + base_payloads[size][len(marker):])

build = load("tcp-unix-release-build.json")
assert build["success"] and build["canonical_scored_binary_restored"]
assert sha(build["output_utf8"].encode()) == build["output_sha256"]
for name, expected in build["source_sha256"].items():
    assert sha((docs / ("tcp-unix-source-" + name.replace("/", "--"))).read_bytes()) == expected
reports = []
for concurrency, pairs, rounds in ((1,20,5),(8,10,3)):
    for index, mode in enumerate(("baseline","candidate","candidate","baseline"),1):
        report = load(f"tcp-unix-c{concurrency}-run-{index}-{mode}.json")
        assert report["success"] and report["artifacts_unchanged"]
        assert report["concurrency"] == concurrency and report["pairs"] == pairs and report["rounds_per_size_per_guest"] == rounds
        expected = set(itertools.product(("hypermachine","firecracker"),range(pairs),range(rounds),(4096,1048576),range(concurrency)))
        keys = [(r["engine"],r["pair"],r["round"],r["payload_bytes"],r["client"]) for r in report["rows"]]
        assert len(keys) == len(expected) and set(keys) == expected
        assert report["remaining_sandboxes"] == 0 and report["node_stopped"] and not report["cleanup_errors"]
        assert all(r["success"] and r["cleanup_success"] for r in report["preparation"])
        assert report["cpu_count"] == 1 and report["memory_mb"] == 1024 and report["host_affinity"] == list(range(8))
        for row in report["rows"]:
            assert row["success"] and row["payload_sha256"] == payload_hash(row["pair"],row["round"],row["payload_bytes"],row["client"])
        for key, fields in report["summaries"].items():
            engine,size = key.rsplit("-",1)
            rows = [r for r in report["rows"] if r["engine"] == engine and r["payload_bytes"] == int(size)]
            for field, summary in fields.items():
                values = sorted(r[field] for r in rows)
                assert summary["n"] == len(values)
                for percentile in (50,95,99):
                    assert math.isclose(summary[f"p{percentile}"],values[math.ceil(percentile/100*len(values))-1])
        assert report["artifact_sha256"]["harness"] == sha((docs / "tcp-unix-bench-tcp-local.py").read_bytes())
        if mode == "candidate":
            assert report["artifact_sha256"]["hypermachine"] == build["binary_sha256"]
        reports.append(report)
assert all({k:v for k,v in r["artifact_sha256"].items() if k != "hypermachine"} == {k:v for k,v in reports[0]["artifact_sha256"].items() if k != "hypermachine"} for r in reports)
verification = load("tcp-unix-verification.json")
assert verification["success"] and len(verification["results"]) == 11
assert verification["coordinator_sha256"] == sha((docs / "tcp-unix-verify-tcp-unix.py").read_bytes())
for row in verification["results"]:
    assert row["exit_code"] == 0 and sha(row["output_utf8"].encode()) == row["output_sha256"]
e2e = load("tcp-unix-e2e.json")
assert e2e["success"] and len(e2e["cases"]) == 15 and all(r["success"] for r in e2e["cases"])
assert e2e["remaining_sandboxes"] == 0 and not e2e["cleanup_errors"]
assert all(r["exit_code"] is not None for r in e2e["owned_processes_stopped"])
assert e2e["artifact_sha256"]["daemon"] == build["binary_sha256"]
assert e2e["artifact_sha256"]["initrd"] == reports[0]["artifact_sha256"]["initrd"]
for platform,daemon_count in (("windows",32),("linux",36)):
    checked = load(f"tcp-unix-checks-{platform}.json")
    assert checked["success"] and len(checked["results"]) == 5
    for row,expected in zip(checked["results"],([2],[16,31],[8],[daemon_count],[])):
        assert row["exit_code"] == 0 and sha(row["output_utf8"].encode()) == row["output_sha256"]
        assert sorted(map(int,re.findall(r"test result: ok\. (\d+) passed; 0 failed",row["output_utf8"]))) == expected
if args.production:
    original = (docs / "tcp-flow-final-compiled-crates--hv2-sandboxd--src--forwards.rs").read_bytes().replace(b"\r\n",b"\n")
    assert (root / "crates/hv2-sandboxd/src/forwards.rs").read_bytes().replace(b"\r\n",b"\n") == original
print(json.dumps({"archive_files":len(manifest["files"]),"verified_transfers":5440,"TLS_lifecycle_cases":15,"prototype_reverted":args.production}))
