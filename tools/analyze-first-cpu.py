#!/usr/bin/env python3
"""Summarize calling-thread CPU consumption during first backend diagnostics."""
import argparse
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("first_exit", Path(__file__).with_name("analyze-first-exit.py"))
first_exit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(first_exit)


def analyze(path):
    base = first_exit.analyze(path)
    report = json.loads(path.read_text())
    cpu = report["first_backend_cpu"]
    names = {row["sandbox_id"] for batch in report["batches"] if batch["engine"] == "hypermachine"
        for row in batch["samples"] if row["success"] and row["cleanup_success"]}
    matched = sorted(names & cpu.keys())
    values = {"thread_cpu_ms":[], "bracket_wall_ms":[], "elapsed_not_cpu_ms":[], "cpu_fraction":[]}
    for name in matched:
        row = cpu[name]
        consumed = row["first_backend_cpu_ns"]
        wall = row["first_backend_cpu_wall_ns"]
        if type(consumed) is not int or type(wall) is not int or not 0 <= consumed <= wall <= 2**64-1 or not wall:
            raise ValueError("invalid CPU clock pair")
        values["thread_cpu_ms"].append(consumed / 1_000_000)
        values["bracket_wall_ms"].append(wall / 1_000_000)
        values["elapsed_not_cpu_ms"].append((wall - consumed) / 1_000_000)
        values["cpu_fraction"].append(consumed / wall)
    base.update(cpu_clock="Linux CLOCK_THREAD_CPUTIME_ID", matched_cpu_ids=len(matched),
        unmatched_cpu_ids=sorted(names - cpu.keys()), other_cpu_ids=sorted(cpu.keys() - names),
        first_backend_cpu={field:first_exit.summarize(items) for field,items in values.items()})
    base["limitations"].extend([
        "CPU consumption includes all calling-thread CPU charged inside the bracket, not guest decompression alone",
        "Wall minus CPU is elapsed time not charged to this thread; it does not separate scheduling, blocking and nested-host effects",
        "The CPU bracket excludes the preceding diagnostic log and encloses the CPU clock reads",
        "No matching Firecracker first-backend CPU measurement or competitor CPU ranking"])
    return base


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cohort", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists(): parser.error("output already exists")
    args.output.write_text(json.dumps(analyze(args.cohort), indent=2) + "\n")
