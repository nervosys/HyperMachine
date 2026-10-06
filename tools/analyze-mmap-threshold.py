#!/usr/bin/env python3
"""Validate static-threshold environment isolation and matched restore evidence."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import statistics


def require(value, message):
    if not value:
        raise ValueError(message)


def analyze(report, directory):
    spec = importlib.util.spec_from_file_location("threshold_prepared_analysis", directory / "analyze-prepared-engines.py")
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    require(report["same_binary"] is True and report["artifacts_unchanged"] is True
            and report["runtime_change_adopted"] is False and report["managed_competitor_win_established"] is False,
            "input or adoption claim differs")
    require(report["intervention"] == "static_glibc_mmap_threshold_131072"
            and report["candidate_environment"] == {"MALLOC_MMAP_THRESHOLD_": "131072"}
            and report["libc"][0] == "glibc", "allocator policy differs")
    require(type(report["pairs"]) is int and 1 <= report["pairs"] <= 8
            and type(report["concurrency"]) is int and 1 <= report["concurrency"] <= 100,
            "invalid scored profile")
    require(len(report["cpu_affinity"]) == len(set(report["cpu_affinity"])) == 8, "host CPU count differs")
    require(len(report["runs"]) == report["pairs"] * 2, "planned outer run missing")
    require(all(re.fullmatch(r"[0-9a-f]{64}", value) for value in report["artifact_sha256"].values()), "invalid input hash")
    require(report["success"] == all(row["success"] for row in report["runs"]), "outer success differs")
    cohorts = []
    metrics = {variant: {"samples": [], "controls": [], "held": [], "empty": [], "capture": []}
               for variant in ("baseline", "candidate")}
    per_pair = {}
    for index, row in enumerate(report["runs"]):
        pair = index // 2
        order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
        variant = row["variant"]
        require(row["pair"] == pair and variant == order[index % 2], "counterbalance differs")
        require(len(row["activations"]) == 1 and row["owned_daemon_exit_codes"] == [0], "owned daemon lifecycle differs")
        activation = row["activations"][0]
        require(type(activation["pid"]) is int and activation["pid"] > 1
                and activation["executable_sha256"] == report["artifact_sha256"]["daemon"], "running daemon differs")
        environment = dict(activation["environment"])
        require(environment.pop("MALLOC_MMAP_THRESHOLD_", None) == ("131072" if variant == "candidate" else None),
                "running allocator threshold differs")
        require(set(environment) == {"PATH", "HV2_KERNEL", "HV2_INITRD", "RUST_LOG"}
                and environment["RUST_LOG"] == "warn", "unexpected allocator/probe/diagnostic environment")
        raw = row["prepared_report"]
        require(raw.get("diagnostic_only") is False and raw["pairs"] == 2 and raw["concurrency"] == report["concurrency"],
                "nested scoring profile differs")
        require(raw["resource_validation_phase"] == "after_all_timed_attempts", "resource validation changed timing")
        require(raw["driver_cpu_affinity"] == report["cpu_affinity"], "nested host affinity differs")
        require(raw["artifact_sha256"]["hypermachine"] == report["artifact_sha256"]["daemon"], "same executable binding differs")
        require(all(raw["artifact_sha256"][key] == report["artifact_sha256"][key]
                    for key in ("firecracker", "kernel", "initrd", "coordinator", "engines", "firecracker_harness")),
                "nested input differs")
        checked = warm.analyze(raw)
        require(checked["cohort_success"] == row["success"] and checked["cleanup_verified"]
                and checked.get("matched_guest_restore_contract") is True, "guest state, maintenance or cleanup differs")
        item = {"samples": [], "controls": [], "held": [], "empty": []}
        for batch in raw["runs"]:
            values = [sample["ready_ms"] for sample in batch["samples"] if sample["success"] and sample["cleanup_success"]]
            key = "samples" if batch["engine"] == "hypermachine" else "controls"
            metrics[variant][key].extend(values)
            item[key].extend(values)
            if batch["engine"] == "hypermachine" and batch["success"]:
                for key, field in (("held", "held_process_memory_kib"), ("empty", "empty_process_memory_baseline_kib")):
                    value = batch[field]["Pss_kib"] / 1024
                    metrics[variant][key].append(value)
                    item[key].append(value)
        metrics[variant]["capture"].append(raw["preparation"]["hypermachine"]["named_capture_ms"])
        per_pair.setdefault(pair, {})[variant] = item
        cohorts.append({"pair": pair, "variant": variant, "engines": checked["engines"], "cleanup_verified": True})
        per_pair[pair].setdefault("environment", {})[variant] = environment
    planned = report["pairs"] * 2 * report["concurrency"]
    result = {"cohort_success": report["success"], "cleanup_verified": True, "variants": {}, "pairs": [],
        "cohorts": cohorts, "runtime_change_adopted": False, "managed_competitor_win_established": False,
        "latencies_conditional_on_success": True, "memory_conditional_on_complete_batches": True}
    for variant, values in metrics.items():
        samples = values["samples"]
        result["variants"][variant] = {"planned": planned, "passed": len(samples), "failed": planned - len(samples),
            "successful_mean_ms": statistics.mean(samples) if samples else None,
            "successful_p50_ms": warm.percentile(samples, .5), "successful_p99_ms": warm.percentile(samples, .99),
            "median_held_pss_mib": statistics.median(values["held"]) if values["held"] else None,
            "median_empty_pss_mib": statistics.median(values["empty"]) if values["empty"] else None,
            "median_named_capture_ms": statistics.median(values["capture"]),
            "firecracker_control_passed": len(values["controls"]),
            "firecracker_control_mean_ms": statistics.mean(values["controls"]) if values["controls"] else None,
            "firecracker_control_p99_ms": warm.percentile(values["controls"], .99)}
    for pair, values in per_pair.items():
        require(values["environment"]["baseline"] == values["environment"]["candidate"], "paired daemon environment differs")
        baseline, candidate = values["baseline"], values["candidate"]
        complete = all(len(values[v]["samples"]) == report["concurrency"] * 2 for v in ("baseline", "candidate"))
        row = {"pair": pair, "complete": complete}
        if complete:
            row.update(mean_reduction_ms=statistics.mean(baseline["samples"]) - statistics.mean(candidate["samples"]),
                p99_reduction_ms=warm.percentile(baseline["samples"], .99) - warm.percentile(candidate["samples"], .99),
                held_reduction_mib=statistics.median(baseline["held"]) - statistics.median(candidate["held"]),
                empty_reduction_mib=statistics.median(baseline["empty"]) - statistics.median(candidate["empty"]),
                firecracker_control_mean_shift_ms=statistics.mean(candidate["controls"]) - statistics.mean(baseline["controls"]))
        result["pairs"].append(row)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), "preserve previous analysis")
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_bytes()), Path(__file__).parent), indent=2) + "\n")
