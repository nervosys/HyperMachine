#!/usr/bin/env python3
"""A directory of a machine's disk snapshot, downloaded as a gzipped tar.

Runs an owned hv2-sandboxd on real KVM and checks, through its API:

1. A template machine whose guest wrote a tree (a file with an odd name,
   a file with an owner of its own, a subdirectory, a symbolic link) and was
   stopped: `GET /machines/{name}/snapshots/{id}/archive?path=/srv/data` is a
   gzipped tar of exactly that tree, every file byte for byte, the link a
   link, the owner kept where the node runs as root, and nothing from above.
2. The same for `/` (the whole filesystem), which holds the tree too.
3. What cannot be archived is refused, each with why: a file (read it with
   /file), no such path, `..`, a snapshot that does not exist.
4. The scratch the download used is gone.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, io, json, os, shutil, signal, socket, subprocess, tarfile, tempfile, time
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
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of archiving a directory of a machine disk snapshot; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "running_as_root": os.geteuid() == 0, "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-archive-check-", dir="/var/tmp"))
    scratch_tmp = work / "tmp"
    scratch_tmp.mkdir()
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info",
               TMPDIR=str(scratch_tmp))
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def raw(method, path, body=None, timeout=600):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, {k.lower(): v for k, v in response.headers.items()}, response.read()

    def request(method, path, body=None):
        status, _, data = raw(method, path, body)
        try:
            return status, json.loads(data) if data else None
        except ValueError:
            return status, data.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        detail = action()
        row["seconds"] = round(time.monotonic() - began, 3)
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def archive(machine, snap, path):
        return raw("GET", f"/machines/{machine}/snapshots/{snap}/archive?" + urllib.parse.urlencode({"path": path}))

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
            time.sleep(0.2)
        state = {}
        payload = bytes(range(256)) * 40

        def a_tree():
            status, created = request("POST", "/machines", {"name": "tpl-01", "diskGiB": 1, "memoryMB": 512})
            assert status == 201, (status, created)
            req = urllib.request.Request(base + "/machines/tpl-01/files?" + urllib.parse.urlencode(
                {"path": f"/srv/data/{ODD}"}), method="PUT", data=payload)
            assert urllib.request.urlopen(req, timeout=60).status == 204
            status, value = request("POST", "/machines/tpl-01/exec", {
                "cmd": "echo hello > /srv/data/note && mkdir -p /srv/data/sub && echo deep > /srv/data/sub/inner && "
                       "ln -s note /srv/data/link && chown 1000:1000 /srv/data/note && sync", "timeout_secs": 30})
            assert status == 200 and value["exit_code"] == 0, value
            status, stopped = request("POST", "/machines/tpl-01/stop")
            assert status == 200, (status, stopped)
            status, snap = request("POST", "/machines/tpl-01/snapshots", {"name": "for archive"})
            assert status == 201, (status, snap)
            state["snap"] = snap["snapshotID"]
            began = time.monotonic()
            status, headers, body = archive("tpl-01", state["snap"], "/srv/data")
            took = round(time.monotonic() - began, 3)
            assert status == 200, (status, body[:200])
            assert headers.get("content-type") == "application/gzip", headers
            assert ".tar.gz" in headers.get("content-disposition", ""), headers
            tar = tarfile.open(fileobj=io.BytesIO(body), mode="r:gz")
            members = {m.name.removeprefix("./"): m for m in tar.getmembers()}
            members.pop(".", None)
            assert set(members) == {ODD, "note", "sub", "sub/inner", "link"}, sorted(members)
            assert tar.extractfile(members[ODD]).read() == payload
            assert tar.extractfile(members["note"]).read() == b"hello\n"
            assert tar.extractfile(members["sub/inner"]).read() == b"deep\n"
            assert members["link"].issym() and members["link"].linkname == "note", members["link"]
            assert members["sub"].isdir()
            owner = (members["note"].uid, members["note"].gid)
            if os.geteuid() == 0:
                assert owner == (1000, 1000), owner
            assert not Path("pwned").exists()
            return {"archive_bytes": len(body), "seconds": took, "owner_of_note": list(owner)}
        case("a stopped template machine's tree is archived exactly", a_tree)

        def whole():
            status, headers, body = archive("tpl-01", state["snap"], "/")
            assert status == 200, (status, body[:200])
            names = {m.name.removeprefix("./") for m in tarfile.open(fileobj=io.BytesIO(body), mode="r:gz")}
            assert {"srv/data/note", "srv/data/sub/inner", "srv/data/link"} <= names, sorted(names)[:30]
            return {"archive_bytes": len(body), "entries": len(names)}
        case("the whole filesystem is archived and holds the tree", whole)

        def refusals():
            refused = {}
            for label, snap, path, want in [
                ("a file", state["snap"], "/srv/data/note", 400),
                ("no such path", state["snap"], "/srv/nothing", 404),
                ("dot-dot", state["snap"], "/srv/../etc", 400),
                ("no such snapshot", "snap-ffffffff", "/srv", 404),
            ]:
                status, _, body = archive("tpl-01", snap, path)
                assert status == want, (label, status, body[:200])
                refused[label] = json.loads(body).get("message", "")[:140] if body else ""
            return {"refused": refused}
        case("what cannot be archived is refused, each with why", refusals)

        def no_scratch():
            left = sorted(p.name for p in scratch_tmp.iterdir() if p.name.startswith(".tmp"))
            assert left == [], left
        case("the scratch is gone", no_scratch)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
