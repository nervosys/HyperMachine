#!/usr/bin/env python3
"""Summarize matched native gateway runs without pooling per-peer percentiles."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics


def analyze(baseline, candidate):
    rows = []
    common = None
    for variant, paths in (("baseline", baseline), ("candidate", candidate)):
        for path in paths:
            report = json.loads(path.read_text())
            for key in ("daemon_reaped", "control_and_redis_reaped", "cli_reaped", "native_gateway_reaped"):
                if not report[key]: raise ValueError("incomplete cleanup: " + key)
            if report["guests_remaining"] != 0: raise ValueError("remaining guests")
            blocks = [block for block in report["native_cli_comparison"] if block["path"] == "native"]
            if len(blocks) != 2: raise ValueError("two native blocks required")
            signature = [(b["guest_port"], b["payload_bytes"], b["samples_per_peer"], len(b["peers"])) for b in blocks]
            inputs = {key: value for key, value in report["inputs_sha256"].items() if "gateway" not in Path(key).name}
            identity = (signature, inputs, report["guest_udp_ipv6"], report["local_udp_ipv6"])
            if common is None: common = identity
            if identity != common: raise ValueError("unmatched fixture or measurement scope")
            peers = [peer for block in blocks for peer in block["peers"]]
            rows.append({"variant": variant, "report": str(path), "report_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                         "native_mean_block_rps": statistics.mean(b["roundtrips_per_second"] for b in blocks),
                         "mean_peer_p50_ms": statistics.mean(p["p50_ms"] for p in peers),
                         "mean_peer_p99_ms": statistics.mean(p["p99_ms"] for p in peers)})
    summary = {}
    for metric in ("native_mean_block_rps", "mean_peer_p50_ms", "mean_peer_p99_ms"):
        old = statistics.mean(r[metric] for r in rows if r["variant"] == "baseline")
        new = statistics.mean(r[metric] for r in rows if r["variant"] == "candidate")
        summary[metric] = {"baseline": old, "candidate": new, "candidate_change_percent": (new / old - 1) * 100}
    return {"runs": rows, "summary": summary, "scope": "Development-profile, unpinned owned KVM; means of block rates and per-peer percentiles, not pooled percentiles or statistical confidence; not competitor evidence."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, action="append", required=True)
    parser.add_argument("--candidate", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if len(args.baseline) < 2 or len(args.candidate) < 2: parser.error("at least two runs per variant required")
    result = analyze(args.baseline, args.candidate)
    with args.output.open("x") as stream: json.dump(result, stream, indent=2); stream.write("\n")


if __name__ == "__main__": main()
