#!/usr/bin/env python3
"""Ownership by a non-root template user, through a real guest on KVM.

Every image the agent-ops checks used ran as root, so the `@template` owner on make_dir,
write_file (owner), make_symlink and set_attr -R during an environment repository's unpack
(`env_vars.rs`, `unpack_by_agent`, now one batch) was only unit-tested. This builds a template
whose default user is `alice` (uid/gid 1000: the build COPYs an /etc/passwd and /etc/group that
hold her, then `USER alice`), makes a sandbox from it with an environment that holds a Git
repository (served over plain HTTP on loopback via --git-remote) and a file, and reads the
whole result back through a checkpoint's `files` listing, which reports uid, gid and mode.

Cases:

a. The template build: COPY passwd, COPY group, USER alice (found in passwd: no script),
   COPY of a directory tree (subdirectory, a 0755 script, a file symlink, a directory
   symlink) owned by alice, COPY of a file owned by alice, WORKDIR as alice, and a COPY with
   no owner after USER (it defaults to the build's current user, as `copy` in builds.rs reads).
b. The environment's repository unpacked into /srv/app, every entry of the whole tree (the
   directories, files and symlinks, .git included) listed through the checkpoint: uid and gid
   1000 on every one, a tracked file's mode as in the repository (644 or 755), the symlinks
   named symlinks, bytes of every tracked file equal to the repository's, no archive left in
   /tmp. The environment's file /etc/app/config.toml and the directory made for it are
   alice's too.
c. The COPY trees from case a are alice's, link entries included.
d. The daemon's log must show the code path the image should take.

--agent-ops says which: `new` (the agent has file operations and `batch`) or `old` (an agent
from before the file operations: the shell's `tar` and `chown`; the observed owners are
recorded, and where they differ from `new` the difference is the finding, not a failure of
the driver). --shell says whether the image has a /bin/sh. Writes report.json and the
daemon's log into --output, with the number of `Kvm` and `Tcg` platform lines (Tcg must be 0).
"""
import argparse, hashlib, http.server, io, json, os, shutil, signal, socket, subprocess, tarfile, tempfile, threading, time
import urllib.error, urllib.parse, urllib.request
from collections import Counter
from pathlib import Path

