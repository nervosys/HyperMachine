#!/usr/bin/env python3
"""Counterbalance protected inventory throughput with durable audit on and off."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.client
import importlib.util
import itertools
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import threading
import time

spec = importlib.util.spec_from_file_location("audit_verifier", Path(__file__).with_name("verify-access-audit.py"))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def counters(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    ticks = os.sysconf("SC_CLK_TCK")
    io = dict(line.split(":", 1) for line in Path(f"/proc/{pid}/io").read_text().splitlines())
    return {"cpu_seconds": (int(fields[11]) + int(fields[12])) / ticks,
        "written_storage_bytes": int(io["write_bytes"])}


def batch(api_port, key, concurrency, per_worker, phase):
    timing = {}
    barrier = threading.Barrier(concurrency + 1, action=lambda: timing.update(started=time.perf_counter()))
    rows = []
    lock = threading.Lock()
    def worker(index):
        connection = http.client.HTTPConnection("127.0.0.1", api_port, timeout=10)
        try:
            # Connect before the barrier so this workload measures repeated
            # API calls, with the same persistent connection policy everywhere.
            connection.connect()
            barrier.wait(timeout=20)
            for sequence in range(per_worker):
                started = time.perf_counter()
                row = {"worker": index, "sequence": sequence, "phase": phase, "success": False}
                try:
                    connection.request("GET", "/sandboxes", headers={"x-api-key": key})
                    response = connection.getresponse()
                    body = response.read()
                    row["status"] = response.status
                    row["success"] = response.status == 200 and json.loads(body) == []
                    if not row["success"]:
                        row["error"] = "unexpected status or nonempty inventory"
                except Exception as error:
                    row["error"] = type(error).__name__
                row["latency_ms"] = (time.perf_counter() - started) * 1000
                with lock: rows.append(row)
        except Exception as error:
            with lock: rows.append({"worker": index, "phase": phase, "success": False,
                "setup_error": type(error).__name__})
            barrier.abort()
        finally:
            connection.close()
    started = time.perf_counter()
    error = None
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = [pool.submit(worker, index) for index in range(concurrency)]
        try:
            barrier.wait(timeout=20)
            started = timing["started"]
        except threading.BrokenBarrierError:
            error = "client barrier failed"
        for future in futures: future.result()
    elapsed = time.perf_counter() - started
    rows.sort(key=lambda row: (row["worker"], row.get("sequence", -1)))
    passing = sum(row["success"] for row in rows)
    return {"phase": phase, "samples": rows, "planned": concurrency * per_worker,
        "elapsed_seconds": elapsed, "passed": passing, "error": error,
        "passing_requests_per_second": passing / elapsed,
        "success": not error and len(rows) == concurrency * per_worker and passing == len(rows)}


def run(args, variant, concurrency, block, position):
    directory = args.output / f"c{concurrency}-b{block}-{position}-{variant}"
    directory.mkdir()
    api, proxy = free_port(), free_port()
    while api == proxy: proxy = free_port()
    key = secrets.token_urlsafe(32)
    environment = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn", "HV2_API_KEY": key}
    binary = args.previous if variant == "previous" else args.current
    audit = directory / "access.jsonl"
    if variant == "audit-on":
        key_file = directory / "fixture.key"
        key_file.write_text("42" * 32 + "\n")
        environment.update(HV2_ACCESS_AUDIT=str(audit), HV2_ACCESS_AUDIT_KEY_FILE=str(key_file))
    row = {"variant": variant, "concurrency": concurrency, "block": block, "position": position,
        "success": False, "cleanup_errors": [], "batches": []}
    process = None
    try:
        with (directory / "daemon.log").open("wb") as log:
            process = subprocess.Popen([str(binary), "--port", str(api), "--proxy-port", str(proxy)],
                env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            deadline = time.monotonic() + 10
            while True:
                if time.monotonic() >= deadline: raise RuntimeError("owned daemon readiness timeout")
                if process.poll() is not None: raise RuntimeError("owned daemon exited during startup")
                connection = http.client.HTTPConnection("127.0.0.1", api, timeout=2)
                try:
                    connection.request("GET", "/health")
                    response = connection.getresponse(); response.read()
                    if response.status == 200: break
                except OSError:
                    if time.monotonic() >= deadline: raise
                    time.sleep(.01)
                finally: connection.close()
            row["batches"].append(batch(api, key, concurrency, args.warmup_per_worker, "warmup"))
            before = counters(process.pid)
            row["batches"].append(batch(api, key, concurrency, args.requests_per_worker, "measured"))
            after = counters(process.pid)
            row["measured_process_delta"] = {name: after[name] - before[name] for name in before}
    except Exception as error:
        row["setup_error"] = str(error)
    finally:
        if process is not None:
            try:
                if process.poll() is None:
                    process.terminate()
                    try: process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill(); process.wait(timeout=5)
                row["daemon_exit_code"] = process.returncode
            except Exception as error: row["cleanup_errors"].append(str(error))
        if variant == "audit-on":
            try:
                raw = audit.read_bytes()
                if key.encode() in raw: raise ValueError("API credential appeared in audit records")
                verified = verifier.verify(raw, bytes.fromhex("42" * 32))
                successful = sum(item["passed"] for item in row["batches"])
                if verified["verified_records"] != successful * 2 or verified["uncompleted_admissions"]:
                    raise ValueError("audit records do not match the complete successful request cohort")
                row["audit"] = dict(verified, sha256=digest(audit), credentials_absent=True)
            except Exception as error: row["cleanup_errors"].append("audit: " + str(error))
        row["success"] = (len(row["batches"]) == 2 and all(item["success"] for item in row["batches"])
            and not row["cleanup_errors"] and not row.get("setup_error") and row.get("daemon_exit_code") is not None)
    (directory / "report.json").write_text(json.dumps(row, indent=2) + "\n")
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("previous", "current", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--requests-per-worker", type=int, default=20)
    parser.add_argument("--warmup-per-worker", type=int, default=2)
    parser.add_argument("--profiles", default="1,8,50,100")
    args = parser.parse_args()
    profiles = [int(value) for value in args.profiles.split(",")]
    if not 1 <= args.requests_per_worker <= 100 or not 1 <= args.warmup_per_worker <= 10 or len(set(profiles)) != len(profiles) or not 1 <= len(profiles) <= 4 or any(not 1 <= value <= 100 for value in profiles):
        parser.error("requires 1..100 measured and 1..10 warmup requests per worker, and 1..4 distinct profiles in 1..100")
    args.previous = args.previous.resolve(strict=True)
    args.current = args.current.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    affinity = sorted(os.sched_getaffinity(0))[:8]
    os.sched_setaffinity(0, affinity)
    identities = {"previous": digest(args.previous), "current": digest(args.current),
        "harness": digest(Path(__file__)), "verifier": digest(Path(verifier.__file__))}
    report = {"purpose": "durable audit API cost; no guest or competitor comparison",
        "artifact_sha256": identities, "driver_cpu_affinity": affinity, "added_CPU_load": False,
        "profiles": profiles, "requests_per_worker": args.requests_per_worker,
        "warmup_per_worker": args.warmup_per_worker, "orders": list(itertools.permutations(("previous", "audit-off", "audit-on"))),
        "workload": "persistent-HTTP authenticated GET /sandboxes with empty in-memory inventory",
        "limitations": ["Shared WSL host; no dedicated hardware", "Local HTTP, no TLS or guest work",
            "Fixed closed-loop workers; not sustained offered-load capacity",
            "Warmups retained separately; any failure fails the cohort",
            "CPU ticks and process write_bytes are coarse counters; no storage-latency attribution"],
        "runs": [], "success": False}
    for concurrency in profiles:
        for block, order in enumerate(report["orders"]):
            for position, variant in enumerate(order):
                row = run(args, variant, concurrency, block, position)
                report["runs"].append(row)
                (args.output / "matrix.json").write_text(json.dumps(report, indent=2) + "\n")
                print(json.dumps({"concurrency": concurrency, "block": block, "variant": variant,
                    "success": row["success"]}), flush=True)
    report["artifacts_unchanged"] = (digest(args.previous) == identities["previous"]
        and digest(args.current) == identities["current"] and digest(Path(__file__)) == identities["harness"]
        and digest(Path(verifier.__file__)) == identities["verifier"])
    report["success"] = report["artifacts_unchanged"] and all(row["success"] for row in report["runs"])
    (args.output / "matrix.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
