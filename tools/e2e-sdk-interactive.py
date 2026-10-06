#!/usr/bin/env python3
"""Check SDK stdin, EOF and signals on real guests; timings are diagnostic."""
import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import os
from pathlib import Path
import time
import uuid

spec = importlib.util.spec_from_file_location("bench", Path(__file__).with_name("bench-e2b-sdk.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--api-url", required=True, type=bench.endpoint)
    parser.add_argument("--sandbox-url", required=True, type=bench.endpoint)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--template", default="base")
    parser.add_argument("--samples", type=bench.positive, default=20)
    args = parser.parse_args()
    if not os.environ.get("E2B_API_KEY"):
        parser.error("E2B_API_KEY must be configured")
    if args.samples > 1000:
        parser.error("samples must be <=1000 per operation")
    if any(os.environ.get(name) for name in ("E2B_API_URL", "E2B_SANDBOX_URL", "E2B_ENVD_POOL_SHARDS")):
        parser.error("unset SDK endpoint/pool environment overrides")
    if importlib.metadata.version("e2b") != "2.51.0":
        parser.error("requires e2b==2.51.0")
    from e2b import Sandbox
    sandbox = None
    info = None
    records = []
    cleanup_error = None
    setup_error = None
    try:
        sandbox = Sandbox.create(template=args.template, timeout=300, allow_internet_access=False,
            debug=False, api_key=os.environ["E2B_API_KEY"], api_url=args.api_url,
            sandbox_url=args.sandbox_url, request_timeout=120, retries=0)
        info = sandbox.get_info()
        for operation in ("stdin", "eof", "signal"):
            for index in range(args.samples):
                row = {"operation": operation, "index": index, "success": False}
                records.append(row)
                try:
                    command = {"stdin": "read line; printf '%s' \"$line\"", "eof": "cat", "signal": "exec sleep 86400"}[operation]
                    handle = sandbox.commands.run(command, background=True, stdin=True, timeout=10)
                    marker = "hm-interactive-" + uuid.uuid4().hex
                    # Vary arrival within a poll interval, outside the timed action.
                    time.sleep((index % 5) * .01)
                    started = time.perf_counter()
                    if operation == "stdin":
                        handle.send_stdin(marker + "\n")
                    elif operation == "eof":
                        handle.close_stdin()
                    else:
                        if not handle.kill():
                            raise RuntimeError("signal did not reach a running process")
                    try:
                        result = handle.wait()
                    except Exception as error:
                        if operation != "signal" or getattr(error, "exit_code", None) != 137:
                            raise
                        result = error
                    row["action_to_exit_ms"] = (time.perf_counter() - started) * 1000
                    row["exit_code"] = result.exit_code
                    row["stdout_bytes"] = len(result.stdout.encode())
                    row["stderr_bytes"] = len(result.stderr.encode())
                    expected = marker if operation == "stdin" else ""
                    if result.stdout != expected or result.exit_code != (137 if operation == "signal" else 0):
                        raise RuntimeError("interactive output or exit status mismatch")
                    row["success"] = True
                except Exception as error:
                    row["error"] = str(error).replace(os.environ["E2B_API_KEY"], "[redacted]")
    except Exception as error:
        setup_error = str(error).replace(os.environ["E2B_API_KEY"], "[redacted]")
    finally:
        if sandbox is not None:
            try:
                sandbox.kill()
            except Exception as error:
                cleanup_error = str(error).replace(os.environ["E2B_API_KEY"], "[redacted]")
    print(json.dumps({"schema_version": 1, "label": args.label, "environment": args.environment,
        "sdk_version": "2.51.0", "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "cpu_count": None if info is None else info.cpu_count,
        "memory_mb": None if info is None else info.memory_mb,
        "cleanup_error": cleanup_error, "setup_error": setup_error,
        "samples": records, "action_to_exit_ms": {operation: bench.summary([row["action_to_exit_ms"]
            for row in records if row["operation"] == operation and row["success"]])
            for operation in ("stdin", "eof", "signal")}}, indent=2))
    return 0 if len(records) == 3 * args.samples and all(row["success"] for row in records) and cleanup_error is None and setup_error is None else 1


if __name__ == "__main__":
    raise SystemExit(main())
