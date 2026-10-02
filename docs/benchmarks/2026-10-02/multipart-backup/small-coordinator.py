#!/usr/bin/env python3
"""Verify owned multipart S3 uploads, scoped aborts, and ambiguous completion."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time


def require(condition, message):
    if not condition: raise ValueError(message)


def digest(path):
    with path.open("rb") as stream: return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--large", action="store_true", help="also transfer and recover a ciphertext larger than the former 5,000,000,000-byte limit")
    args = parser.parse_args()
    args.output = args.output.resolve(); args.output.mkdir(parents=True, exist_ok=False)
    os.umask(0o077)
    tool = Path(__file__).with_name("backup-snapshot-store.py")
    spec = importlib.util.spec_from_file_location("backup", tool)
    backup = importlib.util.module_from_spec(spec); spec.loader.exec_module(backup)
    import boto3, botocore, moto, cryptography
    report = {"success": False, "performance_comparison": False, "storage": "owned Moto S3 HTTP emulator",
        "tool_sha256": digest(tool), "coordinator_sha256": digest(Path(__file__)), "checks": [], "cleanup_errors": [],
        "versions": {"boto3": boto3.__version__, "botocore": botocore.__version__, "moto": moto.__version__, "cryptography": cryptography.__version__}}
    with socket.socket() as probe: probe.bind(("127.0.0.1", 0)); api = probe.getsockname()[1]
    endpoint, bucket = f"http://127.0.0.1:{api}", "hm-owned-multipart-fixture"
    key = args.output / "fixture.key"; key.write_text("42" * 32)
    credential = secrets.token_urlsafe(32)
    client = boto3.client("s3", endpoint_url=endpoint, region_name="us-east-1", aws_access_key_id="owned-fixture",
        aws_secret_access_key=credential, config=botocore.config.Config(signature_version="s3v4", s3={"addressing_style": "path"}, retries={"total_max_attempts": 1}))
    env = {"PATH": "/usr/bin:/bin", "AWS_EC2_METADATA_DISABLED": "true"}
    with (args.output / "s3.log").open("wb") as log:
        process = subprocess.Popen([str(Path(sys.executable).with_name("moto_server")), "-H", "127.0.0.1", "-p", str(api)], env=env, stdout=log, stderr=log)
    source = args.output / "source"; source.mkdir(); (source / backup.LOCK).touch()
    with (source / "data").open("wb") as stream:
        for _ in range(20): stream.write(os.urandom(1024**2))
    source_hash = digest(source / "data")
    parent_client = backup.client

    def invoke(operation, object, adapter=client, expected=0, destination=None, sha=None, extra=()):
        old_argv, old_client = sys.argv, backup.client
        sys.argv = [str(tool), operation, "--bucket", bucket, "--object", object, "--endpoint", endpoint, "--key-file", str(key)]
        if operation == "backup": sys.argv += ["--store", str(source), "--multipart-threshold-mib", "8", "--multipart-part-mib", "8"]
        else: sys.argv += ["--destination", str(destination), "--sha256", sha]
        sys.argv += list(extra)
        output, error = io.StringIO(), io.StringIO()
        backup.client = lambda endpoint, region: adapter
        try:
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(error): result = backup.main()
        finally: backup.client, sys.argv = old_client, old_argv
        require(result == expected, "unexpected multipart command outcome: " + error.getvalue())
        require(credential not in output.getvalue() + error.getvalue() and key.read_text() not in output.getvalue() + error.getvalue(), "secret in tool output")
        value = json.loads(output.getvalue() if expected == 0 else error.getvalue())
        require(value["success"] == (expected == 0), "multipart result mismatch")
        return value

    def object_hash(name):
        response = client.get_object(Bucket=bucket, Key=name)
        value, count = hashlib.sha256(), 0
        with response["Body"] as stream:
            while block := stream.read(1024**2): value.update(block); count += len(block)
        return value.hexdigest(), count

    def uploads():
        return client.list_multipart_uploads(Bucket=bucket).get("Uploads", [])

    class Fault:
        def __init__(self, mode): self.mode, self.owned_id = mode, None
        def create_multipart_upload(self, **kwargs):
            result = client.create_multipart_upload(**kwargs); self.owned_id = result["UploadId"]
            if self.mode == "create-response-loss": raise RuntimeError("injected acknowledgement loss after create")
            return result
        def upload_part(self, **kwargs):
            if kwargs["PartNumber"] == 2:
                if self.mode in ["part-failure", "abort-failure"]: raise RuntimeError("injected part failure")
                if self.mode == "interrupt": raise KeyboardInterrupt()
            result = client.upload_part(**kwargs)
            if self.mode == "part-checksum-mismatch": result["ChecksumSHA256"] = "wrong"
            return result
        def complete_multipart_upload(self, **kwargs):
            result = client.complete_multipart_upload(**kwargs)
            if self.mode == "complete-response-loss": raise RuntimeError("injected acknowledgement loss after complete commit")
            if self.mode == "complete-checksum-mismatch": result["ChecksumSHA256"] = "wrong"
            return result
        def abort_multipart_upload(self, **kwargs):
            require(kwargs["UploadId"] == self.owned_id, "aborted another invocation's upload")
            if self.mode == "abort-failure": raise RuntimeError("injected abort failure")
            return client.abort_multipart_upload(**kwargs)

    try:
        deadline = time.monotonic() + 15
        while True:
            require(process.poll() is None, "emulator exited during startup")
            try: client.create_bucket(Bucket=bucket); break
            except botocore.exceptions.EndpointConnectionError: pass
            require(time.monotonic() < deadline, "emulator readiness timeout"); time.sleep(.05)
        receipt = invoke("backup", "success.hmb")
        require(receipt["upload_method"] == "multipart" and receipt["parts"] == 3, "multipart parts not exercised")
        require(object_hash("success.hmb") == (receipt["sha256"], receipt["encrypted_bytes"]), "whole-object receipt mismatch")
        response = client.head_object(Bucket=bucket, Key="success.hmb", ChecksumMode="ENABLED")
        require(response["ChecksumSHA256"] == receipt["composite_sha256"] and response["ChecksumSHA256"] != receipt["sha256"], "composite checksum confused with whole-object hash")
        destination = args.output / "recovered"
        invoke("restore", "success.hmb", destination=destination, sha=receipt["sha256"])
        require(digest(destination / "data") == source_hash, "multipart recovery changed plaintext")
        duplicate = invoke("backup", "success.hmb", expected=1)
        require(duplicate["multipart_cleanup"]["status"] == "aborted" and object_hash("success.hmb") == (receipt["sha256"], receipt["encrypted_bytes"]), "multipart overwrite or incomplete cleanup")
        require(not uploads(), "successful/duplicate uploads left sessions")
        report["checks"].append({"name": "multipart-roundtrip-composite-and-no-overwrite", "receipt": receipt, "passed": True})
        foreign = client.create_multipart_upload(Bucket=bucket, Key="scope.hmb")["UploadId"]
        fault = Fault("part-failure")
        value = invoke("backup", "scope.hmb", adapter=fault, expected=1)
        require(value["multipart_cleanup"]["status"] == "aborted" and [u["UploadId"] for u in uploads()] == [foreign], "abort was not scoped to this invocation")
        client.abort_multipart_upload(Bucket=bucket, Key="scope.hmb", UploadId=foreign)
        report["checks"].append({"name": "part-failure-preserves-other-upload", "passed": True})
        for mode in ["part-checksum-mismatch", "abort-failure", "create-response-loss", "interrupt", "complete-response-loss", "complete-checksum-mismatch"]:
            fault = Fault(mode)
            value = invoke("backup", mode + ".hmb", adapter=fault, expected=130 if mode == "interrupt" else 1)
            status = value["multipart_cleanup"]["status"]
            expected = "abort_failed" if mode == "abort-failure" else "upload_id_unavailable" if mode == "create-response-loss" else "already_completed_or_absent" if mode.startswith("complete-") else "aborted"
            require(status == expected, "multipart uncertainty/cleanup misclassified")
            if mode.startswith("complete-"):
                attempt = value["attempt_receipt"]
                require(object_hash(mode + ".hmb") == (attempt["sha256"], attempt["encrypted_bytes"]), "ambiguous completion receipt wrong")
                invoke("restore", mode + ".hmb", destination=args.output / (mode + "-recovered"), sha=attempt["sha256"])
            elif mode in ["abort-failure", "create-response-loss"]:
                require([u["UploadId"] for u in uploads()] == [fault.owned_id], "unknown/failed-abort upload not preserved")
                # Fixture-only operator cleanup of its known injected orphan.
                client.abort_multipart_upload(Bucket=bucket, Key=mode + ".hmb", UploadId=fault.owned_id)
            require(not uploads(), "fixture left multipart session")
            report["checks"].append({"name": mode, "passed": True, "cleanup_status": status,
                                     "injection": "owned client adapter around real emulator operations"})
        for flag, value in [("--multipart-part-mib", "7"), ("--multipart-part-mib", "129"), ("--multipart-threshold-mib", "0"), ("--multipart-threshold-mib", "4097")]:
            result = invoke("backup", "invalid.hmb", extra=[flag, value], expected=1)
            require(result["error"].startswith("multipart"), "invalid upload setting accepted")
        sparse = args.output / "oversized-plaintext"
        with sparse.open("wb") as stream: stream.truncate(backup.MAX_OBJECT)
        try: backup.encrypt(sparse, args.output / "oversized.hmb", bytes.fromhex(key.read_text()))
        except ValueError: pass
        else: raise ValueError("GCM format bound ignored")
        require(not (args.output / "oversized.hmb").exists(), "oversized plaintext was read/encrypted")
        sparse.unlink()
        report["checks"].append({"name": "configuration-and-format-limits", "passed": True})
        if args.large:
            # A repeated 16 MiB random block remains incompressible to deflate's
            # 32 KiB window, without keeping a multi-gigabyte source in memory.
            size, block = 5_001_000_000, os.urandom(16 * 1024**2)
            with (source / "data").open("wb") as stream:
                remaining = size
                while remaining: value = block[:min(len(block), remaining)]; stream.write(value); remaining -= len(value)
            source_hash = digest(source / "data")
            large = invoke("backup", "large.hmb", extra=["--multipart-threshold-mib", "64", "--multipart-part-mib", "64"])
            require(large["encrypted_bytes"] > 5_000_000_000 and large["upload_method"] == "multipart" and large["parts"] >= 75, "former size limit not crossed")
            require(object_hash("large.hmb") == (large["sha256"], large["encrypted_bytes"]), "large ciphertext receipt mismatch")
            invoke("restore", "large.hmb", destination=args.output / "large-recovered", sha=large["sha256"])
            require(digest(args.output / "large-recovered/data") == source_hash, "large multipart recovery changed data")
            report["large"] = dict(large, source_bytes=size, plaintext_sha256=source_hash, recovered_hash_identical=True)
        report["success"] = True
    except Exception as error: report["error"] = str(error)
    finally:
        backup.client = parent_client
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
        report["emulator_exit_code"] = process.poll()
        report["artifacts_unchanged"] = digest(tool) == report["tool_sha256"] and digest(Path(__file__)) == report["coordinator_sha256"]
        report["success"] &= report["emulator_exit_code"] == 0 and report["artifacts_unchanged"]
        key.unlink(missing_ok=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report), flush=True)
    return 0 if report["success"] else 1


if __name__ == "__main__": raise SystemExit(main())
