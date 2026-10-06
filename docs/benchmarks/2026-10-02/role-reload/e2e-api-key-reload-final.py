#!/usr/bin/env python3
"""Owned local control-plane process check for Unix SIGHUP policy rotation."""
import argparse
import hashlib
import json
import os
import secrets
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--roles", action="store_true", help="verify same-key operator/observer role changes and rejected role reloads")
    args = parser.parse_args()
    require(not args.output.exists() and not args.output.with_suffix(".log").exists(), "output already exists")
    report = {"success": False, "scope": "local process-level Unix signal reload; no VM or performance comparison",
              "binary_sha256": hashlib.sha256(args.control_plane.read_bytes()).hexdigest(),
              "coordinator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "roles_requested": args.roles, "checks": []}
    process = None
    with tempfile.TemporaryDirectory(prefix="hm-key-reload-") as raw:
        root = Path(raw)
        policies = root / "policies.json"
        log_path = root / "service.log"
        admin, old, new = (secrets.token_urlsafe(32) for _ in range(3))
        def document(key, expiry=None, role="operator", scope="inventory"):
            return json.dumps([{"sha256": hashlib.sha256(key.encode()).hexdigest(),
                "expires_at": expiry or int(time.time()) + 600, "scopes": [scope], "role": role}])
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
        def status(key, method="GET", path="/sandboxes"):
            request = urllib.request.Request(f"http://127.0.0.1:{api}{path}",
                headers={"x-api-key":key}, method=method)
            try:
                with urllib.request.urlopen(request, timeout=2) as response:
                    return response.status
            except urllib.error.HTTPError as error:
                return error.code
        def check(name, key, expected, method="GET", path="/sandboxes"):
            actual = status(key, method, path)
            require(actual == expected, (name, actual, expected))
            report["checks"].append({"name":name,"status":actual,"expected":expected})
        def reload(expected_message):
            before = log_path.read_text().count(expected_message)
            process.send_signal(signal.SIGHUP)
            deadline = time.monotonic() + 10
            while log_path.read_text().count(expected_message) <= before:
                require(process.poll() is None, "control plane exited")
                if time.monotonic() > deadline:
                    raise TimeoutError("reload acknowledgement missing")
                time.sleep(.02)
        with log_path.open("wb") as log:
            try:
                environment = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn", "HV2_API_KEY": admin}
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
                    require(process.poll() is None, "startup failed")
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
                if args.roles:
                    replace(document(old, role="operator", scope="admin"))
                    reload("API key policies reloaded")
                    check("operator volume inventory", old, 200, path="/volumes")
                    replace(document(old, role="observer", scope="admin"))
                    reload("API key policies reloaded")
                    check("same-key downgrade preserves inventory", old, 200)
                    check("same-key downgrade blocks capabilities", old, 403, path="/volumes")
                    check("observer blocks mutation", old, 403, "POST")
                    check("observer blocks GET tunnel", old, 403, path="/sandboxes/fixture/ports/22/tcp")
                    for name, role in [("unknown role", "unknown"), ("null role", None)]:
                        replace(document(old, role=role, scope="admin"))
                        reload("API key reload rejected")
                        check(name + " preserves inventory", old, 200)
                        check(name + " preserves capability denial", old, 403, path="/volumes")
                    replace(document(old, role="operator", scope="admin"))
                    reload("API key policies reloaded")
                    check("same-key upgrade restores capability", old, 200, path="/volumes")
                    replace(document(old, role="observer", scope="sandboxes"))
                    reload("API key policies reloaded")
                    check("observer scope allows sandbox inventory", old, 200)
                    check("observer scope excludes template inventory", old, 403, path="/templates")
                    check("role changes keep legacy admin", admin, 200, path="/volumes")
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
                raw_log = log_path.read_bytes()
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.with_suffix(".log").write_bytes(raw_log)
                report["credentials_absent"] = all(key.encode() not in raw_log for key in [admin, old, new])
                report["service_log_sha256"] = hashlib.sha256(raw_log).hexdigest()
                report["binary_unchanged"] = report["binary_sha256"] == hashlib.sha256(args.control_plane.read_bytes()).hexdigest()
                report["success"] = report["success"] and report["binary_unchanged"] and report["credentials_absent"]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
