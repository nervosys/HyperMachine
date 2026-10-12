#!/usr/bin/env python3
"""Files on real KVM: bytes put into a machine's guest and taken out of it.

Runs an owned hv2-sandboxd with a --machine-dir and one machine from the base
template, then checks, through its API:

1. A file goes in and comes out the same: text, every byte value, nothing at
   all, and one larger than a piece, whose digest the guest computes itself.
2. The directories above a file are made, and with a user the file and
   those directories are that user's.
3. A file written over a longer one is exactly the new one.
4. A file the guest made comes out, by a path with spaces and more in it.
5. What cannot be done is refused, each with a status that says whose it is.
6. Nothing goes in or out of a stopped machine.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, http.client, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

NAME = "files-01"
MAX_FILE = 64 << 20


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
              "purpose": "functional verification of machine file routes on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-files-check-", dir="/var/tmp"))
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

    def raw(method, path, data=None, timeout=120, json_body=False):
        headers = {"content-type": "application/json"} if json_body else {}
        req = urllib.request.Request(base + path, method=method, headers=headers, data=data)
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, response.read(), response.headers.get("content-type")

    def request(method, path, body=None, timeout=120):
        status, data, _ = raw(method, path, None if body is None else json.dumps(body).encode(), timeout, True)
        try:
            return status, json.loads(data) if data else None
        except ValueError:
            return status, data.decode(errors="replace")

    def where(path, machine=NAME, **more):
        return f"/machines/{machine}/files?" + urllib.parse.urlencode(dict(path=path, **more))

    def put(path, data, **more):
        status, answer, _ = raw("PUT", where(path, **more), data)
        assert status == 204, (path, status, answer[:300])

    def get(path):
        status, data, kind = raw("GET", where(path))
        assert status == 200, (path, status, data[:300])
        assert kind == "application/octet-stream", kind
        return data

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, (cmd, status, value)
        return value["stdout"]

    def refused(method, path, want, data=None):
        status, answer, _ = raw(method, path, data)
        assert status == want, (method, path, status, answer[:300])
        try:
            return json.loads(answer)["message"]
        except ValueError:
            return answer.decode(errors="replace")[:120]

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
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
        assert status == 201 and created["state"] == "running", (status, created)

        def round_trips():
            # Larger than one 4 MiB piece of the guest agent's, and not a
            # multiple of it; not compressible, so nothing can hide a gap.
            large = os.urandom(9 * 1024 * 1024 + 123)
            sizes = {}
            for label, data in [("text", "one line\nand another, with ü\n".encode()),
                                ("every byte value", bytes(range(256)) * 3),
                                ("empty", b""),
                                ("larger than a piece", large)]:
                path = "/tmp/files/" + label.replace(" ", "-")
                put(path, data)
                assert get(path) == data, label
                # The guest's own account of what it holds.
                assert int(guest(f"wc -c < {path}")) == len(data), label
                assert guest(f"sha256sum {path}").split()[0] == hashlib.sha256(data).hexdigest(), label
                sizes[label] = len(data)
            return sizes
        case("a file goes in and comes out the same, and the guest's digest of it agrees", round_trips)

        def owned():
            guest("grep -q '^dev:' /etc/passwd 2>/dev/null || echo 'dev:x:1000:1000:dev:/tmp:/bin/sh' >> /etc/passwd")
            put("/srv/made/for/dev/note.txt", b"for dev\n", user="dev")
            owners = guest("stat -c '%u:%g %n' /srv/made /srv/made/for /srv/made/for/dev /srv/made/for/dev/note.txt")
            assert [line.split()[0] for line in owners.splitlines()] == ["1000:1000"] * 4, owners
            # And dev can write beside it, which is what owning the directory is for.
            status, value = request("POST", f"/machines/{NAME}/runs", {
                "cmd": "echo mine > /srv/made/for/dev/second && cat /srv/made/for/dev/note.txt", "user": "dev"})
            assert status == 201, (status, value)
            deadline = time.monotonic() + 20
            while True:
                status, run = request("GET", f"/machines/{NAME}/runs/{value['runID']}?stdout=0&stderr=0")
                if run["state"] != "running":
                    break
                assert time.monotonic() < deadline, run
                time.sleep(0.2)
            assert run["exitCode"] == 0 and run["stdout"]["text"] == "for dev\n", run
            # Without a user: root's.
            put("/srv/roots/note.txt", b"for root\n")
            assert guest("stat -c '%u' /srv/roots /srv/roots/note.txt").split() == ["0", "0"]
            return {"owners": owners.splitlines()}
        case("directories are made, and with a user they and the file are that user's", owned)

        def overwritten():
            put("/tmp/files/over", b"a long first version of the file\n" * 100)
            put("/tmp/files/over", b"short\n")
            assert get("/tmp/files/over") == b"short\n"
            return None
        case("a file written over a longer one is exactly the new one", overwritten)

        def from_the_guest():
            name = "/tmp/files/made in the guest, with 'quotes' & ü.txt"
            guest("printf 'made here\\n' > \"/tmp/files/made in the guest, with 'quotes' & ü.txt\"")
            assert get(name) == b"made here\n"
            put(name + ".copy", b"and back\n")
            assert guest("cat \"/tmp/files/made in the guest, with 'quotes' & ü.txt.copy\"") == "and back\n"
            return {"path": name}
        case("a file the guest made comes out, by a path with spaces and more in it", from_the_guest)

        def refusals():
            guest("dd if=/dev/zero of=/tmp/files/too-big bs=1M count=0 seek=65 2>/dev/null")
            results = {}
            for label, method, path, want, data in [
                ("a relative path", "GET", where("etc/hosts"), 400, None),
                ("a relative path, written", "PUT", where("x"), 400, b"x"),
                ("a directory's name", "PUT", where("/tmp/files/"), 400, b"x"),
                ("no path", "GET", f"/machines/{NAME}/files", 400, None),
                ("a file that is not there", "GET", where("/tmp/files/absent"), 404, None),
                ("reading a directory", "GET", where("/tmp/files"), 400, None),
                ("writing under a file", "PUT", where("/tmp/files/text/below"), 400, b"x"),
                ("a user for a read", "GET", where("/tmp/files/text", user="dev"), 400, None),
                ("a user the guest has not", "PUT", where("/tmp/files/x", user="nobody-here"), 400, b"x"),
                ("a file out that is too large", "GET", where("/tmp/files/too-big"), 413, None),
                ("no such machine", "GET", where("/etc/hosts", machine="absent-99"), 404, None),
            ]:
                results[label] = refused(method, path, want, data)
            # One byte more than there may be. The node may answer before it
            # has been sent it all, so the sending is allowed to be cut off.
            conn = http.client.HTTPConnection("127.0.0.1", api, timeout=120)
            conn.putrequest("PUT", where("/tmp/files/huge"))
            conn.putheader("content-length", str(MAX_FILE + 1))
            conn.endheaders()
            try:
                conn.send(bytes(MAX_FILE + 1))
            except OSError:
                pass
            response = conn.getresponse()
            assert response.status == 413, response.status
            results["a file in that is too large"] = response.read().decode(errors="replace")[:120]
            conn.close()
            # Refused before any of it was written.
            assert "huge" not in guest("ls /tmp/files"), "part of a refused file was written"
            return results
        case("what cannot be done is refused, each with a status that says whose it is", refusals)

        def stopped():
            status, value = request("POST", f"/machines/{NAME}/stop", timeout=120)
            assert status == 200 and value["state"] == "stopped", (status, value)
            return {"read": refused("GET", where("/tmp/files/text"), 409),
                    "write": refused("PUT", where("/tmp/files/text"), 409, b"x")}
        case("nothing goes in or out of a stopped machine", stopped)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
