#!/usr/bin/env python3
"""Verify scoped/expiring keys through the shipped control-plane binary."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import re
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
    environment["RUST_LOG"] = "warn,hv2_cluster::access=info"
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
        log_path = Path(directory) / "daemon.log"
        with log_path.open("wb") as log:
            process = subprocess.Popen([binary, "--port", str(api_port), "--proxy-port", str(proxy_port),
                                        "--api-keys-file", str(policy_file)], env=environment, stdout=log, stderr=log)
            try:
                def status(path, key=None, method="GET", payload=None):
                    headers = {} if key is None else {"X-API-Key": key}
                    request = urllib.request.Request(base + path, headers=headers, method=method,
                        data=payload if payload is not None else (b'{}' if method == "POST" else None))
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
                assert status("/sandboxes?private=audit-private-query", inventory) == 200
                assert status("/sandboxes/audit-private-path", inventory) == 403
                assert status("/sandboxes", inventory, "POST", b'{"private":"audit-private-body"}') == 403
                assert status("/sandboxes/missing/network", inventory, "audit-private-method") == 403
                assert status("/sandboxes", "audit-private-key") == 401
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
        audit = re.sub(r"\x1b\[[0-9;]*m", "", log_path.read_text())
        assert "control-plane access started" in audit
        assert "control-plane access completed" in audit
        assert "/sandboxes/{id}" in audit, "audit must use route templates, not supplied resource IDs"
        for forbidden in [inventory, admin, sandboxes, "audit-private-query", "audit-private-path",
                          "audit-private-body", "audit-private-method", "audit-private-key",
                          hashlib.sha256(b"audit-private-key").hexdigest()[:16]]:
            assert forbidden not in audit, "audit exposed credential or request contents"
        assert hashlib.sha256(inventory.encode()).hexdigest()[:16] in audit
        for expected in ["status=200", "status=401", "status=403"]:
            assert expected in audit, f"audit missing {expected}"
        assert re.search(r'principal="?expired"?', audit), "audit missing expired credential category"
        started, completed = set(), set()
        for line in audit.splitlines():
            match = re.search(r"request_id=([0-9a-f-]{36})", line)
            if match is None:
                continue
            if "control-plane access started" in line:
                assert match[1] not in started, "duplicate audit start"
                started.add(match[1])
            if "control-plane access completed" in line:
                assert match[1] not in completed, "duplicate audit completion"
                completed.add(match[1])
        assert started and started <= completed, "accepted requests must have matching completion IDs"
        assert len(completed) > len(started), "denied requests must also have audit records"
    print("PASS: shipped control-plane scopes, expiry, token denial, fail-closed startup and credential-safe access audit")


if __name__ == "__main__":
    main()
