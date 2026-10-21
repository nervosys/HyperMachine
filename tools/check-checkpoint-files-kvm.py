#!/usr/bin/env python3
"""Files read out of a sandbox's checkpoint from outside, on real KVM.

Runs an owned hv2-sandboxd, with a template of one vCPU and 512 MiB, and
checks through its API:

1. A directory in a checkpoint lists each entry's type, mode, owner and
   size, a name with a space and a newline in it included.
2. A file read from a checkpoint is as it was when the checkpoint was
   taken, byte for byte, though the sandbox has changed or deleted it
   since.
3. The sandbox is untouched: the same sandbox, its files as it left them,
   its own program still running.
4. A paused sandbox's checkpoint is read, and the sandbox stays paused.
5. Readers at once: three reads together all answer.
6. What cannot be read is refused, each with why: no such checkpoint,
   sandbox or path; a file listed or a directory read; a symlink read; a
   relative path; a file over 64 MiB.
7. A directory downloaded whole as a gzipped tar holds every entry as it
   was taken, with its modes, owners, symlink and odd name, and none of
   what the sandbox changed or added since; a subdirectory is downloaded
   alone; `/` downloads the whole filesystem without `/proc`, `/sys`,
   `/dev` and `/run`; a file, a missing path and a relative path are
   refused.

Writes report.json and the daemon's log into --output.
"""
import argparse, concurrent.futures, hashlib, io, json, os, shutil, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path


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
              "purpose": "functional verification of reading files out of a sandbox's checkpoint on KVM; the "
                         "times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-checkpoint-files-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300, raw=False):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            data = response.read()
            if raw and response.status == 200:
                return response.status, data
            try:
                return response.status, json.loads(data) if data else None
            except ValueError:
                return response.status, data.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(sandbox, cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200, (status, value)
        assert value.get("exit_code", 0) == 0, value
        return value["stdout"]

    def listing(sandbox, name, path, want=200):
        began = time.monotonic()
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/files?"
                                + urllib.parse.urlencode({"path": path}))
        assert status == want, (path, status, value)
        return value, round(time.monotonic() - began, 3)

    def read(sandbox, name, path, want=200):
        began = time.monotonic()
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/file?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (path, status, value)
        return value, round(time.monotonic() - began, 3)

    def archive(sandbox, name, path, want=200):
        began = time.monotonic()
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/archive?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (path, status, value)
        return value, round(time.monotonic() - began, 3)

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {}

        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        sandbox = made["sandboxID"]
        sh(sandbox, "mkdir -p /root/notes/sub && cd /root/notes && echo hello > a.txt && chmod 600 a.txt && "
                    "printf 'odd' > \"$(printf 'a b\\nc')\" && ln -s a.txt link && "
                    "head -c 1048576 /dev/urandom > big.bin && chown 1000:1000 big.bin && "
                    "sha256sum big.bin | cut -d' ' -f1 > /root/big.sum")
        state["big_sum"] = sh(sandbox, "cat /root/big.sum").strip()
        # A program of its own, writing as it runs.
        sh(sandbox, "setsid sh -c 'while :; do date +%s >> /root/ticker; sleep 1; done' >/dev/null 2>&1 &")
        time.sleep(1.5)
        status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
        assert status in (200, 201, 204), (status, value)
        # Changed since.
        sh(sandbox, "echo changed > /root/notes/a.txt && rm /root/notes/big.bin && touch /root/notes/after")

        def listed():
            value, took = listing(sandbox, "one", "/root/notes")
            by = {e["name"]: e for e in value["entries"]}
            assert set(by) == {"a.txt", "a b\nc", "link", "big.bin", "sub"}, sorted(by)
            assert by["a.txt"] == {"name": "a.txt", "type": "file", "mode": "600", "uid": 0, "gid": 0,
                                   "size": 6}, by["a.txt"]
            assert by["sub"]["type"] == "directory" and by["sub"]["size"] is None
            assert by["link"]["type"] == "symlink"
            assert by["big.bin"]["size"] == 1048576 and by["big.bin"]["uid"] == 1000
            assert by["a b\nc"]["size"] == 3
            return {"seconds": took, "entries": value["entries"]}
        case("a directory in a checkpoint lists each entry's type, mode, owner and size", listed)

        def as_taken():
            text, took = read(sandbox, "one", "/root/notes/a.txt")
            assert text == b"hello\n", text
            big, took_big = read(sandbox, "one", "/root/notes/big.bin")
            assert len(big) == 1048576 and hashlib.sha256(big).hexdigest() == state["big_sum"]
            odd, _ = read(sandbox, "one", "/root/notes/a b\nc")
            assert odd == b"odd", odd
            return {"seconds": took, "megabyte_seconds": took_big}
        case("a file read from a checkpoint is as it was taken, though the sandbox changed it", as_taken)

        def downloaded():
            data, took = archive(sandbox, "one", "/root/notes")
            tar = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
            members = {m.name.removeprefix("./"): m for m in tar.getmembers()}
            members.pop(".", None)
            assert set(members) == {"a.txt", "a b\nc", "link", "big.bin", "sub"}, sorted(members)
            assert tar.extractfile(members["a.txt"]).read() == b"hello\n"
            assert members["a.txt"].mode & 0o777 == 0o600, oct(members["a.txt"].mode)
            big = tar.extractfile(members["big.bin"]).read()
            assert hashlib.sha256(big).hexdigest() == state["big_sum"] and members["big.bin"].uid == 1000
            assert tar.extractfile(members["a b\nc"]).read() == b"odd"
            assert members["link"].issym() and members["link"].linkname == "a.txt"
            assert members["sub"].isdir()
            # Only the subtree asked for, with nothing in it.
            sub, _ = archive(sandbox, "one", "/root/notes/sub")
            inner = [m.name for m in tarfile.open(fileobj=io.BytesIO(sub), mode="r:gz").getmembers()]
            assert [n.removeprefix("./") for n in inner] in ([], ["."], [""]), inner
            # The whole filesystem: the notes are in it, as taken, and what
            # the kernel makes up is not.
            whole, took_whole = archive(sandbox, "one", "/")
            names = [m.name.removeprefix("./") for m in tarfile.open(fileobj=io.BytesIO(whole), mode="r:gz").getmembers()]
            assert "root/notes/a.txt" in names and "root/notes/big.bin" in names, names[:20]
            assert not [n for n in names if n.split("/")[0] in ("proc", "sys", "dev", "run")], names[:20]
            refused = {}
            for label, path, want in [("a file", "/root/notes/a.txt", 400),
                                      ("no such path", "/root/nothing", 404),
                                      ("a relative path", "root/notes", 400)]:
                value, _ = archive(sandbox, "one", path, want)
                refused[label] = value["message"]
            return {"seconds": took, "archive_bytes": len(data), "entries": sorted(members),
                    "whole_filesystem": {"seconds": took_whole, "archive_bytes": len(whole), "entries": len(names)},
                    "refused": refused}
        case("a directory is downloaded whole as a tar, as it was taken", downloaded)

        def untouched():
            assert sh(sandbox, "cat /root/notes/a.txt") == "changed\n"
            assert sh(sandbox, "[ -e /root/notes/after ] && [ ! -e /root/notes/big.bin ] && echo yes") == "yes\n"
            before = int(sh(sandbox, "wc -l < /root/ticker"))
            time.sleep(2.5)
            after = int(sh(sandbox, "wc -l < /root/ticker"))
            assert after > before, (before, after)
            listed = [s["sandboxID"] for s in request("GET", "/sandboxes")[1]]
            assert listed.count(sandbox) == 1, listed
            # What the reader's copy of the program wrote: the checkpoint's
            # lines, give or take one written before it was stopped.
            first, _ = read(sandbox, "one", "/root/ticker")
            time.sleep(2.5)
            second, _ = read(sandbox, "one", "/root/ticker")
            return {"ticker_lines": {"sandbox_before": before, "sandbox_after": after,
                                     "checkpoint_read_1": first.count(b"\n"),
                                     "checkpoint_read_2": second.count(b"\n")}}
        case("the sandbox is untouched: its files as it left them, its program running", untouched)

        def paused():
            status, value = request("POST", f"/sandboxes/{sandbox}/pause", {})
            assert status in (200, 204), (status, value)
            text, took = read(sandbox, "one", "/root/notes/a.txt")
            assert text == b"hello\n", text
            # Still paused: a resume of a running sandbox is refused (409).
            status, value = request("POST", f"/sandboxes/{sandbox}/resume", {})
            assert status == 201, (status, value)
            assert sh(sandbox, "cat /root/notes/a.txt") == "changed\n"
            return {"seconds": took}
        case("a paused sandbox's checkpoint is read, and it stays paused", paused)

        def together():
            began = time.monotonic()
            with concurrent.futures.ThreadPoolExecutor(3) as pool:
                got = list(pool.map(lambda _: read(sandbox, "one", "/root/notes/a.txt")[0], range(3)))
            assert got == [b"hello\n"] * 3, got
            return {"seconds": round(time.monotonic() - began, 3)}
        case("three reads at once all answer", together)

        def refusals():
            refused = {}
            for label, call, want in [
                ("no such checkpoint", lambda w: read(sandbox, "absent", "/root/notes/a.txt", w), 404),
                ("no such sandbox", lambda w: read("sbx-absent", "one", "/root/notes/a.txt", w), 404),
                ("no such path", lambda w: read(sandbox, "one", "/root/nothing", w), 404),
                ("a file listed", lambda w: listing(sandbox, "one", "/root/notes/a.txt", w), 400),
                ("a directory read", lambda w: read(sandbox, "one", "/root/notes", w), 400),
                ("a symlink read", lambda w: read(sandbox, "one", "/root/notes/link", w), 400),
                ("a relative path", lambda w: read(sandbox, "one", "root/notes/a.txt", w), 400),
            ]:
                value, _ = call(want)
                refused[label] = value["message"]
            sh(sandbox, "head -c 68157440 /dev/zero > /root/huge")
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "huge"})
            assert status in (200, 201, 204), (status, value)
            value, _ = read(sandbox, "huge", "/root/huge", 413)
            refused["over 64 MiB"] = value["message"]
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"refused": refused}
        case("what cannot be read is refused, each with why", refusals)

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
