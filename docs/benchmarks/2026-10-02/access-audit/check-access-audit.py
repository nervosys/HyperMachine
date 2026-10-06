#!/usr/bin/env python3
"""Exercise durable audit startup, restart, privacy and storage failure on owned Linux processes."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import resource
import secrets
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.request

spec = importlib.util.spec_from_file_location("verifier", Path(__file__).with_name("verify-access-audit.py"))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


def port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    binary = args.control_plane.resolve(strict=True)
    identity = hashlib.sha256(binary.read_bytes()).hexdigest()
    key_file, audit_path = args.output / "fixture.key", args.output / "access.jsonl"
    key_file.write_text("42" * 32 + "\n")
    credential, inventory = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
    policies = args.output / "policies.json"
    policies.write_text(json.dumps([{"sha256": hashlib.sha256(inventory.encode()).hexdigest(),
        "expires_at": int(time.time()) + 600, "scopes": ["inventory"]}]))
    environment = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn", "HV2_API_KEY": credential,
        "HV2_ACCESS_AUDIT": str(audit_path), "HV2_ACCESS_AUDIT_KEY_FILE": str(key_file)}
    processes = []
    report = {"success": False, "checks": [], "cleanup_errors": [], "control_plane_sha256": identity,
        "fixture_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "verifier_sha256": hashlib.sha256(Path(verifier.__file__).read_bytes()).hexdigest(),
        "synthetic_audit_key": "42 repeated 32 times", "performance_comparison": False}

    def rejected(name, env):
        result = subprocess.run([str(binary)], env=env, capture_output=True, timeout=10)
        assert result.returncode != 0
        assert credential.encode() not in result.stderr
        (args.output / (name + ".log")).write_bytes(result.stdout + result.stderr)
        report["checks"].append(name)

    def stop(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait(timeout=5)

    def start(name):
        api, proxy = port(), port()
        while api == proxy: proxy = port()
        log = (args.output / (name + ".log")).open("wb")
        # Ignoring SIGXFSZ permits the owned process to observe EFBIG in the
        # explicit file-size fault below. This applies only to this fixture.
        process = subprocess.Popen([str(binary), "--port", str(api), "--proxy-port", str(proxy),
            "--api-keys-file", str(policies)], env=environment, stdout=log, stderr=log,
            preexec_fn=lambda: signal.signal(signal.SIGXFSZ, signal.SIG_IGN))
        log.close()
        processes.append(process)
        base = f"http://127.0.0.1:{api}"
        deadline = time.monotonic() + 10
        while True:
            assert process.poll() is None, "owned control plane exited during startup"
            try:
                if request(base, "/health", None)[0] == 200: return process, base
            except OSError: pass
            assert time.monotonic() < deadline, "owned control plane readiness timeout"
            time.sleep(.02)

    def request(base, path, key=credential, method="GET", body=None):
        req = urllib.request.Request(base + path, method=method,
            headers={} if key is None else {"x-api-key": key, "content-type": "application/json"},
            data=None if body is None else json.dumps(body).encode())
        try: response = urllib.request.urlopen(req, timeout=5)
        except urllib.error.HTTPError as error: response = error
        with response: return response.status, response.read()

    try:
        for missing in ["HV2_ACCESS_AUDIT", "HV2_ACCESS_AUDIT_KEY_FILE"]:
            env = dict(environment); env.pop(missing)
            rejected("partial-" + missing.lower(), env)
        bad = args.output / "bad.key"; bad.write_text("z" * 129)
        rejected("invalid-key", dict(environment, HV2_ACCESS_AUDIT_KEY_FILE=str(bad)))
        for index in range(2):
            process, base = start("restart-" + str(index))
            if index == 0: rejected("second-writer", environment)
            assert request(base, "/sandboxes?metadata[hidden]=query-private", "unknown-private")[0] == 401
            assert request(base, "/sandboxes", inventory)[0] == 200
            assert request(base, "/v2/sandboxes", inventory, "POST", {"metadata":{"hidden":"body-private"}})[0] == 403
            assert request(base, "/sandboxes/path-private")[0] == 404
            assert request(base, "/sandboxes")[0] == 200
            stop(process)
            report["checks"].append("restart-" + str(index) + "-authorized-and-denied-records")
        raw = audit_path.read_bytes()
        result = verifier.verify(raw, bytes.fromhex("42" * 32))
        assert result["verified_records"] == 20 and result["uncompleted_admissions"] == 0
        for value in [credential, inventory, "unknown-private", "query-private", "body-private", "path-private"]:
            assert value.encode() not in raw
        report["audit"] = result
        report["checks"].append("independent-mac-verification-and-private-values-absent")
        wrong_key = args.output / "wrong.key"; wrong_key.write_text("43" * 32)
        rejected("wrong-key-restart", dict(environment, HV2_ACCESS_AUDIT_KEY_FILE=str(wrong_key)))
        audit_path.write_bytes(raw[:-1])
        rejected("partial-tail-restart", environment)
        assert audit_path.read_bytes() == raw[:-1]
        audit_path.write_bytes(raw)
        edited = raw.replace(b'"status":401', b'"status":200', 1)
        assert edited != raw
        audit_path.write_bytes(edited)
        rejected("edited-record-restart", environment)
        assert audit_path.read_bytes() == edited
        audit_path.write_bytes(raw)
        process, base = start("storage-fault")
        resource.prlimit(process.pid, resource.RLIMIT_FSIZE, (len(raw), len(raw)))
        for _ in range(2):
            status, body = request(base, "/v2/sandboxes", method="POST", body={"templateID":"base"})
            assert status == 503 and b"not dispatched" in body
        assert process.poll() is None
        assert request(base, "/health", None)[0] == 200
        stop(process)
        assert audit_path.read_bytes() == raw
        report["checks"].append("storage-failure-latches-before-dispatch-and-health-remains-available")
        report["success"] = True
    except Exception as error:
        report["error"] = str(error)
    finally:
        for process in processes:
            try: stop(process)
            except Exception as error: report["cleanup_errors"].append(str(error))
        report["owned_processes_stopped"] = all(process.poll() is not None for process in processes)
        report["artifact_unchanged"] = hashlib.sha256(binary.read_bytes()).hexdigest() == identity
        report["success"] &= report["owned_processes_stopped"] and report["artifact_unchanged"] and not report["cleanup_errors"]
        policies.unlink(missing_ok=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
