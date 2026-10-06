#!/usr/bin/env python3
"""Resolve a retained guest RIP using the exact benchmark kernel in an owned VM."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["firecracker", "kernel", "initrd", "failure-report", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--address", required=True)
    args = parser.parse_args()
    require(re.fullmatch(r"[0-9a-f]{16}", args.address), "requires a 16-digit lowercase hex address")
    require(not args.output.exists(), "output exists; retain earlier observations")
    spec = importlib.util.spec_from_file_location("fc", Path(__file__).with_name("bench-firecracker-local.py"))
    fc = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fc)
    paths = {name: getattr(args, name) for name in ["firecracker", "kernel", "initrd", "failure_report"]}
    paths.update(coordinator=Path(__file__), harness=Path(fc.__file__))
    identities = {name: digest(path) for name, path in paths.items()}
    failure = json.loads(args.failure_report.read_text())
    require(failure["artifact_sha256"]["kernel"] == identities["kernel"], "kernel differs from failure report")
    require(failure["artifact_sha256"]["initrd"] == identities["initrd"], "initrd differs from failure report")
    require(args.address in args.failure_report.read_text(), "address absent from retained failure evidence")
    original = fc.rpc
    observed = {}
    # Addresses share a fixed-width lowercase representation, so lexical
    # comparison gives a bounded predecessor/successor query without awk's
    # floating point conversion losing low address bits.
    command = "awk '$1 <= \"" + args.address + "\" {previous=$0; next} {print previous; print; exit}' /proc/kallsyms"

    def probe(stream, request_id, body):
        result = original(stream, request_id, body)
        if request_id == 2 and body.get("kind") == "exec":
            observed.update(command=command, response=original(stream, 3, {
                "kind": "exec", "program": "/bin/sh", "args": ["-c", command], "timeout_ms": 10000}))
        return result

    fc.rpc = probe
    args.timeout = 30
    try:
        sample = fc.sample(args, 0)
    finally:
        fc.rpc = original
    report = {"diagnostic_only": True, "performance_comparison": False,
        "address": args.address, "artifact_sha256": identities, "sample": sample,
        "symbol_query": observed, "success": False,
        "limitations": ["Symbol mapping from a separately booted identical kernel; not failed-guest execution tracing",
            "A function identity alone does not explain a readiness timeout",
            "The extra command is included in sample timing; no scored latency claim"]}
    response = observed.get("response", {})
    lines = response.get("stdout", "").splitlines()
    valid = response.get("exit_code") == 0 and not response.get("timed_out") and not response.get("truncated")
    matches = [re.fullmatch(r"([0-9a-f]{16}) ([A-Za-z]) (\S+)", line) for line in lines]
    if valid and len(matches) == 2 and all(matches):
        lower, upper = [match.groups() for match in matches]
        if int(lower[0], 16) <= int(args.address, 16) < int(upper[0], 16):
            report["mapping"] = {"symbol": lower[2], "symbol_address": lower[0],
                "offset": int(args.address, 16) - int(lower[0], 16), "next_symbol": upper[2], "next_address": upper[0]}
    report["artifacts_unchanged"] = all(digest(path) == identities[name] for name, path in paths.items())
    report["success"] = bool(report.get("mapping")) and sample["success"] and sample["cleanup_success"] and report["artifacts_unchanged"]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({name: report[name] for name in ["success", "address", "sample", "artifacts_unchanged"]}))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
