#!/usr/bin/env python3
"""Snapshots of a machine's disk on real KVM: taken, restored, downloaded.

Runs an owned hv2-sandboxd with a --machine-dir and one machine from the base
template, then checks, through its API and from inside the guest:

1. A snapshot of a running machine is refused.
2. Stopped, a snapshot is taken, and listed with what it holds on the node.
3. After the guest changes and adds files, restoring the snapshot brings
   the disk back: the change undone and the new file gone.
4. Restoring the later snapshot brings the change back: snapshots are
   independent of each other and of the disk.
5. A snapshot's downloaded disk is a filesystem holding the files as they
   were when it was taken, read without booting anything.
6. A snapshot taken before a resize restores the smaller disk size.
7. What cannot be done is refused; deleting a snapshot removes it, and
   deleting the machine removes them all.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "snapped-01"


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
              "purpose": "functional verification of machine disk snapshots on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-snapshots-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=300):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, (cmd, status, value)
        return value["stdout"]

    def power(action, want):
        status, machine = request("POST", f"/machines/{NAME}/{action}")
        assert status == 200 and machine["state"] == want, (action, status, machine)
        return machine

    def take(label):
        status, snap = request("POST", f"/machines/{NAME}/snapshots", {"name": label})
        assert status == 201, (status, snap)
        return snap

    def restore(snap_id):
        status, machine = request("POST", f"/machines/{NAME}/snapshots/{snap_id}/restore", {})
        assert status == 200, (status, machine)
        return machine

    def files():
        return guest("cat /root/note; ls /root/extra 2>/dev/null || echo no-extra")

    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1})
        assert status == 201 and created["state"] == "running", (status, created)
        guest("echo first version > /root/note; dd if=/dev/urandom of=/root/data bs=1k count=300 2>/dev/null; sync")
        state = {"data": guest("sha256sum /root/data").split()[0]}

        def running_refused():
            status, value = request("POST", f"/machines/{NAME}/snapshots", {"name": "x"})
            assert status == 409 and "stop it" in value["message"], (status, value)
            status, listed = request("GET", f"/machines/{NAME}/snapshots")
            assert listed == [], listed
            return {"refused": value["message"]}
        case("a snapshot of a running machine is refused", running_refused)

        def taken():
            power("stop", "stopped")
            first = take("first version")
            assert first["name"] == "first version" and first["diskGiB"] == 1, first
            assert first["snapshotID"].startswith("snap-"), first
            # A sparse copy: far less than the disk's gibibyte.
            assert 0 < first["bytesOnNode"] < 512 << 20, first
            status, listed = request("GET", f"/machines/{NAME}/snapshots")
            assert [s["snapshotID"] for s in listed] == [first["snapshotID"]], listed
            status, alone = request("GET", f"/machines/{NAME}/snapshots/{first['snapshotID']}")
            assert status == 200 and alone["name"] == "first version", alone
            state["first"] = first["snapshotID"]
            return {"snapshot": first}
        case("stopped, a snapshot is taken and listed, with what it holds on the node", taken)

        def restored():
            power("start", "running")
            guest("echo second version > /root/note; echo here > /root/extra; rm /root/data; sync")
            assert files() == "second version\n/root/extra\n"
            power("stop", "stopped")
            state["second"] = take("second version")["snapshotID"]
            restore(state["first"])
            power("start", "running")
            assert files() == "first version\nno-extra\n", files()
            assert guest("sha256sum /root/data").split()[0] == state["data"]
            return None
        case("restoring a snapshot undoes what changed since", restored)

        def and_back():
            power("stop", "stopped")
            restore(state["second"])
            power("start", "running")
            assert files() == "second version\n/root/extra\n", files()
            assert "No such file" in request("POST", f"/machines/{NAME}/exec",
                                             {"cmd": "cat /root/data", "timeout_secs": 30})[1]["stderr"]
            # And the first again: each snapshot is its own.
            power("stop", "stopped")
            restore(state["first"])
            power("start", "running")
            assert files() == "first version\nno-extra\n", files()
            return None
        case("restoring the later snapshot brings it back; each snapshot is its own", and_back)

        def downloaded():
            image = work / "downloaded.img"
            url = f"{base}/machines/{NAME}/snapshots/{state['second']}/disk"
            with urllib.request.urlopen(url, timeout=600) as response, image.open("wb") as out:
                assert response.headers["content-type"] == "application/octet-stream"
                length = int(response.headers["content-length"])
                shutil.copyfileobj(response, out, 1 << 20)
            assert image.stat().st_size == length == 1 << 30, (image.stat().st_size, length)
            # Read without booting it: debugfs reads ext4 from the image.
            note = subprocess.run(["debugfs", "-R", "cat /root/note", str(image)],
                                  capture_output=True, text=True, check=True).stdout
            extra = subprocess.run(["debugfs", "-R", "cat /root/extra", str(image)],
                                   capture_output=True, text=True, check=True).stdout
            assert note == "second version\n" and extra == "here\n", (note, extra)
            check = subprocess.run(["e2fsck", "-f", "-n", str(image)], capture_output=True, text=True)
            assert check.returncode == 0, check.stdout[-500:] + check.stderr[-500:]
            image.unlink()
            return {"bytes": length, "note": note.strip()}
        case("a downloaded disk is a clean filesystem holding the files as they were", downloaded)

        def size_restored():
            power("stop", "stopped")
            status, machine = request("PATCH", f"/machines/{NAME}", {"diskGiB": 2})
            assert status == 200 and machine["diskGiB"] == 2, (status, machine)
            machine = restore(state["first"])
            assert machine["diskGiB"] == 1, machine
            power("start", "running")
            root_kb = int(guest("df -k / | tail -1").split()[1])
            assert root_kb < 1024 * 1024, root_kb
            assert files() == "first version\nno-extra\n"
            return {"root_kb": root_kb}
        case("a snapshot taken before a resize restores the smaller disk", size_restored)

        def refusals_and_deletes():
            results = {}
            status, value = request("POST", f"/machines/{NAME}/snapshots/{state['first']}/restore", {})
            assert status == 409, (status, value)
            results["restoring onto a running machine"] = value["message"]
            power("stop", "stopped")
            for label, method, path, body, want in [
                ("no such snapshot", "GET", "/snapshots/snap-000000000000000", None, 404),
                ("not a snapshot ID", "GET", "/snapshots/..%2F..%2Fmachine.json", None, 404),
                ("restoring no such snapshot", "POST", "/snapshots/snap-000000000000000/restore", {}, 404),
                ("a name that is not one", "POST", "/snapshots", {"name": " "}, 400),
                ("a field that is not one", "POST", "/snapshots", {"label": "x"}, 422),
            ]:
                status, value = request(method, f"/machines/{NAME}{path}", body)
                assert status == want, (label, status, value)
                results[label] = value["message"] if isinstance(value, dict) else value[:160]
            status, value = request("DELETE", f"/machines/{NAME}/snapshots/{state['second']}")
            assert status == 204, (status, value)
            status, listed = request("GET", f"/machines/{NAME}/snapshots")
            assert [s["snapshotID"] for s in listed] == [state["first"]], listed
            status, value = request("DELETE", f"/machines/{NAME}")
            assert status == 204, (status, value)
            left = list((work / "machines").iterdir()) if (work / "machines").exists() else []
            assert left == [], left
            return results
        case("what cannot be done is refused; deletes remove snapshots and the machine its own", refusals_and_deletes)

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
