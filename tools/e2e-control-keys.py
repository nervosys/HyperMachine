#!/usr/bin/env python3
"""Verify scoped/expiring keys through the shipped control-plane binary."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane", required=True)
    args = parser.parse_args()
    binary = str(Path(args.control_plane).resolve(strict=True))
    environment = dict(os.environ)
    environment.pop("HV2_API_KEY", None)
    environment.pop("HV2_CLUSTER_TOKEN", None)
    environment["RUST_LOG"] = "warn"
    with tempfile.TemporaryDirectory(prefix="hm-control-keys-") as directory:
        policy_file = Path(directory) / "keys.json"
        policy_file.write_text("[]")
        invalid = subprocess.run([binary, "--api-keys-file", str(policy_file)],
                                 env=environment, capture_output=True, timeout=10)
        assert invalid.returncode != 0, "empty policy must fail startup"
        assert b"API key policy needs" in invalid.stderr, "policy validation must fail before listeners"
        inventory, admin, sandboxes = (secrets.token_urlsafe(32) for _ in range(3))
        expiry = int(time.time()) + 5
        policy_file.write_text(json.dumps([
            {"sha256": hashlib.sha256(key.encode()).hexdigest(), "expires_at": expires, "scopes": scopes}
            for key, expires, scopes in [(inventory, expiry, ["inventory"]),
                                        (admin, expiry + 60, ["admin"]),
                                        (sandboxes, expiry + 60, ["sandboxes"])]]))
        collision_environment = dict(environment, HV2_API_KEY=inventory)
        collision = subprocess.run([binary, "--api-keys-file", str(policy_file)],
                                   env=collision_environment, capture_output=True, timeout=10)
        assert collision.returncode != 0, "admin/scoped credential collision must fail startup"
        assert b"must differ" in collision.stderr
        assert inventory.encode() not in collision.stderr, "startup errors must not expose credentials"
        api_port, proxy_port = port(), port()
        while api_port == proxy_port:
            proxy_port = port()
        base = f"http://127.0.0.1:{api_port}"
        with (Path(directory) / "daemon.log").open("wb") as log:
            process = subprocess.Popen([binary, "--port", str(api_port), "--proxy-port", str(proxy_port),
                                        "--api-keys-file", str(policy_file)], env=environment, stdout=log, stderr=log)
            try:
                def status(path, key=None, method="GET"):
                    headers = {} if key is None else {"X-API-Key": key}
                    request = urllib.request.Request(base + path, headers=headers, method=method,
                        data=b'{}' if method == "POST" else None)
                    try:
                        with urllib.request.urlopen(request, timeout=2) as response:
                            return response.status
                    except urllib.error.HTTPError as error:
                        return error.code
                deadline = time.monotonic() + 3
                while True:
                    assert process.poll() is None, "control plane exited before readiness"
                    try:
                        if status("/health") == 200:
                            break
                    except urllib.error.URLError:
                        pass
                    assert time.monotonic() < deadline, "control plane did not become ready"
                    time.sleep(.05)
                assert status("/sandboxes") == 401
                assert status("/sandboxes", inventory) == 200
                assert status("/sandboxes", inventory, "POST") == 403
                assert status("/sandboxes/missing", inventory) == 403
                assert status("/volumes", inventory) == 403
                assert status("/events/webhooks", inventory) == 403
                assert status("/templates", sandboxes) == 403
                assert status("/sandboxes", sandboxes) == 200
                assert status("/templates", admin) == 200
                while time.time() < expiry:
                    time.sleep(.05)
                assert status("/sandboxes", inventory) == 401, "key must expire without restart"
                assert status("/sandboxes", admin) == 200
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
    print("PASS: shipped control-plane scopes, live expiry, token-access denial and fail-closed policy/collision startup")


if __name__ == "__main__":
    main()
