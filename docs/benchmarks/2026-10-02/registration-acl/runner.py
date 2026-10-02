#!/usr/bin/env python3
"""Run registration tests against a temporary owned loopback Redis server."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--filter", default="")
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    with tempfile.TemporaryDirectory(prefix="hm-acl-redis-") as temporary:
        with args.output.with_suffix(".redis.log").open("wb") as log:
            server = subprocess.Popen(["redis-server", "--bind", "127.0.0.1", "--port", str(port),
                                       "--save", "", "--appendonly", "no", "--dir", temporary],
                                      stdout=log, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 10
                while subprocess.run(["redis-cli", "-p", str(port), "PING"], stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE).stdout.strip() != b"PONG":
                    assert server.poll() is None and time.monotonic() < deadline
                    time.sleep(.05)
                environment = dict(os.environ, CARGO_TARGET_DIR="/var/tmp/hm-competitive-target",
                                   HV2_TEST_REDIS=f"redis://127.0.0.1:{port}",
                                   HV2_TEST_REDIS_ACL=f"redis://127.0.0.1:{port}")
                command = ["cargo", "test", "-p", "hv2-cluster", "--lib"]
                if args.filter:
                    command.append(args.filter)
                with args.output.with_suffix(".tests.log").open("wb") as tests:
                    result = subprocess.run(command + ["--", "--nocapture"], cwd=args.source,
                                            env=environment, stdout=tests, stderr=subprocess.STDOUT)
                users = subprocess.check_output(["redis-cli", "-p", str(port), "ACL", "USERS"]).splitlines()
                assert users == [b"default"], users
            finally:
                server.terminate()
                server.wait(timeout=10)
    report = {"success": result.returncode == 0, "test_exit_code": result.returncode,
              "owned_redis_stopped": server.poll() is not None, "redis_exit_code": server.returncode,
              "temporary_acl_users_removed": True, "filter": args.filter}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
