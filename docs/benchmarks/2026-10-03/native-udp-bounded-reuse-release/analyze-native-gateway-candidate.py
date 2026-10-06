#!/usr/bin/env python3
"""Summarize matched native gateway runs without pooling per-peer percentiles."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics


def analyze(baseline, candidate):
    rows = []
    common = None
    profiles = set()
    resources = []
    gateway_hashes = {}
    reference_rows = []
    for variant, paths in (("baseline", baseline), ("candidate", candidate)):
        for path in paths:
            report = json.loads(path.read_text())
            for key in ("daemon_reaped", "control_and_redis_reaped", "cli_reaped", "native_gateway_reaped"):
                if not report[key]: raise ValueError("incomplete cleanup: " + key)
            if report["guests_remaining"] != 0: raise ValueError("remaining guests")
            blocks = [block for block in report["native_cli_comparison"] if block["path"] == "native"]
            if len(blocks) != 2: raise ValueError("two native blocks required")
            if [block["path"] for block in report["native_cli_comparison"]]!=["cli","native","native","cli"]:raise ValueError("matched ABBA path order required")
            gateways=[value for key,value in report["inputs_sha256"].items() if "gateway" in Path(key).name]
            if len(gateways)!=1:raise ValueError("one gateway input identity required")
            if variant in gateway_hashes and gateway_hashes[variant]!=gateways[0]:raise ValueError("variant gateway changed")
            gateway_hashes[variant]=gateways[0]
            for block in report["native_cli_comparison"]:
                count=block["samples_per_peer"]
                if count<=0 or not block["peers"]:raise ValueError("empty measured block")
                if {peer["peer"] for peer in block["peers"]}!=set(range(len(block["peers"]))):raise ValueError("duplicate or missing peer")
                for peer in block["peers"]:
                    samples=peer["samples_ms"]
                    if len(samples)!=count or any(not math.isfinite(value) or value<=0 for value in samples):raise ValueError("invalid raw samples")
                    ordered=sorted(samples)
                    if peer["p50_ms"]!=ordered[math.ceil(count*.50)-1] or peer["p99_ms"]!=ordered[math.ceil(count*.99)-1]:raise ValueError("percentiles disagree with raw samples")
                    if not math.isfinite(peer["measured_wall_seconds"]) or peer["measured_wall_seconds"]<=0:raise ValueError("invalid measured duration")
                rate=count*len(block["peers"])/max(peer["measured_wall_seconds"] for peer in block["peers"])
                if not math.isclose(rate,block["roundtrips_per_second"],rel_tol=1e-12):raise ValueError("rate disagrees with measured duration")

            signature = [(b["guest_port"], b["payload_bytes"], b["samples_per_peer"], b["warmups_per_peer"], len(b["peers"])) for b in report["native_cli_comparison"]]
            inputs = {key: value for key, value in report["inputs_sha256"].items() if "gateway" not in Path(key).name}
            profile=report.get("host_build_profile_declared","development")
            if profile not in ("development","release"):raise ValueError("invalid build profile")
            profiles.add(profile)
            memory=report.get("native_resources",[])
            if memory:
                if [row["phase"] for row in memory]!=["after_maximum_udp","after_abba"]:raise ValueError("unmatched memory sampling phases")
                if len({row["gateway_pid"] for row in memory})!=1:raise ValueError("gateway changed during sampling")
                for row in memory:resources.append(dict(row,variant=variant))
            identity = (signature, inputs, report["guest_udp_ipv6"], report["local_udp_ipv6"],profile,
                        report.get("guest_resources_from_inventory"),report.get("host_environment"),bool(memory))
            if common is None: common = identity
            if identity != common: raise ValueError("unmatched fixture or measurement scope")
            reference=[block for block in report["native_cli_comparison"] if block["path"]=="cli"]
            reference_peers=[peer for block in reference for peer in block["peers"]]
            reference_rows.append({"variant":variant,"cli_mean_block_rps":statistics.mean(block["roundtrips_per_second"] for block in reference),
                                   "mean_peer_p50_ms":statistics.mean(peer["p50_ms"] for peer in reference_peers),
                                   "mean_peer_p99_ms":statistics.mean(peer["p99_ms"] for peer in reference_peers)})
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
    reference_summary={}
    for metric in ("cli_mean_block_rps","mean_peer_p50_ms","mean_peer_p99_ms"):
        old=statistics.mean(row[metric] for row in reference_rows if row["variant"]=="baseline")
        new=statistics.mean(row[metric] for row in reference_rows if row["variant"]=="candidate")
        reference_summary[metric]={"baseline":old,"candidate":new,"candidate_change_percent":(new/old-1)*100}
    memory_summary={}
    for phase in ("after_maximum_udp","after_abba"):
        if not resources:break
        memory_summary[phase]={}
        for metric in ("rss_kib","pss_kib","private_kib","anonymous_kib"):
            old=statistics.mean(row[metric] for row in resources if row["variant"]=="baseline" and row["phase"]==phase)
            new=statistics.mean(row[metric] for row in resources if row["variant"]=="candidate" and row["phase"]==phase)
            memory_summary[phase][metric]={"baseline":old,"candidate":new,"candidate_change_percent":(new/old-1)*100 if old else None}
    return {"runs": rows, "summary": summary,"memory_summary":memory_summary,"cli_reference_summary":reference_summary,
            "scope": next(iter(profiles)).capitalize()+"-profile, unpinned owned KVM; means of block rates and per-peer percentiles, not pooled percentiles or statistical confidence; memory is gateway-only snapshot KiB, not peak or whole-stack memory; not competitor evidence."}


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
