#!/usr/bin/env python3
"""Check opt-in sandbox references against owned native control-plane processes."""
import argparse
import hashlib
import hmac
import importlib.util
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import struct
import time
import urllib.error
import urllib.request


def require(condition, message):
    if not condition: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    binary = args.control_plane.resolve(strict=True)
    key_file = args.output / "fixture.key"
    key = bytes([42]) * 32
    key_file.write_text(key.hex())
    credential = secrets.token_urlsafe(32)
    spec = importlib.util.spec_from_file_location("verifier", Path(__file__).with_name("verify-access-audit.py"))
    verifier = importlib.util.module_from_spec(spec); spec.loader.exec_module(verifier)
    identity = hashlib.sha256(binary.read_bytes()).hexdigest()
    base_env = {"PATH": "/usr/bin:/bin", "RUST_LOG": "warn", "HV2_API_KEY": credential}
    processes = []
    report = {"success": False, "checks": [], "cleanup_errors": [], "control_plane_sha256": identity,
        "coordinator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "verifier_sha256": hashlib.sha256(Path(verifier.__file__).read_bytes()).hexdigest(),
        "synthetic_key": "42 repeated 32 times", "performance_comparison": False}

    def port():
        with socket.socket() as s: s.bind(("127.0.0.1", 0)); return s.getsockname()[1]

    def stop(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)

    def request(base, path, token):
        req = urllib.request.Request(base + path, headers={"x-api-key": token})
        try: response = urllib.request.urlopen(req, timeout=5)
        except urllib.error.HTTPError as error: response = error
        with response: return response.status

    def reference(target):
        return hmac.new(key, b"HyperMachine access resource v1\0sandbox\0" + target.encode(), hashlib.sha256).hexdigest()

    try:
        for setting in ["bad", "TRUE", "1", "true"]:
            env = dict(base_env, HV2_ACCESS_AUDIT_RESOURCES=setting)
            result = subprocess.run([str(binary)], env=env, capture_output=True, timeout=10)
            require(result.returncode != 0, "invalid/unaudited attribution accepted")
            require(credential.encode() not in result.stdout + result.stderr, "credential leaked")
            (args.output / ("rejected-" + setting + ".log")).write_bytes(result.stdout + result.stderr)
            report["checks"].append("rejected-" + setting)
        for setting in [None, "false", "true"]:
            label = "default" if setting is None else setting
            log_path = args.output / (label + ".jsonl")
            for restart in range(2):
                env = dict(base_env, HV2_ACCESS_AUDIT=str(log_path), HV2_ACCESS_AUDIT_KEY_FILE=str(key_file))
                if setting is not None: env["HV2_ACCESS_AUDIT_RESOURCES"] = setting
                api, proxy = port(), port()
                while proxy == api: proxy = port()
                with (args.output / (label + "-" + str(restart) + ".log")).open("wb") as log:
                    process = subprocess.Popen([str(binary), "--port", str(api), "--proxy-port", str(proxy)], env=env, stdout=log, stderr=log)
                processes.append(process)
                base = f"http://127.0.0.1:{api}"
                deadline = time.monotonic() + 10
                while True:
                    require(process.poll() is None, "process exited before readiness")
                    try:
                        if request(base, "/health", credential) == 200: break
                    except OSError: pass
                    require(time.monotonic() < deadline, "readiness timeout")
                    time.sleep(.02)
                cases = [
                    ("/sandboxes/target-private?hidden=query-secret", credential, 404),
                    ("/v2/sandboxes/target%2Dprivate/logs", credential, 404),
                    ("/sandboxes/target-private/checkpoints/private-name", "unknown-secret", 401),
                    ("/sandboxes/other-private", credential, 404),
                    ("/sandboxes", credential, 200),
                    ("/sandboxes/" + "x" * 257, credential, 404),
                ]
                for path, token, status in cases: require(request(base, path, token) == status, "unexpected status")
                stop(process)
            raw = log_path.read_bytes()
            for secret in [credential, "target-private", "other-private", "query-secret", "private-name", "unknown-secret"]:
                require(secret.encode() not in raw, "private value leaked")
            result = verifier.verify(raw, key)
            target_summary = verifier.verify(raw, key, "target-private")["sandbox_target"]
            expected_statuses = {"404": 4, "401": 2} if setting == "true" else {}
            require(target_summary["reference"] == reference("target-private") and target_summary["completion_statuses"] == expected_statuses and target_summary["admissions"] == target_summary["completions"] == (6 if setting == "true" else 0), "target lookup mismatch")
            require(result["verified_records"] == 24 and result["uncompleted_admissions"] == 0, "missing requests")
            records = [json.loads(line)["event"] for line in raw.splitlines()]
            for i, event in enumerate(records):
                case = (i // 2) % 6
                expected = reference("target-private") if case < 3 else reference("other-private") if case == 3 else None
                if setting != "true": expected = None
                require(event.get("sandbox_ref") == expected, "reference/decoded/restart mismatch")
                require(("sandbox_ref" in event) == (expected is not None), "default/untargeted field changed")
            if setting == "true":
                # Re-sign malformed events to distinguish schema/correlation checks
                # from generic MAC corruption detection.
                for replacement in ["0" * 64, "a" * 63, "A" * 64, None]:
                    edited = [json.loads(line) for line in raw.splitlines()[:2]]
                    edited[1]["event"]["sandbox_ref"] = replacement
                    previous = bytes(32)
                    lines = []
                    for record in edited:
                        record["prev"] = previous.hex()
                        source = record["source"].encode()
                        canonical = json.dumps(record["event"], sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
                        message = b"HyperMachine audit chain v1\0" + struct.pack(">QQI", record["seq"], record["timestamp_ms"], len(source)) + source + previous + canonical
                        previous = hmac.new(key, message, hashlib.sha256).digest()
                        record["mac"] = previous.hex()
                        lines.append(json.dumps(record).encode())
                    try: verifier.verify(b"\n".join(lines) + b"\n", key)
                    except ValueError: pass
                    else: raise ValueError("re-signed invalid reference accepted")
                report["checks"].append("four-resigned-invalid-reference-records-rejected")
            report["checks"].append(label + "-restart-privacy-decoding-and-correlation")
            report[label] = result
        report["success"] = True
    except Exception as error: report["error"] = str(error)
    finally:
        for process in processes:
            try: stop(process)
            except Exception as error: report["cleanup_errors"].append(str(error))
        report["owned_processes_stopped"] = all(p.poll() is not None for p in processes)
        report["artifact_unchanged"] = hashlib.sha256(binary.read_bytes()).hexdigest() == identity
        report["success"] &= report["owned_processes_stopped"] and report["artifact_unchanged"] and not report["cleanup_errors"]
        key_file.unlink(missing_ok=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report))
    return 0 if report["success"] else 1


if __name__ == "__main__": raise SystemExit(main())
