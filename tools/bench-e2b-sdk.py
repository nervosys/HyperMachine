#!/usr/bin/env python3
"""Measure identical SDK create-to-command readiness on E2B-compatible APIs.

Install e2b==2.51.0 in an isolated environment. Supply explicit endpoints,
E2B_API_KEY, and accurate host/image descriptions; provider labels alone do
not prove product identity or hardware equivalence. No provider runs are
performed automatically. See https://pypi.org/project/e2b/2.51.0/.
"""

import argparse
import concurrent.futures
import datetime
import hashlib
import importlib.metadata
import json
import math
import os
import platform
from pathlib import Path
import shlex
import time
import urllib.parse
import uuid


def positive(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def endpoint(value):
    parsed = urllib.parse.urlsplit(value)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise argparse.ArgumentTypeError("must be an HTTP(S) URL without credentials, query or fragment")
    return value


def summary(values):
    if not values:
        return None
    values = sorted(values)
    return {"n": len(values), "min": values[0], "max": values[-1],
            "mean": sum(values) / len(values),
            **{name: values[math.ceil(fraction * len(values)) - 1]
               for name, fraction in (("p50", .50), ("p95", .95), ("p99", .99))}}


def harness_digest():
    """Fingerprint exact harness bytes, including checkout line endings."""
    return hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def sample(factory, args, nonce, index):
    sandbox = None
    cleanup = []
    record = {"index": index, "success": False}
    started = time.perf_counter()
    phase = "create"
    try:
        sandbox = factory.create(template=args.template, timeout=300,
                                 allow_internet_access=False,
                                 debug=False, api_key=os.environ["E2B_API_KEY"],
                                 api_url=args.api_url, sandbox_url=args.sandbox_url,
                                 request_timeout=args.request_timeout, retries=0)
        record["sandbox_id"] = sandbox.sandbox_id
        cleanup.append(sandbox)
        record["create_ms"] = (time.perf_counter() - started) * 1000
        phase = "ready"
        marker = f"hm-sdk-ready-{nonce}-{index}"
        command = (f"printf '%s' {shlex.quote(marker)}" if args.workload == "posix"
                   else "python3 -c " + shlex.quote(f"import sys; sys.stdout.write({marker!r})"))
        if args.operation != "create":
            phase = "prepare-state"
            directory = f"/tmp/hm-sdk-state-{nonce}-{index}"
            child = f"env HM_SDK_MEMORY={shlex.quote(marker)} sleep 86400"
            prepare = (f"mkdir {directory} && printf '%s' {shlex.quote(marker)} > {directory}/state && "
                       f"cat /proc/sys/kernel/random/boot_id > {directory}/boot && {{ "
                       f"{child} < /dev/null > /dev/null 2>&1 & echo $! > {directory}/pid; }}")
            prepared = sandbox.commands.run(prepare, timeout=args.command_timeout)
            if prepared.exit_code != 0:
                raise RuntimeError("failed to prepare the live-process state probe")
            verify = (f"test \"$(cat {directory}/state)\" = {shlex.quote(marker)} && "
                      f"test \"$(cat {directory}/boot)\" = \"$(cat /proc/sys/kernel/random/boot_id)\" && "
                      f"kill -0 \"$(cat {directory}/pid)\" && "
                      f"tr '\\000' '\\n' < /proc/$(cat {directory}/pid)/environ | "
                      f"grep -Fx {shlex.quote('HM_SDK_MEMORY=' + marker)} > /dev/null")
            # Validate before snapshotting, including the asynchronous child's startup.
            precheck = sandbox.commands.run(
                f"for attempt in 1 2 3 4 5 6 7 8 9 10; do {verify} && exit 0; sleep 0.1; done; exit 1",
                timeout=args.command_timeout)
            if precheck.exit_code != 0:
                raise RuntimeError("live process, memory marker or filesystem state probe was not ready")
            command = verify + " && " + command
            if args.operation == "resume":
                phase = "pause"
                paused_at = time.perf_counter()
                sandbox.pause(keep_memory=True, request_timeout=args.request_timeout)
                record["pause_ms"] = (time.perf_counter() - paused_at) * 1000
                if sandbox.get_info(request_timeout=args.request_timeout).state != "paused":
                    raise RuntimeError("pause did not leave the sandbox in the paused state")
                phase = "resume"
                started = time.perf_counter()
                sandbox.connect(timeout=300, on_resume="restore", request_timeout=args.request_timeout)
            else:
                phase = "fork"
                started = time.perf_counter()
                forks = sandbox.fork(count=1, timeout=300, request_timeout=args.request_timeout)
                cleanup.extend(fork for fork in forks if not isinstance(fork, Exception))
                if len(forks) != 1 or isinstance(forks[0], Exception):
                    raise RuntimeError("fork did not return exactly one successful sandbox")
                record["parent_sandbox_id"] = sandbox.sandbox_id
                sandbox = forks[0]
                record["sandbox_id"] = sandbox.sandbox_id
                if record["sandbox_id"] == record["parent_sandbox_id"]:
                    raise RuntimeError("fork returned the parent instead of a distinct sandbox")
            record["operation_ms"] = (time.perf_counter() - started) * 1000
            phase = "ready"
        execution_started = time.perf_counter()
        result = sandbox.commands.run(command, timeout=args.command_timeout)
        record["exec_ms"] = (time.perf_counter() - execution_started) * 1000
        record["ready_ms"] = (time.perf_counter() - started) * 1000
        if result.exit_code != 0 or result.stdout != marker:
            raise RuntimeError("readiness command did not produce its expected marker with exit 0")
        phase = "resources"
        info = sandbox.get_info(request_timeout=args.request_timeout)
        if info.state != "running":
            raise RuntimeError("verified command sandbox is not reported as running")
        record["cpu_count"] = info.cpu_count
        record["memory_mb"] = info.memory_mb
        if info.cpu_count != args.expected_cpus or info.memory_mb != args.expected_memory_mb:
            raise RuntimeError("server guest resources differ from the requested comparison baseline")
        if args.operation == "fork":
            phase = "parent-state"
            parent = cleanup[0].commands.run(command, timeout=args.command_timeout)
            if parent.exit_code != 0 or parent.stdout != marker:
                raise RuntimeError("fork parent did not retain its live-process and filesystem state")
        record["success"] = True
    except Exception as error:
        record.update(phase=phase, error=str(error).replace(os.environ["E2B_API_KEY"], "[redacted]"))
    finally:
        for known_sandbox in reversed(cleanup):
            try:
                known_sandbox.kill(request_timeout=args.request_timeout)
            except Exception as error:
                record["success"] = False
                record.setdefault("cleanup_errors", []).append({"sandbox_id": known_sandbox.sandbox_id,
                    "error": str(error).replace(os.environ["E2B_API_KEY"], "[redacted]")})
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", required=True)
    parser.add_argument("--api-url", required=True, type=endpoint)
    parser.add_argument("--sandbox-url", type=endpoint, help="Override SDK envd URL; omit for provider-managed sandbox domains")
    parser.add_argument("--template", required=True)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--image-description", required=True)
    parser.add_argument("--expected-cpus", required=True, type=positive)
    parser.add_argument("--expected-memory-mb", required=True, type=positive)
    parser.add_argument("--workload", choices=("posix", "python"), default="posix")
    parser.add_argument("--operation", choices=("create", "resume", "fork"), default="create",
                        help="Lifecycle operation through first verified command; resume/fork also check live process memory and filesystem state")
    parser.add_argument("--samples", type=positive, default=100)
    parser.add_argument("--concurrency", type=positive, default=1)
    parser.add_argument("--request-timeout", type=positive, default=120)
    parser.add_argument("--command-timeout", type=positive, default=30)
    parser.add_argument("--max-p99-ready-ms", type=positive)
    args = parser.parse_args()
    if args.samples > 10000 or args.concurrency > 1000:
        parser.error("samples must be <=10000 and concurrency <=1000")
    if args.request_timeout <= args.command_timeout:
        parser.error("request timeout must exceed command timeout")
    if not all(value.strip() for value in (args.provider, args.environment, args.image_description, args.template)):
        parser.error("provider, template, environment and image description must be nonempty")
    if not os.environ.get("E2B_API_KEY"):
        parser.error("E2B_API_KEY must be set")
    if os.environ.get("E2B_ENVD_POOL_SHARDS"):
        parser.error("unset E2B_ENVD_POOL_SHARDS to preserve the SDK connection-pool baseline")
    if os.environ.get("E2B_API_URL") or os.environ.get("E2B_SANDBOX_URL"):
        parser.error("unset E2B_API_URL/E2B_SANDBOX_URL; use explicit endpoint arguments for this comparison")
    version = importlib.metadata.version("e2b")
    if version != "2.51.0":
        parser.error(f"expected e2b==2.51.0; installed {version}")
    from e2b import Sandbox
    dependencies = {name: importlib.metadata.version(name)
                    for name in ("e2b", "pyqwest", "connectrpc", "httpx", "httpcore")}
    source_digest = harness_digest()
    nonce = uuid.uuid4().hex
    started = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        records = list(pool.map(lambda index: sample(Sandbox, args, nonce, index), range(args.samples)))
    elapsed = time.perf_counter() - started
    successful = [record for record in records if record["success"]]
    ready = summary([record["ready_ms"] for record in successful])
    passed = None if args.max_p99_ready_ms is None else bool(ready and ready["p99"] <= args.max_p99_ready_ms)
    try:
        harness_unchanged = harness_digest() == source_digest
    except OSError:
        # Preserve raw samples if an editor removes or replaces the source.
        harness_unchanged = False
    print(json.dumps({"schema_version": 1, "transport": "e2b-python-sdk", "sdk_version": version,
                      "dependency_versions": dependencies,
                      "harness_sha256": source_digest, "harness_unchanged_during_run": harness_unchanged,
                      "provider": args.provider, "api_url": args.api_url, "sandbox_url": args.sandbox_url,
                      "template": args.template, "environment": args.environment, "image_description": args.image_description,
                      "workload": args.workload, "expected_cpus": args.expected_cpus, "expected_memory_mb": args.expected_memory_mb,
                      "operation": args.operation,
                      "internet_access": False, "sdk_debug": False, "sdk_retries": 0, "sandbox_lifetime_seconds": 300,
                      "request_timeout_seconds": args.request_timeout, "command_timeout_seconds": args.command_timeout,
                      "samples_requested": args.samples, "concurrency": args.concurrency, "successful_samples": len(successful),
                      "failed_samples": len(records) - len(successful), "elapsed_seconds": elapsed,
                      "completed_lifecycles_per_second": len(successful) / elapsed,
                      "client_os": platform.platform(), "client_python": platform.python_version(),
                      "finished_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                      "create_ms": summary([record["create_ms"] for record in successful]),
                      "exec_ms": summary([record["exec_ms"] for record in successful]), "ready_ms": ready,
                      "operation_ms": summary([record["operation_ms"] for record in successful if "operation_ms" in record]),
                      "pause_ms": summary([record["pause_ms"] for record in successful if "pause_ms" in record]),
                      "max_p99_ready_ms": args.max_p99_ready_ms, "threshold_passed": passed, "samples": records}, indent=2))
    return 0 if len(successful) == args.samples and passed is not False and harness_unchanged else 1


if __name__ == "__main__":
    raise SystemExit(main())