PASSWD = "root:x:0:0:root:/root:/bin/sh\nalice:x:1000:1000::/srv/home:/bin/sh\n"
GROUP = "root:x:0:\nalice:x:1000:\n"
UID = GID = 1000


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tar_of(members):
    """members: (name, kind, mode, payload) with kind file|dir|sym."""
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w") as tar:
        for name, kind, mode, payload in members:
            info = tarfile.TarInfo(name)
            info.mode = mode
            if kind == "dir":
                info.type = tarfile.DIRTYPE
                tar.addfile(info)
            elif kind == "sym":
                info.type = tarfile.SYMTYPE
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
    parser.add_argument("--agent-ops", choices=["new", "old"], required=True)
    parser.add_argument("--shell", choices=["yes", "no"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    has_shell = args.shell == "yes"
    report = {"success": False, "label": args.label, "agent_ops": args.agent_ops, "shell": has_shell,
              "purpose": "functional verification of ownership by a non-root template user (environment "
                         "repository unpack, COPY), on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-template-user-check-", dir="/var/tmp"))
    kit = work / "kit"
    kit.mkdir()
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=check", "-c", "user.email=check@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env, capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)
        return done.stdout.strip()

    # The repository: nested directories, a script, dot file, empty file, three symlinks.
    src = work / "src"
    src.mkdir()
    git("init", "-q", "-b", "main", cwd=src)
    (src / "src" / "deep" / "er").mkdir(parents=True)
    (src / "bin").mkdir()
    (src / "README.md").write_text("on main\n")
    (src / ".hidden").write_text("dot\n")
    (src / "empty.txt").write_text("")
    (src / "src" / "app.py").write_text("print('main')\n")
    (src / "src" / "deep" / "er" / "leaf.txt").write_text("leaf\n")
    (src / "bin" / "run.sh").write_text("#!/bin/sh\necho run\n")
    os.chmod(src / "bin" / "run.sh", 0o755)
    (src / "big.bin").write_bytes(bytes(range(256)) * 400)
    os.symlink("README.md", src / "link-to-readme")
    os.symlink("src", src / "link-to-src")
    os.symlink("../README.md", src / "src" / "link-up")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "main", cwd=src)
    tracked = git("ls-files", cwd=src).split("\n")
    served = work / "served"
    served.mkdir()
    shutil.copytree(src / ".git", served / "app.git")
    git("update-server-info", cwd=served / "app.git")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

    git_port = port()
    web = http.server.ThreadingHTTPServer(
        ("127.0.0.1", git_port), lambda *a, **k: Handler(*a, directory=str(served), **k))
    threading.Thread(target=web.serve_forever, daemon=True).start()
    remote = f"http://127.0.0.1:{git_port}/app.git"

    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    rust_log = ("info,hv2_sandboxd::env_vars=debug,hv2_sandboxd::builds=debug,"
                "hv2_sandboxd::checkpoint_files=debug")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(kit),
         "--git-remote", remote, "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG=rust_log),
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
            if raw and response.status == 200:
                return response.status, raw_data
            try:
                return response.status, json.loads(raw_data) if raw_data else None
            except ValueError:
                return response.status, raw_data.decode(errors="replace")

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

    def query(sandbox, name, route, path, want=200):
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/{route}?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (route, path, status, value)
        return value

    def listing(sandbox, name, path):
        return json.loads(query(sandbox, name, "files", path))["entries"]

    def walk(sandbox, name, path, seen):
        """Every entry below `path`, directories followed (symlinks not): path -> entry."""
        for entry in listing(sandbox, name, path):
            full = path.rstrip("/") + "/" + entry["name"]
            seen[full] = entry
            if entry["type"] == "directory":
                walk(sandbox, name, full, seen)
        return seen

    def owner_of(entry):
        return (entry["uid"], entry["gid"])

    ctx_passwd = tar_of([("passwd", "file", 0o644, PASSWD.encode())])
    ctx_group = tar_of([("group", "file", 0o644, GROUP.encode())])
    ctx_tree = tar_of([("tree", "dir", 0o755, None),
                       ("tree/sub", "dir", 0o750, None),
                       ("tree/sub/run.sh", "file", 0o755, b"#!/bin/true\n"),
                       ("tree/sub/data.txt", "file", 0o640, b"data\n"),
                       ("tree/link", "sym", 0o777, "sub/run.sh"),
                       ("tree/linkdir", "sym", 0o777, "sub")])
    ctx_owned = tar_of([("owned.txt", "file", 0o644, b"owned\n")])
    ctx_plain = tar_of([("plain.txt", "file", 0o644, b"plain\n")])
    state = {}

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

        def template_build():
            steps = [
                {"type": "COPY", "args": ["passwd", "/etc/passwd"], "files": "passwd"},
                {"type": "COPY", "args": ["group", "/etc/group"], "files": "group"},
                {"type": "USER", "args": ["alice"]},
                {"type": "COPY", "args": ["tree", "/opt/tree", "alice"], "files": "tree"},
                {"type": "COPY", "args": ["owned.txt", "/srv/work/owned.txt", "alice"], "files": "owned"},
                {"type": "COPY", "args": ["plain.txt", "/srv/work/plain.txt"], "files": "plain"},
                {"type": "WORKDIR", "args": ["/srv/work/sub"]},
            ]
            status, reason = build("tuser", steps, {"passwd": ctx_passwd, "group": ctx_group, "tree": ctx_tree,
                                                    "owned": ctx_owned, "plain": ctx_plain})
            assert status == "ready", (status, reason)
            return {"status": status, "steps": len(steps)}
        case("a. template build: COPY passwd+group, USER alice, COPY owned by alice, WORKDIR as alice",
             template_build)

        status, value = request("PUT", "/environments/tu", {
            "files": [{"path": "/etc/app/config.toml", "content": "port = 9090\n"}],
            "repositories": [dict(kind="git", remote=remote, path="/srv/app")]})
        assert status == 201, (status, value)
        status, made = request("POST", "/sandboxes", {"templateID": "tuser", "timeout": 1200,
                                                      "environment": "tu"})
        assert status in (200, 201), (status, made)
        sandbox = made["sandboxID"]
        status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
        assert status in (200, 201, 204), (status, value)

        def repository():
            seen = walk(sandbox, "one", "/srv/app", {})
            by_rel = {p[len("/srv/app/"):]: e for p, e in seen.items()}
            kinds = Counter(e["type"] for e in seen.values())
            owners = Counter(owner_of(e) for e in seen.values())
            detail = {"entries": len(seen), "kinds": dict(kinds),
                      "owners": {f"{u}:{g}": n for (u, g), n in owners.items()}}
            root = [e for e in listing(sandbox, "one", "/srv") if e["name"] == "app"][0]
            detail["root_dir"] = root
            detail["root_dir_owner"] = f"{root['uid']}:{root['gid']}"
            if args.agent_ops == "new":
                assert set(owners) == {(UID, GID)}, (owners, [p for p, e in seen.items() if owner_of(e) != (UID, GID)][:20])
                assert owner_of(root) == (UID, GID), root
            # Modes and kinds against the repository, file by file.
            modes = {}
            for rel in tracked:
                local = src / rel
                got = by_rel.get(rel)
                assert got is not None, ("missing", rel)
                if local.is_symlink():
                    assert got["type"] == "symlink", (rel, got)
                    modes[rel] = ("symlink", got["uid"], got["gid"])
                    continue
                want = "755" if os.access(local, os.X_OK) else "644"
                assert got["type"] == "file" and got["mode"] == want, (rel, got, want)
                assert query(sandbox, "one", "file", f"/srv/app/{rel}") == local.read_bytes(), rel
                modes[rel] = (got["mode"], got["uid"], got["gid"])
            detail["tracked"] = modes
            dirs = {}
            for rel, e in sorted(by_rel.items()):
                if e["type"] == "directory" and not rel.startswith(".git/"):
                    dirs[rel] = (e["mode"], e["uid"], e["gid"])
            detail["dirs"] = dirs
            detail["dir_modes_outside_git"] = sorted({m for m, _, _ in dirs.values()})
            git_modes = Counter((e["type"], e["mode"]) for p, e in by_rel.items() if p.startswith(".git"))
            detail["git_dir_modes"] = {f"{t}:{m}": n for (t, m), n in sorted(git_modes.items())}
            for rel in ("link-to-readme", "link-to-src", "src/link-up"):
                assert by_rel[rel]["type"] == "symlink", by_rel[rel]
            detail["symlinks"] = {rel: by_rel[rel] for rel in ("link-to-readme", "link-to-src", "src/link-up")}
            tmp = [e["name"] for e in listing(sandbox, "one", "/tmp")]
            assert not [n for n in tmp if "hm-repository" in n], tmp
            detail["tmp"] = tmp
            return detail
        case("b. environment repository: whole tree owned by the template user, modes as in the repository",
             repository)

        def env_file():
            etc = {e["name"]: e for e in listing(sandbox, "one", "/etc")}
            app = {e["name"]: e for e in listing(sandbox, "one", "/etc/app")}
            detail = {"/etc/app": etc["app"], "config.toml": app["config.toml"]}
            assert query(sandbox, "one", "file", "/etc/app/config.toml") == b"port = 9090\n"
            if args.agent_ops == "new":
                assert owner_of(app["config.toml"]) == (UID, GID), app["config.toml"]
                assert owner_of(etc["app"]) == (UID, GID), etc["app"]
            return detail
        case("b2. environment file and the directory made for it", env_file)

        def copy_trees():
            seen = walk(sandbox, "one", "/opt/tree", {})
            work_entries = {e["name"]: e for e in listing(sandbox, "one", "/srv/work")}
            srv = {e["name"]: e for e in listing(sandbox, "one", "/srv")}
            opt = {e["name"]: e for e in listing(sandbox, "one", "/opt")}
            detail = {"/opt/tree": {p[len("/opt/tree/"):]: [e["type"], e["mode"], e["uid"], e["gid"]]
                                    for p, e in sorted(seen.items())},
                      "/opt/tree dir itself": [opt["tree"]["mode"], opt["tree"]["uid"], opt["tree"]["gid"]],
                      "owned.txt": work_entries["owned.txt"], "plain.txt": work_entries["plain.txt"],
                      "/srv/work": srv["work"], "sub": work_entries.get("sub")}
            assert set(seen) == {"/opt/tree/sub", "/opt/tree/sub/run.sh", "/opt/tree/sub/data.txt",
                                 "/opt/tree/link", "/opt/tree/linkdir"}, sorted(seen)
            assert seen["/opt/tree/link"]["type"] == "symlink" and seen["/opt/tree/linkdir"]["type"] == "symlink"
            assert seen["/opt/tree/sub/run.sh"]["mode"] == "755" and seen["/opt/tree/sub"]["mode"] == "750"
            assert seen["/opt/tree/sub/data.txt"]["mode"] == "640"
            # The agent sets the owner's primary group too; the shell's `chown -h alice` leaves the
            # group it had. On `old` the uid must be 1000 and the gids are recorded, not required.
            want = (lambda e: owner_of(e) == (UID, GID)) if args.agent_ops == "new" else (lambda e: e["uid"] == UID)
            for p, e in seen.items():
                assert want(e), (p, e)
            assert want(opt["tree"]), opt["tree"]
            assert want(work_entries["owned.txt"]), work_entries["owned.txt"]
            assert query(sandbox, "one", "file", "/srv/work/owned.txt") == b"owned\n"
            assert query(sandbox, "one", "file", "/opt/tree/sub/data.txt") == b"data\n"
            return detail
        case("c. COPY owned by the template user: every entry of the tree, links included, is 1000:1000", copy_trees)

        def who():
            # What a command in this sandbox runs as: recorded only.
            if not has_shell:
                return {"recorded": "no shell; not asked"}
            status, value = request("POST", f"/sandboxes/{sandbox}/exec",
                                    {"argv": ["/bin/busybox", "id"], "timeout_secs": 30})
            return {"status": status, "exec_id": value}
        case("e. which user an exec runs as (recorded)", who)

        request("DELETE", f"/sandboxes/{sandbox}")
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        web.shutdown()
        text = log_path.read_text(errors="replace")
        lines = text.splitlines()
        count = lambda needle: sum(1 for l in lines if needle in l)
        report["platform_lines"] = {"Kvm": count("Detected hypervisor platform: Kvm"),
                                    "Tcg": count("Detected hypervisor platform: Tcg")}
        report["code_path_lines"] = {
            "unpacking a repository: using agent op": count("unpacking a repository into /srv/app: using agent op"),
            "unpacking a repository: falling back to shell": count("unpacking a repository into /srv/app: falling back to shell"),
            "no batch operation": count("has no batch operation"),
            "COPY step using agent op": sum(1 for l in lines if "using agent op" in l and "COPY step" in l),
            "COPY step falling back to shell": sum(1 for l in lines if "falling back to shell" in l and "COPY step" in l),
        }
        report["builds"] = {n: {"status": v["status"], "reason": v.get("reason"), "logs": [
            e["message"] for e in v.get("logEntries", [])]} for n, v in builds.items()}
        if all(row["success"] for row in report["cases"]) and report["cases"]:
            try:
                path = report["code_path_lines"]
                if args.agent_ops == "new":
                    assert path["unpacking a repository: using agent op"] == 1, path
                    assert path["unpacking a repository: falling back to shell"] == 0, path
                    assert path["no batch operation"] == 0, path
                else:
                    assert path["unpacking a repository: falling back to shell"] >= 1, path
                    assert path["unpacking a repository: using agent op"] == 0, path
                assert report["platform_lines"]["Tcg"] == 0 and report["platform_lines"]["Kvm"] >= 1, report["platform_lines"]
                report["success"] = True
            except AssertionError as error:
                report["code_path_error"] = repr(error)
                print("FAIL: code path", error, flush=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("success:", report["success"])
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
