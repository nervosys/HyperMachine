#!/usr/bin/env python3
"""Owned local control-plane process check for Unix SIGHUP policy rotation."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = {"success": False, "scope": "local process-level Unix signal reload; no VM or performance comparison",
              "binary_sha256": hashlib.sha256(args.control_plane.read_bytes()).hexdigest(),
              "coordinator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "checks": []}
    process = None
    with tempfile.TemporaryDirectory(prefix="hm-key-reload-") as raw:
        root = Path(raw)
        policies = root / "policies.json"
        log_path = root / "service.log"
        admin, old, new = "fixture-admin", "fixture-old", "fixture-new"
        def document(key, expiry=None):
            return json.dumps([{"sha256": hashlib.sha256(key.encode()).hexdigest(),
                "expires_at": expiry or int(time.time()) + 600, "scopes": ["inventory"]}])
        def replace(text):
            temporary = root / "replacement.json"
            temporary.write_bytes(text if isinstance(text, bytes) else text.encode())
            os.replace(temporary, policies)
        replace(document(old))
        def port():
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                return sock.getsockname()[1]
        api, proxy = port(), port()
        while proxy == api:
            proxy = port()
        def status(key, method="GET"):
            request = urllib.request.Request(f"http://127.0.0.1:{api}/sandboxes",
                headers={"x-api-key":key}, method=method)
            try:
                with urllib.request.urlopen(request, timeout=2) as response:
                    return response.status
            except urllib.error.HTTPError as error:
                return error.code
        def check(name, key, expected, method="GET"):
            actual = status(key, method)
            assert actual == expected, (name, actual, expected)
            report["checks"].append({"name":name,"status":actual,"expected":expected})
        def reload(expected_message):
            before = log_path.read_text().count(expected_message)
            process.send_signal(signal.SIGHUP)
            deadline = time.monotonic() + 10
            while log_path.read_text().count(expected_message) <= before:
                assert process.poll() is None, "control plane exited"
                if time.monotonic() > deadline:
                    raise TimeoutError("reload acknowledgement missing")
                time.sleep(.02)
        with log_path.open("wb") as log:
            try:
                environment = dict(os.environ, HV2_API_KEY=admin)
                process = subprocess.Popen([str(args.control_plane), "--store", "memory:",
                    "--port", str(api), "--proxy-port", str(proxy), "--api-keys-file", str(policies)],
                    env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                deadline = time.monotonic() + 15
                while True:
                    try:
                        if status(old) == 200:
                            break
                    except OSError:
                        pass
                    assert process.poll() is None, "startup failed"
                    if time.monotonic() > deadline:
                        raise TimeoutError("startup readiness missing")
                    time.sleep(.02)
                check("initial key", old, 200)
                replace(document(new))
                reload("API key policies reloaded")
                check("old key revoked", old, 401)
                check("new key active", new, 200)
                check("scope preserved", new, 403, "POST")
                for name, text in [("malformed", "invalid-fixture"), ("empty", "[]"),
                                   ("admin collision", document(admin)),
                                   ("oversized", b"x" * (1048576 + 1)),
                                   ("invalid UTF-8", b"\xff")]:
                    replace(text)
                    reload("API key reload rejected")
                    check(name + " preserves active key", new, 200)
                    check(name + " preserves revocation", old, 401)
                policies.unlink()
                reload("API key reload rejected")
                check("missing file preserves active key", new, 200)
                check("admin unchanged", admin, 200)
                replace(document(new, int(time.time()) - 60))
                reload("API key policies reloaded")
                check("expired replacement revokes scoped access", new, 401)
                check("expired set keeps authentication required", "", 401)
                replace(document(old))
                reload("API key policies reloaded")
                check("valid update after rejection", old, 200)
                check("restoration revokes previous key", new, 401)
                report["success"] = True
            except Exception as error:
                report["error"] = str(error)
            finally:
                if process is not None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=10)
                    report["owned_process_stopped"] = process.poll() is not None
                    report["exit_code"] = process.returncode
                report["success"] = report["success"] and report.get("owned_process_stopped", False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
