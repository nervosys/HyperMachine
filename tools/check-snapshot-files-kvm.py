#!/usr/bin/env python3
"""Reading files out of a machine's disk snapshot, without booting it.

Runs an owned hv2-sandboxd and checks, through its API:

1. A template machine whose guest wrote files, one with a name holding a
   space, quotes and `$(...)`: its snapshot's directories are listed with
   names, kinds, owners and sizes, and its files read back byte for byte.
2. A machine made from a distribution's cloud image as published (Ubuntu
   24.04, GPT, root on partition 1, /boot an ext4 partition of its own),
   created stopped and never booted: its snapshot's root partition is found
   without being named, /usr/lib/os-release is read from it, a symbolic link
   is listed as one and not followed, and /boot's partition is read when
   named.
3. What cannot be read is refused: no such path, a directory read as a file,
   a file listed as a directory, `..`, a partition that is not ext, a
   partition that is not there.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

ODD = "a file \"q\" $(touch pwned) 'x'"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "firmware", "image", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of reading files from machine disk snapshots; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd", "firmware"]},
              "image": args.image.name,
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-snapshot-files-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    os.symlink(args.image, images / args.image.name)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def raw(method, path, body=None, timeout=600):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, response.read()

    def request(method, path, body=None, timeout=600):
        status, data = raw(method, path, body, timeout)
        try:
            return status, json.loads(data) if data else None
        except ValueError:
            return status, data.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def files(machine, snap, path, partition=None):
        query = {"path": path}
        if partition:
            query["partition"] = partition
        return request("GET", f"/machines/{machine}/snapshots/{snap}/files?" + urllib.parse.urlencode(query))

    def read(machine, snap, path, partition=None):
        query = {"path": path}
        if partition:
            query["partition"] = partition
        return raw("GET", f"/machines/{machine}/snapshots/{snap}/file?" + urllib.parse.urlencode(query))

    def snapshot(machine):
        status, snap = request("POST", f"/machines/{machine}/snapshots", {"name": "for reading"})
        assert status == 201, (status, snap)
        return snap["snapshotID"]

    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {}

        def template_machine():
            status, created = request("POST", "/machines", {"name": "tpl-01", "diskGiB": 1, "memoryMB": 512})
            assert status == 201, (status, created)
            payload = bytes(range(256)) * 40
            req = urllib.request.Request(base + "/machines/tpl-01/files?" + urllib.parse.urlencode(
                {"path": f"/srv/data/{ODD}"}), method="PUT", data=payload)
            assert urllib.request.urlopen(req, timeout=60).status == 204
            status, value = request("POST", "/machines/tpl-01/exec", {
                "cmd": "echo hello > /srv/data/note && mkdir -p /srv/data/sub && ln -s note /srv/data/link && "
                       "chown 1000:1000 /srv/data/note && sync", "timeout_secs": 30})
            assert status == 200 and value["exit_code"] == 0, value
            status, stopped = request("POST", "/machines/tpl-01/stop")
            assert status == 200, (status, stopped)
            snap = snapshot("tpl-01")
            state["tpl"] = snap
            status, listed = files("tpl-01", snap, "/srv/data")
            assert status == 200, (status, listed)
            by_name = {e["name"]: e for e in listed["entries"]}
            assert set(by_name) == {ODD, "note", "sub", "link"}, sorted(by_name)
            assert by_name["note"]["type"] == "file" and by_name["note"]["size"] == 6, by_name["note"]
            assert (by_name["note"]["uid"], by_name["note"]["gid"]) == (1000, 1000), by_name["note"]
            assert by_name["sub"]["type"] == "directory" and by_name["sub"]["size"] is None
            assert by_name["link"]["type"] == "symlink"
            assert listed["partition"] is None
            status, body = read("tpl-01", snap, f"/srv/data/{ODD}")
            assert status == 200 and body == payload, (status, len(body))
            status, body = read("tpl-01", snap, "/srv/data/note")
            assert status == 200 and body == b"hello\n"
            # Nothing in the odd name was run, here or in the guest.
            assert not Path("pwned").exists()
            status, listed = files("tpl-01", snap, "/")
            assert "srv" in [e["name"] for e in listed["entries"]] and "." not in [e["name"] for e in listed["entries"]]
            return {"entries": sorted(by_name), "odd_file_bytes": len(payload)}
        case("a template machine's snapshot is listed and read, odd names included", template_machine)

        def cloud_image():
            status, created = request("POST", "/machines", {
                "name": "img-01", "image": args.image.name, "start": False, "guestAgent": False})
            assert status == 201 and created["state"] == "stopped", (status, created)
            snap = snapshot("img-01")
            state["img"] = snap
            status, listed = files("img-01", snap, "/usr/lib")
            assert status == 200, (status, listed)
            assert listed["partition"] == 1, listed["partition"]
            assert "os-release" in [e["name"] for e in listed["entries"]]
            status, body = read("img-01", snap, "/usr/lib/os-release")
            assert status == 200 and b'NAME="Ubuntu"' in body, (status, body[:200])
            pretty = [l for l in body.decode().splitlines() if l.startswith("PRETTY_NAME=")][0]
            status, etc = files("img-01", snap, "/etc")
            link = [e for e in etc["entries"] if e["name"] == "os-release"][0]
            assert link["type"] == "symlink", link
            status, value = read("img-01", snap, "/etc/os-release")
            assert status == 400 and "not followed" in json.loads(value)["message"], (status, value)
            # /boot is a partition of its own: its root has the kernels.
            boot = None
            for number in range(2, 20):
                status, value = files("img-01", snap, "/", partition=number)
                if status == 200 and any(e["name"].startswith("vmlinuz") for e in value["entries"]):
                    boot = number
                    break
            assert boot, "no partition holds the kernels"
            return {"root_partition": 1, "boot_partition": boot, "os": pretty}
        case("a cloud image never booted: its root is found and read, links are not followed, /boot by number",
             cloud_image)

        def refusals():
            results = {}
            for label, path, kind, partition, want in [
                ("no such path", "/no/such", "file", None, 404),
                ("a directory read as a file", "/srv", "file", None, 400),
                ("a file listed as a directory", "/srv/data/note", "files", None, 400),
                ("a path that climbs", "/srv/../etc", "files", None, 400),
                ("a relative path", "srv", "files", None, 400),
            ]:
                getter = read if kind == "file" else files
                status, value = getter("tpl-01", state["tpl"], path, partition)
                value = value if isinstance(value, dict) else json.loads(value)
                assert status == want, (label, status, value)
                results[label] = value["message"]
            for label, partition, want in [("a partition that is not ext", 14, 400),
                                           ("a partition that is not there", 99, 400)]:
                status, value = files("img-01", state["img"], "/", partition)
                assert status == want, (label, status, value)
                results[label] = value["message"]
            status, value = files("img-01", "snap-000000000000000", "/")
            assert status == 404, (status, value)
            return results
        case("what cannot be read is refused, each with why", refusals)

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
