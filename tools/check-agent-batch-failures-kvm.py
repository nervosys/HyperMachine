#!/usr/bin/env python3
"""Failure paths through the guest agent's `batch` operation, on real KVM.

Runs an owned hv2-sandboxd (`--guest-kit` an empty directory) whose base template is the image under
test, and checks through the public API what the unit tests only simulate:

a. (images with a shell) A checkpoint archive of a tree the walk cannot hold completely: 300 small
   files, a FIFO, a character device, a unix socket if busybox's `nc -lU` makes one, a symlink whose
   target is not UTF-8 (among the small files, so the walk's batch prefetch reads it), a mode-000
   file (root still reads it), a good symlink. The `x-hv2-archive-skipped` header must equal the number
   of FIFOs/sockets/devices the guest holds plus one, and the tar must hold every other entry with the
   right bytes, modes and link targets. The daemon log must show the archive took the agent path and
   that no batch fell back to one operation per connection. Also an archive of /proc/1 and /proc/1/task (files the guest lists but
   cannot read, e.g. write-only clear_refs) must answer 200 with x-hv2-archive-skipped >= 1.
b. A COPY upload whose tar holds a member written through an earlier symlink (`esc -> /etc`, then
   `esc/passwd`): the build must fail, naming the symlink member, and (control) the same tar without
   the member under the link must build. Whether a *repository* could do the same is shown by what
   Git itself says when asked to put a path under a symlink into a tree.
c. A COPY upload holding a hard link: with a shell, the build succeeds through the shell path (log
   "falling back to shell"), both names hold the bytes and share an inode (link count 2); with no
   shell, the build fails saying the COPY needs the shell.

Writes report.json and the daemon's log into --output, with the number of `Kvm` and `Tcg` lines.
"""
import argparse, hashlib, io, json, os, re, shutil, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tar_of(members):
    """members: (name, kind, mode, payload), kind file|dir|sym|hard."""
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w") as tar:
        for name, kind, mode, payload in members:
            info = tarfile.TarInfo(name)
            info.mode = mode
            if kind == "dir":
                info.type = tarfile.DIRTYPE
                tar.addfile(info)
            elif kind in ("sym", "hard"):
                info.type = tarfile.SYMTYPE if kind == "sym" else tarfile.LNKTYPE
                info.linkname = payload
                tar.addfile(info)
            else:
                info.size = len(payload)
                tar.addfile(info, io.BytesIO(payload))
    return out.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--shell", choices=["yes", "no"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    has_shell = args.shell == "yes"
    report = {"success": False, "label": args.label, "shell": has_shell,
              "purpose": "failure paths through a real guest-agent batch (archive skips, COPY refusals), on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-batch-failures-", dir="/var/tmp"))
    kit = work / "kit"
    kit.mkdir()
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(kit),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                 RUST_LOG="info,hv2_sandboxd::builds=debug,hv2_sandboxd::checkpoint_files=debug,"
                          "hv2_sandboxd::env_vars=debug"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300, raw=False, url=None, data=None):
        payload = data if data is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(url or base + path, method=method,
                                     headers={"content-type": "application/json"}, data=payload)
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw_data = response.read()
            headers = dict(response.headers)
            if raw and response.status == 200:
                return response.status, raw_data
            try:
                value = json.loads(raw_data) if raw_data else None
            except ValueError:
                value = raw_data.decode(errors="replace")
            if raw:
                return response.status, value
            return response.status, value

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    builds = {}

    def build(name, steps, uploads):
        status, made = request("POST", "/v3/templates", {"name": name})
        assert status == 202, (status, made)
        hashes = {}
        for label, payload in uploads.items():
            h = hashlib.sha256(payload).hexdigest()
            hashes[label] = h
            status, link = request("GET", f"/templates/{name}/files/{h}")
            assert status == 201, (status, link)
            if not link["present"]:
                with urllib.request.urlopen(urllib.request.Request(link["url"], method="PUT", data=payload),
                                            timeout=60) as r:
                    assert r.status == 200, r.status
        wired = []
        for step in steps:
            step = dict(step)
            if "files" in step:
                step["filesHash"] = hashes[step.pop("files")]
            wired.append(step)
        status, value = request("POST", f"/v2/templates/{name}/builds/{made['buildID']}",
                                {"fromTemplate": "base", "steps": wired})
        assert status == 202, (status, value)
        deadline = time.monotonic() + 600
        while True:
            status, value = request("GET", f"/templates/{name}/builds/{made['buildID']}/status")
            assert status == 200, (status, value)
            if value["status"] in ("ready", "error"):
                builds[name] = value
                return value["status"], value.get("reason") or {}
            assert time.monotonic() < deadline, ("build", name, value["status"])
            time.sleep(0.5)

    def sandbox_of(template):
        status, made = request("POST", "/sandboxes", {"templateID": template, "timeout": 900})
        assert status in (200, 201), (status, made)
        return made["sandboxID"]

    def sh(sandbox, script, timeout=120):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": script, "timeout_secs": timeout})
        assert status == 200, (status, value)
        return value

    def query(sandbox, name, route, path, want=200):
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/{route}?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (route, path, status, value)
        return value

    def archive(sandbox, name, path):
        req = urllib.request.Request(base + f"/sandboxes/{sandbox}/checkpoints/{name}/archive?"
                                     + urllib.parse.urlencode({"path": path}))
        try:
            response = urllib.request.urlopen(req, timeout=300)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, dict(response.headers), response.read()

    def log_text():
        log.flush()
        return log_path.read_text(errors="replace")

    try:
        deadline = time.monotonic() + 240
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

        if has_shell:
            def archive_skips():
                sandbox = sandbox_of("base")
                script = (
                    "set -e; mkdir -p /data/small /data/sub; i=0; "
                    "while [ $i -lt 300 ]; do echo \"small $i\" > /data/small/f$(printf %03d $i); i=$((i+1)); done; "
                    "ln -s \"$(printf '\\377\\376')\" /data/small/f150-bad; "
                    "ln -s f007 /data/small/f007-link; ln -s small /data/dirlink; "
                    "echo secret > /data/sub/mode000; chmod 000 /data/sub/mode000; "
                    "echo exec > /data/sub/run.sh; chmod 755 /data/sub/run.sh; "
                    "mkfifo /data/fifo; mknod /data/zero c 1 5; "
                    "(setsid nc -lU /data/sock >/dev/null 2>&1 &) ; sleep 1; sync")
                done = sh(sandbox, script)
                assert done["exit_code"] == 0, done
                held = sh(sandbox, "find /data \\( -type p -o -type s -o -type b -o -type c \\) | sort; "
                                   "echo ---; ls -l /data/small/f150-bad | cut -c1-12")["stdout"]
                others = [l for l in held.split("---")[0].split() if l]
                status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "p"})
                assert status in (200, 201, 204), (status, value)
                before = log_text()
                status, headers, data = archive(sandbox, "p", "/data")
                assert status == 200, (status, data[:300])
                after = log_text()[len(before):]
                skipped = headers.get("x-hv2-archive-skipped")
                want = len(others) + 1  # plus the non-UTF-8 link
                assert skipped is not None and int(skipped) == want, (skipped, want, others)
                tar = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
                members = {m.name.removeprefix("./"): m for m in tar.getmembers()}
                names = set(members)
                expect = {"small", "sub"} | {f"small/f{i:03d}" for i in range(300)} | {
                    "small/f007-link", "sub/mode000", "sub/run.sh", "dirlink"}
                assert expect <= names, sorted(expect - names)
                extra = names - expect - {"."}
                assert not extra, sorted(extra)
                assert "fifo" not in names and "zero" not in names and "sock" not in names
                assert "small/f150-bad" not in names
                for i in range(300):
                    got = tar.extractfile(members[f"small/f{i:03d}"]).read()
                    assert got == f"small {i}\n".encode(), (i, got)
                assert members["small/f007-link"].issym() and members["small/f007-link"].linkname == "f007"
                assert members["dirlink"].issym() and members["dirlink"].linkname == "small"
                assert members["sub/mode000"].mode & 0o7777 == 0, oct(members["sub/mode000"].mode)
                assert tar.extractfile(members["sub/mode000"]).read() == b"secret\n"
                assert members["sub/run.sh"].mode & 0o7777 == 0o755
                lines = [l for l in after.splitlines()]
                agent = [l for l in lines if "checkpoint reader: using agent op (archive)" in l]
                shell = [l for l in lines if "falling back to shell (archive)" in l]
                nobatch = [l for l in lines if "no batch operation" in l]
                assert len(agent) == 1 and not shell and not nobatch, (len(agent), len(shell), len(nobatch))
                detail = {"header_skipped": int(skipped), "expected_skipped": want,
                          "special_files_in_guest": others, "members": len(members),
                          "bad_link_listing": held.split("---")[1].strip(),
                          "log": {"archive agent path": len(agent), "shell": len(shell), "no batch": len(nobatch)}}
                # The nearest thing the public API has to a path that vanishes mid-walk: /proc/1.
                probe = {}
                for path in ["/proc/1", "/proc/1/task", "/sys/kernel"]:
                    status, headers, body = archive(sandbox, "p", path)
                    entry = {"status": status}
                    if status == 200:
                        entry["skipped_header"] = headers.get("x-hv2-archive-skipped")
                        entry["members"] = len(tarfile.open(fileobj=io.BytesIO(body), mode="r:gz").getmembers())
                    else:
                        entry["body"] = body[:300].decode(errors="replace")
                    probe[path] = entry
                detail["proc_probe"] = probe
                # A file the guest lists but cannot read (/proc/1/clear_refs: write-only, EINVAL on read)
                # is counted in the header and left out, as the shell's `tar` went on past it; it must
                # not fail the archive with 502.
                for path in ("/proc/1", "/proc/1/task"):
                    assert probe[path]["status"] == 200, (path, probe[path])
                    assert int(probe[path]["skipped_header"]) >= 1, (path, probe[path])
                    assert probe[path]["members"] > 1, (path, probe[path])
                request("DELETE", f"/sandboxes/{sandbox}")
                return detail
            case("a. archive of a tree with a FIFO, device, socket, non-UTF-8 link, mode-000 file", archive_skips)

        # b: a member written through an earlier symlink.
        def escape():
            hostile = tar_of([("tree", "dir", 0o755, None),
                              ("tree/esc", "sym", 0o777, "/etc"),
                              ("tree/esc/passwd", "file", 0o644, b"pwned\n")])
            relative = tar_of([("tree", "dir", 0o755, None),
                               ("tree/sub", "dir", 0o755, None),
                               ("tree/esc", "sym", 0o777, "sub"),
                               ("tree/esc/file.txt", "file", 0o644, b"x\n")])
            control = tar_of([("tree", "dir", 0o755, None),
                              ("tree/esc", "sym", 0o777, "/etc"),
                              ("tree/ok.txt", "file", 0o644, b"ok\n")])
            detail = {}
            for name, upload, must_fail in [("esc-abs", hostile, True), ("esc-rel", relative, True),
                                            ("esc-control", control, False)]:
                status, reason = build(name, [{"type": "COPY", "args": ["tree", "/opt/tree"], "files": "tree"}],
                                       {"tree": upload})
                message = reason.get("message", "")
                if must_fail:
                    assert status == "error" and "through a symlink it made earlier" in message, (status, reason)
                    assert "esc" in message, message
                    detail[name] = {"status": status, "message": message}
                else:
                    assert status == "ready", (status, reason)
                    sandbox = sandbox_of(name)
                    status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
                    assert status in (200, 201, 204), (status, value)
                    entries = {e["name"]: e for e in json.loads(query(sandbox, "one", "files", "/opt/tree"))["entries"]}
                    assert entries["esc"]["type"] == "symlink" and entries["ok.txt"]["type"] == "file", entries
                    # Nothing landed in /etc through the link.
                    etc = {e["name"] for e in json.loads(query(sandbox, "one", "files", "/etc"))["entries"]}
                    assert "ok.txt" not in etc, sorted(etc)
                    detail[name] = {"status": status if isinstance(status, str) else "ready",
                                    "entries": sorted(entries)}
                    request("DELETE", f"/sandboxes/{sandbox}")
            # A repository cannot hold the same pair: Git's own refusals.
            env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
            repo = work / "gitcheck"
            repo.mkdir()

            def git(*argv):
                done = subprocess.run(["git", "-c", "user.name=c", "-c", "user.email=c@example.invalid", *argv],
                                      cwd=repo, env=env, capture_output=True, text=True)
                return {"argv": list(argv), "code": done.returncode, "out": (done.stdout + done.stderr).strip()}
            git("init", "-q", "-b", "main")
            os.symlink("/etc", repo / "esc")
            git("add", "esc")
            beyond = git("add", "esc/passwd")
            blob = subprocess.run(["git", "hash-object", "-w", "--stdin"], cwd=repo, env=env, input="x\n",
                                  capture_output=True, text=True).stdout.strip()
            plain = git("update-index", "--add", "--cacheinfo", f"100644,{blob},esc/passwd")
            replaced = git("update-index", "--add", "--replace", "--cacheinfo", f"100644,{blob},esc/passwd")
            listing = git("ls-files", "-s")
            assert beyond["code"] != 0 and "symbolic link" in beyond["out"], beyond
            detail["git"] = {"add_under_symlink": beyond, "cacheinfo_plain": plain, "cacheinfo_replace": replaced,
                             "index_after": listing["out"]}
            return detail
        case("b. COPY with a member written through an earlier symlink is refused; a repo cannot hold one", escape)

        def hard_link():
            upload = tar_of([("hl", "dir", 0o755, None),
                             ("hl/a.txt", "file", 0o644, b"linked\n"),
                             ("hl/b.txt", "hard", 0o644, "hl/a.txt")])
            status, reason = build("hl-build", [{"type": "COPY", "args": ["hl", "/opt/hl"], "files": "hl"}],
                                   {"hl": upload})
            message = reason.get("message", "")
            text = log_text()
            fell = [l for l in text.splitlines() if "COPY step" in l and "falling back to shell" in l
                    and "hard link" in l]
            if has_shell:
                assert status == "ready", (status, reason)
                assert fell, "no 'falling back to shell ... hard link' line"
                sandbox = sandbox_of("hl-build")
                status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
                assert status in (200, 201, 204), (status, value)
                assert query(sandbox, "one", "file", "/opt/hl/a.txt") == b"linked\n"
                assert query(sandbox, "one", "file", "/opt/hl/b.txt") == b"linked\n"
                stat = sh(sandbox, "stat -c '%i %h %n' /opt/hl/a.txt /opt/hl/b.txt")["stdout"].split("\n")
                rows = [l.split() for l in stat if l]
                assert len(rows) == 2 and rows[0][0] == rows[1][0] and rows[0][1] == rows[1][1] == "2", rows
                request("DELETE", f"/sandboxes/{sandbox}")
                return {"status": "ready", "stat_inode_links_name": stat, "log": re.sub(r"\[[0-9;]*m", "", fell[0]).split("DEBUG")[-1].strip()}
            assert status == "error" and "needs the shell" in message and "hard link" in message, (status, reason)
            assert "no /bin/sh" in message, message
            assert fell, "no 'falling back to shell ... hard link' line"
            return {"status": status, "message": message}
        case("c. COPY with a hard link: the shell's path with a shell, a clear error without", hard_link)

        text = log_text()
        report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                    "Tcg": text.count("Detected hypervisor platform: Tcg")}
        assert report["platform_lines"]["Tcg"] == 0 and report["platform_lines"]["Kvm"] >= 1
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        report["builds"] = {n: {"status": v["status"], "reason": v.get("reason"), "logs": [
            e["message"] for e in v.get("logEntries", [])]} for n, v in builds.items()}
        text = log_path.read_text(errors="replace")
        report.setdefault("platform_lines", {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                             "Tcg": text.count("Detected hypervisor platform: Tcg")})
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("success:", report["success"])
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
