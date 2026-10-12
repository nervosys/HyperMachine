#!/usr/bin/env python3
"""The Python SDK against a real node on KVM.

Runs an owned hv2-sandboxd with a template and a machine directory, and
drives it with nothing but `sdk/python`'s `hypermachine` package: a machine
made, a command run and waited for, a run followed, a file put in and read
out, a port published and fetched, the machine stopped and resized, a
snapshot taken, read without booting and restored, an environment kept and
a sandbox started with it, usage read, and refusals raised as ApiError.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, sys, tempfile, time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "sdk" / "python"))

from hypermachine import ApiError, Client  # noqa: E402


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of the Python SDK against a node on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "sdk_sha256": digest(HERE.parent / "sdk" / "python" / "hypermachine" / "__init__.py"),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-python-sdk-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--machine-dir", str(work / "machines"), "--machine-ports", "33100-33109",
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    hm = Client(f"http://127.0.0.1:{api}", timeout=300)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                hm.machines.list()
                break
            except (OSError, ApiError):
                assert time.monotonic() < deadline, "daemon readiness"
                time.sleep(0.2)
        state = {}

        def machine_and_commands():
            web = hm.machines.create("sdk-01", disk_gib=1, memory_mb=512, cpu_count=1)
            assert web.state == "running", web
            state["web"] = web
            result = web.exec("echo hello; echo oops >&2; exit 4")
            assert (result.exit_code, result.stdout, result.stderr) == (4, "hello\n", "oops\n"), result
            run = web.runs.start("for i in 1 2 3; do echo line-$i; sleep 0.5; done")
            text = "".join(run.follow(interval=0.2))
            assert text == "line-1\nline-2\nline-3\n" and run.exit_code == 0, (text, run.info)
            return {"machine": web.id}
        case("a machine is made, a command run, a run followed", machine_and_commands)

        def files_and_ports():
            web = state["web"]
            payload = bytes(range(256)) * 16
            web.files.write("/root/www/data.bin", payload)
            assert web.files.read("/root/www/data.bin") == payload
            web.runs.start("exec httpd -f -p 8080 -h /root/www")
            deadline = time.monotonic() + 20
            while "8080" not in web.exec("netstat -ltn").stdout:
                assert time.monotonic() < deadline
                time.sleep(0.2)
            published = web.ports.publish(8080)
            with urllib.request.urlopen(f"http://127.0.0.1:{published['hostPort']}/data.bin", timeout=30) as r:
                assert r.read() == payload
            web.ports.unpublish(published["hostPort"])
            assert web.ports.list() == []
            return {"hostPort": published["hostPort"]}
        case("a file goes in and out, and a published port serves it", files_and_ports)

        def snapshots_and_resize():
            web = state["web"]
            web.exec("echo first > /root/note; sync")
            web.stop()
            snap = web.snapshots.take("first")
            assert snap.read("/root/note") == b"first\n"
            assert "note" in [e["name"] for e in snap.listdir("/root")]
            web.resize(disk_gib=2, cpu_count=2)
            web.start()
            assert web.exec("grep -c ^processor /proc/cpuinfo").stdout == "2\n"
            web.exec("echo second > /root/note; sync")
            web.stop()
            restored = snap.restore()
            assert restored.info["diskGiB"] == 1, restored.info
            web.start()
            assert web.exec("cat /root/note").stdout == "first\n"
            image = work / "disk.img"
            assert snap.download(str(image)) == 1 << 30
            assert image.stat().st_blocks * 512 < 256 << 20, "the download was not sparse"
            return {"snapshot": snap.id}
        case("stopped, resized, snapshotted, read without booting, restored, downloaded sparse",
             snapshots_and_resize)

        def environments_and_usage():
            hm.put_environment("sdk-env", {"GREETING": "hi"}, {"/etc/sdk/flag": b"\x00\x01set"})
            names = [e["name"] for e in hm.environments()]
            assert "sdk-env" in names, names
            created = hm.call("POST", "/sandboxes", {"templateID": "base", "environment": "sdk-env"})
            sandbox = created["sandboxID"]
            out = hm.call("POST", f"/sandboxes/{sandbox}/exec",
                          {"cmd": 'echo "$GREETING"; base64 /etc/sdk/flag'})
            # b"\x00\x01set", exactly.
            assert out["stdout"] == "hi\nAAFzZXQ=\n", out
            time.sleep(1)
            usage = hm.usage(by="sandbox")
            mine = [g for g in usage["groups"] if g["key"] == sandbox]
            assert mine and mine[0]["runningSeconds"] > 0, usage
            hm.call("DELETE", f"/sandboxes/{sandbox}")
            hm.delete_environment("sdk-env")
            return {"usage_seconds": mine[0]["runningSeconds"]}
        case("an environment starts a sandbox, and its usage is read", environments_and_usage)

        def refusals():
            results = {}
            for label, action, status in [
                ("a machine that is not there", lambda: hm.machines.get("absent-99"), 404),
                ("a second machine of one name", lambda: hm.machines.create("sdk-01", disk_gib=1), 409),
                ("a smaller disk", lambda: (state["web"].stop(), state["web"].resize(disk_gib=1)), None),
            ]:
                try:
                    action()
                    results[label] = "no refusal"
                except ApiError as error:
                    assert status is None or error.status == status, (label, error)
                    results[label] = f"{error.status} {error.message}"
            assert results["a smaller disk"] == "no refusal", "a disk of the size it has is no change"
            try:
                state["web"].resize(disk_gib=0)
                raise AssertionError("a disk of 0 GiB was accepted")
            except ApiError as error:
                assert error.status == 400, error
                results["no disk at all"] = f"{error.status} {error.message}"
            state["web"].delete()
            return results
        case("refusals are raised as ApiError with the node's words", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
