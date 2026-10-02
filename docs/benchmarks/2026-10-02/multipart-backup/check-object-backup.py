#!/usr/bin/env python3
"""Exercise encrypted backups on an owned S3 emulator and optionally real KVM."""
import argparse
import contextlib
import fcntl
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import socket
import struct
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request


def require(condition, message):
    if not condition: raise ValueError(message)


def digest(path):
    with path.open("rb") as stream: return hashlib.file_digest(stream, "sha256").hexdigest()


def port():
    with socket.socket() as probe: probe.bind(("127.0.0.1", 0)); return probe.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--daemon", type=Path)
    parser.add_argument("--kernel", type=Path)
    parser.add_argument("--initrd", type=Path)
    parser.add_argument("--multipart", action="store_true", help="exercise multipart upload for the KVM backup")
    args = parser.parse_args()
    require(all([args.daemon, args.kernel, args.initrd]) or not any([args.daemon, args.kernel, args.initrd]), "KVM paths must be supplied together")
    os.umask(0o077)
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    tool = Path(__file__).with_name("backup-snapshot-store.py")
    spec = importlib.util.spec_from_file_location("backup", tool)
    backup = importlib.util.module_from_spec(spec); spec.loader.exec_module(backup)
    paths = {"tool": tool, "coordinator": Path(__file__)}
    for name in ["daemon", "kernel", "initrd"]:
        if getattr(args, name): paths[name] = getattr(args, name).resolve(strict=True)
    import boto3, botocore, cryptography, moto
    report = {"success": False, "performance_comparison": False, "storage": "owned Moto S3 HTTP emulator; no managed-store durability claim",
        "artifact_sha256": {n: digest(p) for n, p in paths.items()}, "versions": {"boto3": boto3.__version__, "botocore": botocore.__version__, "cryptography": cryptography.__version__, "moto": moto.__version__},
        "checks": [], "cleanup_errors": [], "processes_stopped": []}
    credential, aws_id, aws_secret = secrets.token_urlsafe(32), secrets.token_urlsafe(16), secrets.token_urlsafe(32)
    key = args.output / "backup.key"; key.write_text("42" * 32)
    wrong = args.output / "wrong.key"; wrong.write_text("43" * 32)
    env = {"PATH": "/usr/bin:/bin", "RUST_LOG": "warn", "AWS_ACCESS_KEY_ID": aws_id, "AWS_SECRET_ACCESS_KEY": aws_secret,
           "AWS_EC2_METADATA_DISABLED": "true", "AWS_DEFAULT_REGION": "us-east-1", "HV2_CLUSTER_TOKEN": credential}
    processes = []
    endpoint = "http://127.0.0.1:" + str(port())
    bucket = "hm-owned-backup-fixture"
    client = boto3.client("s3", endpoint_url=endpoint, region_name="us-east-1", aws_access_key_id=aws_id,
                         aws_secret_access_key=aws_secret, config=botocore.config.Config(signature_version="s3v4", s3={"addressing_style": "path"}, retries={"total_max_attempts": 1}))

    def start_process(name, command, environment):
        with (args.output / (name + ".log")).open("wb") as log:
            process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        processes.append((name, process)); return process

    def stop(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)

    def invoke(operation, store=None, destination=None, object="synthetic.hmb", key_file=key, extra=(), expected=True):
        command = [sys.executable, str(tool), operation, "--bucket", bucket, "--object", object,
                   "--endpoint", endpoint, "--key-file", str(key_file)]
        command += ["--store", str(store)] if operation == "backup" else ["--destination", str(destination)]
        result = subprocess.run(command + list(extra), env=env, capture_output=True, timeout=180)
        for secret in [credential, aws_id, aws_secret, key.read_text()]:
            require(secret.encode() not in result.stdout + result.stderr, "secret leaked into tool output")
        require((result.returncode == 0) == expected, "unexpected backup command result: " + (result.stdout + result.stderr).decode(errors="replace"))
        value = json.loads(result.stdout if expected else result.stderr)
        require(value["success"] == expected, "tool success mismatch")
        return value

    def object_bytes(name):
        response = client.get_object(Bucket=bucket, Key=name)
        with response["Body"] as body: return body.read()

    def forged(name, catalog, entries):
        plaintext, encrypted = args.output / "forged.tar.gz", args.output / "forged.hmb"
        with tarfile.open(plaintext, "w:gz") as archive:
            data = backup.canonical(catalog); entry = tarfile.TarInfo(backup.MANIFEST); entry.size = len(data)
            archive.addfile(entry, io.BytesIO(data))
            for member_name, content, kind in entries:
                entry = tarfile.TarInfo(member_name)
                if kind == "symlink": entry.type, entry.linkname = tarfile.SYMTYPE, "../../escape"
                else: entry.size = len(content)
                archive.addfile(entry, None if kind == "symlink" else io.BytesIO(content))
        backup.encrypt(plaintext, encrypted, bytes.fromhex(key.read_text()))
        client.put_object(Bucket=bucket, Key=name, Body=encrypted.read_bytes())
        target = args.output / ("reject-" + name)
        value = invoke("restore", destination=target, object=name, expected=False)
        require(not target.exists(), "invalid archive published")
        report["checks"].append({"name": name, "refused": True, "error": value["error"]})
        plaintext.unlink(); encrypted.unlink()

    def start_daemon(label, store):
        api, proxy = port(), port()
        while api == proxy: proxy = port()
        command = [str(paths["daemon"]), "--port", str(api), "--proxy-port", str(proxy), "--snapshot-store", str(store),
                   "--volume-dir", str(store / "volumes"), "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "4"]
        process = start_process(label, command, dict(env, HV2_KERNEL=str(paths["kernel"]), HV2_INITRD=str(paths["initrd"])))
        base = f"http://127.0.0.1:{api}"
        deadline = time.monotonic() + 60
        while True:
            require(process.poll() is None, "daemon exited during startup")
            try:
                if request(base, "GET", "/templates")[0] == 200: return process, base
            except OSError: pass
            require(time.monotonic() < deadline, "daemon readiness timeout")
            time.sleep(.05)

    def request(base, method, path, body=None):
        request = urllib.request.Request(base + path, method=method, headers={"x-hv2-cluster-token": credential,
            "content-type": "application/json"}, data=None if body is None else json.dumps(body).encode())
        try: response = urllib.request.urlopen(request, timeout=45)
        except urllib.error.HTTPError as error: response = error
        with response:
            raw = response.read(); return response.status, json.loads(raw) if raw else None

    def execute(base, sandbox, command):
        status, value = request(base, "POST", "/sandboxes/" + sandbox + "/exec", {"cmd": command, "timeout_secs": 10})
        require(status == 200 and value["exit_code"] == 0 and not value.get("timed_out") and not value.get("truncated"), "guest command failed")
        return value["stdout"]

    try:
        server = Path(sys.executable).with_name("moto_server")
        process = start_process("s3", [str(server), "-H", "127.0.0.1", "-p", endpoint.rsplit(":", 1)[1]], env)
        deadline = time.monotonic() + 15
        while True:
            require(process.poll() is None, "S3 emulator exited")
            try: client.create_bucket(Bucket=bucket); break
            except botocore.exceptions.EndpointConnectionError: pass
            require(time.monotonic() < deadline, "S3 emulator readiness timeout"); time.sleep(.05)
        source = args.output / "synthetic-store"
        (source / "templates/base").mkdir(parents=True)
        (source / "paused").mkdir(); (source / "volumes/empty").mkdir(parents=True)
        (source / backup.LOCK).touch()
        (source / "templates/base/memory.raw").write_bytes(b"private-memory" * 100 + bytes(4096))
        (source / "volumes/data").write_bytes(b"private-volume" * 100)
        (source / "volumes/user.snap").write_bytes(b"application snapshot, not a VM header")
        os.chmod(source / "volumes/data", 0o640)
        header = {"memory_base": str(source / "templates/base/memory.raw"), "memory_image": None}
        encoded = backup.canonical(header)
        (source / "paused/sbx-fixture.snap").write_bytes(b"HV2SNAP\0" + struct.pack("<II", 2, len(encoded)) + encoded + b"payload-pages")
        manifest = backup.scan(source, 1024**3)
        with (source / backup.LOCK).open("r+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
            value = invoke("backup", store=source, expected=False)
            require("stop every" in value["error"], "active store lock ignored")
        report["checks"].append({"name": "active-store-refused", "refused": True})
        first = invoke("backup", store=source)
        raw = object_bytes("synthetic.hmb")
        require(first["sha256"] == hashlib.sha256(raw).hexdigest() and b"private-memory" not in raw and b"private-volume" not in raw, "object receipt/privacy mismatch")
        invoke("backup", store=source, expected=False)
        require(object_bytes("synthetic.hmb") == raw, "duplicate backup overwrote object")
        report["checks"].append({"name": "encrypted-conditional-upload", "sha256": first["sha256"], "bytes": len(raw)})
        destination = args.output / "synthetic-recovered"
        restored = invoke("restore", destination=destination)
        require((destination / "volumes/data").read_bytes() == (source / "volumes/data").read_bytes(), "volume bytes changed")
        require((destination / "volumes/user.snap").read_bytes() == (source / "volumes/user.snap").read_bytes(), "ordinary .snap volume file misinterpreted")
        require((destination / "volumes/empty").is_dir() and (destination / "volumes/data").stat().st_mode & 0o777 == 0o640, "directory or permissions changed")
        relocated, _ = backup.snapshot_header(destination / "paused/sbx-fixture.snap")
        require(relocated["memory_base"] == str(destination / "templates/base/memory.raw"), "base not relocated")
        require((destination / "paused/sbx-fixture.snap").read_bytes().endswith(b"payload-pages"), "snapshot payload changed")
        before = backup.scan(destination, 1024**3)
        invoke("restore", destination=destination, expected=False)
        require(backup.scan(destination, 1024**3) == before, "existing restore target modified")
        report["checks"].append({"name": "relocated-roundtrip-with-empty-directory-and-permissions", "files": restored["files"]})
        invoke("restore", destination=args.output / "wrong-key-target", key_file=wrong, expected=False)
        require(not (args.output / "wrong-key-target").exists(), "wrong key published")
        for label, corrupt in [("tampered", raw[:30] + bytes([raw[30] ^ 1]) + raw[31:]), ("truncated", raw[:-1])]:
            client.put_object(Bucket=bucket, Key=label, Body=corrupt)
            invoke("restore", destination=args.output / (label + "-target"), object=label, expected=False)
            require(not (args.output / (label + "-target")).exists(), "unauthenticated backup published")
        report["checks"].append({"name": "wrong-key-tampered-truncated-refused", "refused": True})
        invoke("restore", destination=args.output / "oversized-target", extra=["--max-expanded-bytes", "1"], expected=False)
        require(not (args.output / "oversized-target").exists(), "expanded limit ignored")
        report["checks"].append({"name": "expanded-byte-limit-refused", "refused": True})
        minimal = {"version": 1, "source_root": str(source), "files": {"data": {"size": 1, "sha256": hashlib.sha256(b"x").hexdigest(), "mode": 0o600}}, "directories": {}, "expanded_bytes": 1}
        unsafe = json.loads(json.dumps(minimal)); unsafe["files"]["../escape"] = unsafe["files"].pop("data")
        forged("unsafe-path", unsafe, [("store/../escape", b"x", "file")])
        forged("symlink", minimal, [("store/data", b"", "symlink")])
        forged("missing-file", minimal, [])
        forged("wrong-digest", minimal, [("store/data", b"y", "file")])
        forged("extra-file", minimal, [("store/data", b"x", "file"), ("store/extra", b"x", "file")])
        forged("duplicate-file", minimal, [("store/data", b"x", "file"), ("store/data", b"x", "file")])
        (source / "volumes/link").symlink_to(source / "volumes/data")
        invoke("backup", store=source, object="symlink-source", expected=False); (source / "volumes/link").unlink()
        (source / "paused/sbx-fixture.json.claimed-node").write_text("{}")
        invoke("backup", store=source, object="claimed-source", expected=False); (source / "paused/sbx-fixture.json.claimed-node").unlink()
        report["checks"].append({"name": "source-symlink-and-unfinished-claim-refused", "refused": True})
        require(backup.scan(source, 1024**3) == manifest, "synthetic source changed")
        invoke("restore", destination=args.output / "receipt-mismatch-target", extra=["--sha256", "0" * 64], expected=False)
        require(not (args.output / "receipt-mismatch-target").exists(), "receipt mismatch published")
        report["checks"].append({"name": "independent-receipt-mismatch-refused", "refused": True})
        class LostUploadResponse:
            def put_object(self, **kwargs):
                client.put_object(**kwargs)
                raise RuntimeError("injected client acknowledgement loss after S3 commit")
        original_client, original_argv = backup.client, sys.argv
        output = io.StringIO()
        try:
            backup.client = lambda endpoint, region: LostUploadResponse()
            sys.argv = [str(tool), "backup", "--store", str(source), "--bucket", bucket,
                        "--object", "lost-response.hmb", "--endpoint", endpoint, "--key-file", str(key)]
            with contextlib.redirect_stderr(output):
                require(backup.main() == 1, "lost upload response reported success")
        finally:
            backup.client, sys.argv = original_client, original_argv
        uncertain = json.loads(output.getvalue())
        require(not uncertain["success"] and not uncertain["upload_confirmed"], "lost response uncertainty hidden")
        attempt = uncertain["attempt_receipt"]
        uploaded = object_bytes("lost-response.hmb")
        require(attempt["sha256"] == hashlib.sha256(uploaded).hexdigest() and attempt["encrypted_bytes"] == len(uploaded), "failed upload receipt unusable")
        invoke("restore", destination=args.output / "lost-response-recovered", object="lost-response.hmb", extra=["--sha256", attempt["sha256"]])
        report["checks"].append({"name": "lost-upload-response-retains-verifiable-receipt", "passed": True,
                                 "injection": "client acknowledgement loss after real emulator commit", "receipt": attempt})
        if args.daemon:
            store, recovered = args.output / "kvm-store", args.output / "kvm-recovered"
            daemon, base = start_daemon("node-original", store)
            # Both lock directions are checked against the shipped native daemon.
            value = invoke("backup", store=store, object="active-kvm", expected=False)
            require("stop every" in value["error"], "native shared lock ignored")
            status, volume = request(base, "POST", "/volumes", {"name": "backup-fixture"})
            require(status == 201, "volume create failed")
            status, created = request(base, "POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600,
                "volumeMounts": [{"name": "backup-fixture", "path": "/mnt/backup"}]})
            require(status == 201, "guest create failed")
            sandbox = created["sandboxID"]
            marker = secrets.token_hex(24)
            prepare = "mkdir /tmp/backup-state && printf '%s' " + shlex.quote(marker) + " > /tmp/backup-state/state && cat /proc/sys/kernel/random/boot_id > /tmp/backup-state/boot && { env HM_BACKUP_MEMORY=" + marker + " sleep 86400 </dev/null >/dev/null 2>&1 & echo $! > /tmp/backup-state/pid; }"
            execute(base, sandbox, prepare + " && printf %s " + marker + " > /mnt/backup/marker && printf application-file > /mnt/backup/user.snap")
            verify = "test \"$(cat /tmp/backup-state/state)\" = " + marker + " && test \"$(cat /tmp/backup-state/boot)\" = \"$(cat /proc/sys/kernel/random/boot_id)\" && kill -0 \"$(cat /tmp/backup-state/pid)\" && tr '\\000' '\\n' < /proc/$(cat /tmp/backup-state/pid)/environ | grep -Fx HM_BACKUP_MEMORY=" + marker + " >/dev/null"
            execute(base, sandbox, "for n in 1 2 3 4 5 6 7 8 9 10; do " + verify + " && exit 0; sleep .1; done; exit 1")
            require(request(base, "POST", f"/sandboxes/{sandbox}/snapshots", {"name": "backup-template"})[0] == 201, "named snapshot create failed")
            require(request(base, "POST", f"/sandboxes/{sandbox}/pause", {})[0] == 204, "guest pause failed")
            stop(daemon)
            original_header, _ = backup.snapshot_header(store / "paused" / (sandbox + ".snap"))
            require(original_header.get("memory_base") is not None and Path(original_header["memory_base"]).is_relative_to(store), "KVM layered memory base not exercised")
            with (store / backup.LOCK).open("r+b") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                rejected = subprocess.run([str(paths["daemon"]), "--snapshot-store", str(store), "--no-template"], env=dict(env, HV2_KERNEL=str(paths["kernel"]), HV2_INITRD=str(paths["initrd"])), capture_output=True, timeout=10)
                require(rejected.returncode != 0 and b"locked for offline" in rejected.stderr, "native daemon ignored offline lock")
                (args.output / "locked-startup.log").write_bytes(rejected.stdout + rejected.stderr)
            require((store / "volumes" / volume["volumeID"] / "data/marker").read_text() == marker, "guest volume write not persisted")
            receipt = invoke("backup", store=store, object="kvm.hmb", extra=["--multipart-threshold-mib", "8", "--multipart-part-mib", "8"] if args.multipart else [])
            if args.multipart: require(receipt["upload_method"] == "multipart" and receipt["parts"] >= 2, "KVM multipart path not exercised")
            # Make original files unavailable; recovery must use the S3 object.
            original = args.output / "kvm-source-unavailable"
            store.rename(original)
            recovery_receipt = invoke("restore", destination=recovered, object="kvm.hmb", extra=["--sha256", receipt["sha256"]])
            require(recovery_receipt["receipt_checksum_verified"], "KVM backup receipt not verified")
            recovered_header, _ = backup.snapshot_header(recovered / "paused" / (sandbox + ".snap"))
            require(recovered_header["memory_base"] == str(recovered / Path(original_header["memory_base"]).relative_to(store)), "KVM memory base not relocated")
            require(not store.exists() and (recovered / "volumes" / volume["volumeID"] / "data/marker").read_text() == marker, "recovery reused original store or lost volume")
            daemon, base = start_daemon("node-recovered", recovered)
            status, connected = request(base, "POST", f"/sandboxes/{sandbox}/connect", {"timeout": 300})
            require(status == 201 and connected["sandboxID"] == sandbox and connected["envdAccessToken"] == created["envdAccessToken"], "guest identity changed during recovery")
            require(execute(base, sandbox, verify + " && test \"$(cat /mnt/backup/marker)\" = " + marker + " && printf recovered-state-ok") == "recovered-state-ok", "live memory/process/filesystem state lost")
            require(execute(base, sandbox, "cat /mnt/backup/user.snap") == "application-file", "ordinary .snap volume file lost")
            status, child = request(base, "POST", "/v2/sandboxes", {"templateID": "backup-template", "timeout": 300,
                "volumeMounts": [{"name": "backup-fixture", "path": "/mnt/backup"}]})
            require(status == 201 and child["sandboxID"] != sandbox, "recovered named snapshot create failed")
            require(execute(base, child["sandboxID"], verify + " && test \"$(cat /mnt/backup/marker)\" = " + marker + " && printf recovered-template-ok") == "recovered-template-ok", "recovered named snapshot lost state")
            require(request(base, "DELETE", f"/sandboxes/{child['sandboxID']}")[0] == 204, "snapshot child cleanup failed")
            require(request(base, "DELETE", f"/sandboxes/{sandbox}")[0] == 204, "recovered guest delete failed")
            require(request(base, "GET", "/sandboxes")[1] == [], "recovered guest inventory not empty")
            status, recovered_volume = request(base, "GET", "/volumes/" + volume["volumeID"])
            require(status == 200 and recovered_volume == volume, "volume identity or token changed")
            require(request(base, "DELETE", "/volumes/" + volume["volumeID"])[0] == 204, "volume cleanup failed")
            require(request(base, "GET", "/volumes")[1] == [], "volume inventory not empty")
            stop(daemon)
            report["kvm"] = {"same_id_and_access_token": True, "live_process_and_memory_marker": True, "filesystem_and_boot_id": True,
                "volume_marker": True, "guest_9p_volume_roundtrip": True, "same_volume_id_and_token": True, "volume_inventory_empty": True, "original_path_unavailable": True, "cleanup_inventory_empty": True, "backup_receipt": receipt, "receipt_checksum_verified": True, "layered_memory_base_relocated": True, "named_snapshot_recovered": True, "ordinary_volume_snapshot_filename_preserved": True}
            report["checks"].append({"name": "real-kvm-state-recovered-through-s3-at-new-root", "passed": True})
        report["success"] = True
    except Exception as error:
        report["error"] = str(error)
    finally:
        for name, process in reversed(processes):
            try: stop(process)
            except Exception as error: report["cleanup_errors"].append(str(error))
            report["processes_stopped"].append({"name": name, "exit_code": process.poll()})
        report["artifacts_unchanged"] = all(digest(p) == report["artifact_sha256"][n] for n, p in paths.items())
        report["success"] &= report["artifacts_unchanged"] and not report["cleanup_errors"] and all(p["exit_code"] is not None for p in report["processes_stopped"])
        key.unlink(missing_ok=True); wrong.unlink(missing_ok=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report), flush=True)
    return 0 if report["success"] else 1


if __name__ == "__main__": raise SystemExit(main())
