#!/usr/bin/env python3
"""Template builds by steps (E2B's Template.build) on a guest with no shell, on real KVM.

Runs an owned hv2-sandboxd (`--guest-kit` set to an empty directory: a build from an
existing template pulls nothing) whose base template is the image under test, and
drives the template-build API directly (POST /v3/templates, GET/PUT the COPY upload,
POST /v2/templates/{id}/builds/{buildID}, GET .../status). Each build is
`fromTemplate: base`. The files of a built template are read through a checkpoint of a
sandbox made from it (GET /sandboxes/{id}/checkpoints/{name}/files, /file, /archive);
its environment and working directory through exec with an absolute argv, no shell.

a. COPY of a file with an octal --chmod, COPY of a directory holding a script, a file
   symlink and a directory symlink, ENV (one value refers to $PATH, the base
   environment), WORKDIR (as root, and as a user), USER of a user that /etc/passwd
   holds (the build copied it in), COPY owned by that user: the build succeeds, and the
   template's files have the right content, modes, owners and link targets, no staged
   archive is left in /tmp, the process environment holds the ENV values with PATHX
   equal to /x: plus the sandbox's PATH, and its working directory is the last WORKDIR.
b. A build with a RUN step: with --shell no it fails at that step and says the image
   has no /bin/sh; with --shell yes it succeeds and its file is in the template.
c. COPY owned by a user /etc/passwd does not hold goes to the shell (a fail at its step naming
   the user: the shell's chown on a shell image, "needs the shell" without one).
d. COPY --chmod=u+x (not octal): the shell's; with --shell no the build fails saying the
   COPY needs the shell, with --shell yes it succeeds and the mode is 744.
f. COPY owned by a bare uid (4242) that /etc/passwd lacks: the agent resolves owners through
   passwd only, so this is the shell's chown; with --shell yes the file is owned by uid 4242,
   with --shell no the build fails saying the COPY needs the shell.
e. USER of a user the image lacks: with --shell no the build fails saying there is no
   shell to create it; with --shell yes the script runs (the test busybox has no adduser
   applet, so it fails with exit 127, as it always did).

The daemon runs with hv2_sandboxd::builds at debug, and the driver counts its lines
`using agent op`, `falling back to shell`, and the base environment's source, requiring
exactly the numbers each image should give. Writes report.json and the daemon's log
into --output, with the number of `Kvm` and `Tcg` platform lines (Tcg must be 0).
"""
import argparse, hashlib, io, json, os, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

PASSWD = "root:x:0:0:root:/root:/bin/sh\napp:x:1000:1000::/srv/home:/bin/sh\n"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tar_of(members):
    """members: (name, kind, mode, payload) with kind file|dir|sym; payload bytes or link target."""
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
    parser.add_argument("--shell", choices=["yes", "no"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    has_shell = args.shell == "yes"
    report = {"success": False, "label": args.label, "shell": has_shell,
              "purpose": "functional verification of template builds (COPY, ENV, WORKDIR, USER) through the "
                         "guest agent's file operations, on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-build-noshell-check-", dir="/var/tmp"))
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
                 RUST_LOG="info,hv2_sandboxd::builds=debug"),
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

    def build(name, steps, uploads=None):
        """Run a build to its end: (status, reason)."""
        status, made = request("POST", "/v3/templates", {"name": name})
        assert status == 202, (status, made)
        hashes = {}
        for label, payload in (uploads or {}).items():
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

    def tool(name):
        """A coreutils program by absolute path: an applet of /bin/busybox on a busybox image."""
        return ["/bin/busybox", name] if has_shell else [f"/bin/{name}"]

    def argv(sandbox, *cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"argv": list(cmd), "timeout_secs": 30})
        assert status == 200, (status, value)
        assert value["exit_code"] == 0, value
        return value["stdout"]

    def query(sandbox, name, route, path, want=200):
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/{route}?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (route, path, status, value)
        return value

    def listing(sandbox, name, path):
        return {e["name"]: e for e in json.loads(query(sandbox, name, "files", path))["entries"]}

    def log_lines():
        log.flush()
        return log_path.read_text(errors="replace").splitlines()

    ctx_hello = tar_of([("hello.txt", "file", 0o644, b"hello\n")])
    ctx_tree = tar_of([("tree", "dir", 0o755, None),
                       ("tree/sub", "dir", 0o750, None),
                       ("tree/sub/run.sh", "file", 0o755, b"#!/bin/true\n"),
                       ("tree/link", "sym", 0o777, "sub/run.sh"),
                       ("tree/linkdir", "sym", 0o777, "sub")])
    ctx_passwd = tar_of([("passwd", "file", 0o644, PASSWD.encode())])
    ctx_owned = tar_of([("owned.txt", "file", 0o644, b"owned\n")])

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

        def steps_build():
            steps = [
                {"type": "COPY", "args": ["passwd", "/etc/passwd"], "files": "passwd"},
                {"type": "COPY", "args": ["hello.txt", "/opt/app/hello.txt", "", "640"], "files": "hello"},
                {"type": "COPY", "args": ["tree", "/opt/tree"], "files": "tree"},
                {"type": "ENV", "args": ["GREETING", "hi", "PATHX", "/x:$PATH"]},
                {"type": "WORKDIR", "args": ["/srv/work"]},
                {"type": "USER", "args": ["app"]},
                {"type": "COPY", "args": ["owned.txt", "/srv/work/owned.txt", "app"], "files": "owned"},
                {"type": "WORKDIR", "args": ["sub"]},
            ]
            status, reason = build("nsb-ok", steps, {"passwd": ctx_passwd, "hello": ctx_hello,
                                                    "tree": ctx_tree, "owned": ctx_owned})
            assert status == "ready", (status, reason)
            sandbox = sandbox_of("nsb-ok")
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
            assert status in (200, 201, 204), (status, value)
            detail = {}
            # COPY of a file with an octal mode.
            app = listing(sandbox, "one", "/opt/app")
            assert set(app) == {"hello.txt"}, app
            assert app["hello.txt"]["mode"] == "640" and app["hello.txt"]["uid"] == 0, app["hello.txt"]
            assert query(sandbox, "one", "file", "/opt/app/hello.txt") == b"hello\n"
            detail["hello.txt"] = app["hello.txt"]
            # COPY of a directory: modes, a script, two symlinks.
            tree = listing(sandbox, "one", "/opt/tree")
            assert set(tree) == {"sub", "link", "linkdir"}, sorted(tree)
            assert tree["sub"]["type"] == "directory" and tree["sub"]["mode"] == "750", tree["sub"]
            assert tree["link"]["type"] == "symlink" and tree["linkdir"]["type"] == "symlink", tree
            sub = listing(sandbox, "one", "/opt/tree/sub")
            assert sub["run.sh"]["mode"] == "755" and sub["run.sh"]["uid"] == 0, sub
            assert query(sandbox, "one", "file", "/opt/tree/sub/run.sh") == b"#!/bin/true\n"
            req = urllib.request.Request(base + f"/sandboxes/{sandbox}/checkpoints/one/archive?"
                                         + urllib.parse.urlencode({"path": "/opt/tree"}))
            with urllib.request.urlopen(req, timeout=120) as r:
                archive = tarfile.open(fileobj=io.BytesIO(r.read()), mode="r:gz")
            members = {m.name.removeprefix("./"): m for m in archive.getmembers()}
            assert members["link"].issym() and members["link"].linkname == "sub/run.sh", members["link"]
            assert members["linkdir"].issym() and members["linkdir"].linkname == "sub", members["linkdir"]
            detail["tree"] = {n: tree[n]["type"] for n in sorted(tree)}
            # USER app, then COPY owned by it (no explicit chmod).
            work = listing(sandbox, "one", "/srv/work")
            assert work["owned.txt"]["uid"] == 1000 and work["owned.txt"]["gid"] == 1000, work["owned.txt"]
            assert query(sandbox, "one", "file", "/srv/work/owned.txt") == b"owned\n"
            # WORKDIR as root made /srv/work for root; as app it made sub for app.
            srv = listing(sandbox, "one", "/srv")
            assert srv["work"]["uid"] == 0, srv["work"]
            assert work["sub"]["type"] == "directory" and work["sub"]["uid"] == 1000, work["sub"]
            detail["work"] = {"/srv/work": srv["work"], "sub": work["sub"], "owned.txt": work["owned.txt"]}
            # No archive was staged in the guest, or left in it.
            status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/one/files?"
                                    + urllib.parse.urlencode({"path": "/tmp"}))
            tmp = [e["name"] for e in value["entries"] if "hv2-copy" in e["name"]] if status == 200 else []
            assert status in (200, 404) and not tmp, (status, tmp)
            # The process environment and working directory a command in the template has.
            env = dict(item.split("=", 1) for item in argv(sandbox, *tool("cat"), "/proc/self/environ").split("\0")
                       if "=" in item)
            assert env.get("GREETING") == "hi", env
            assert env.get("PATH") and env.get("PATHX") == "/x:" + env["PATH"], (env.get("PATH"), env.get("PATHX"))
            cwd = argv(sandbox, *tool("ls"), "-l", "/proc/self/cwd")
            assert cwd.strip().endswith("/srv/work/sub"), cwd
            detail["env"] = {"GREETING": env["GREETING"], "PATH": env["PATH"], "PATHX": env["PATHX"]}
            detail["cwd"] = cwd.strip()
            request("DELETE", f"/sandboxes/{sandbox}")
            return detail
        case("a. COPY (file+octal chmod, directory with symlinks, owned by a user), ENV, WORKDIR, USER", steps_build)

        def run_step():
            steps = [{"type": "COPY", "args": ["hello.txt", "/opt/app/hello.txt"], "files": "hello"},
                     {"type": "RUN", "args": ["echo from-run > /ran.txt"]}]
            status, reason = build("nsb-run", steps, {"hello": ctx_hello})
            if has_shell:
                assert status == "ready", (status, reason)
                sandbox = sandbox_of("nsb-run")
                assert argv(sandbox, *tool("cat"), "/ran.txt") == "from-run\n"
                request("DELETE", f"/sandboxes/{sandbox}")
                return {"status": status}
            assert status == "error", (status, reason)
            message = reason.get("message", "")
            assert reason.get("step") == "2" and "no /bin/sh" in message and "RUN" in message, reason
            return {"status": status, "step": reason["step"], "message": message}
        case("b. a RUN step: works with a shell, fails clearly without one", run_step)

        def unknown_owner():
            steps = [{"type": "COPY", "args": ["hello.txt", "/opt/app/hello.txt", "ghost"], "files": "hello"}]
            status, reason = build("nsb-ghost", steps, {"hello": ctx_hello})
            message = reason.get("message", "")
            assert status == "error" and reason.get("step") == "1" and "ghost" in message, (status, reason)
            return {"message": message}
        case("c. COPY owned by a user the image lacks fails at its step, naming the user", unknown_owner)

        def symbolic_mode():
            steps = [{"type": "COPY", "args": ["hello.txt", "/opt/app/hello.txt", "", "u+x"], "files": "hello"}]
            status, reason = build("nsb-symmode", steps, {"hello": ctx_hello})
            if has_shell:
                assert status == "ready", (status, reason)
                sandbox = sandbox_of("nsb-symmode")
                status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
                assert status in (200, 201, 204), (status, value)
                entry = listing(sandbox, "one", "/opt/app")["hello.txt"]
                assert entry["mode"] == "744", entry
                request("DELETE", f"/sandboxes/{sandbox}")
                return {"mode": entry["mode"]}
            message = reason.get("message", "")
            assert status == "error" and "needs the shell" in message and "no /bin/sh" in message, (status, reason)
            return {"message": message}
        case("d. COPY --chmod=u+x (not octal): the shell's; clear failure without one", symbolic_mode)

        def missing_user():
            steps = [{"type": "USER", "args": ["builder"]}]
            status, reason = build("nsb-user", steps)
            message = reason.get("message", "")
            if has_shell:
                # The script ran, as before this change. The test image's busybox has no `adduser`
                # applet, so it exits 127: a failure of the image, and the shell path's own.
                assert status == "error" and "builder" in message and "exited" in message                     and "no shell" not in message and "no /bin/sh" not in message, (status, reason)
                return {"status": status, "message": message}
            assert status == "error" and "builder" in message and "no shell" in message, (status, reason)
            return {"message": message}
        case("e. USER of a user the image lacks: the shell script runs; clear failure without a shell", missing_user)

        def numeric_owner():
            steps = [{"type": "COPY", "args": ["hello.txt", "/opt/app/hello.txt", "4242"], "files": "hello"}]
            status, reason = build("nsb-numeric", steps, {"hello": ctx_hello})
            message = reason.get("message", "")
            if has_shell:
                # Not in passwd, so the agent cannot resolve it; the shell's chown takes a bare uid.
                assert status == "ready", (status, reason)
                sandbox = sandbox_of("nsb-numeric")
                status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "one"})
                assert status in (200, 201, 204), (status, value)
                entry = listing(sandbox, "one", "/opt/app")["hello.txt"]
                assert entry["uid"] == 4242, entry
                request("DELETE", f"/sandboxes/{sandbox}")
                return {"uid": entry["uid"]}
            assert status == "error" and "needs the shell" in message and "4242" in message, (status, reason)
            return {"message": message}
        case("f. COPY owned by a bare uid not in passwd: the shell's chown on a shell image, clear failure without one", numeric_owner)

        # The daemon's own account of which path each step took.
        lines = log_lines()
        count = lambda needle, *also: sum(1 for l in lines if needle in l and all(a in l for a in also))
        counts = {
            "copy_agent": count("using agent op", "COPY step"),
            "copy_shell": count("falling back to shell", "COPY step"),
            "workdir_agent": count("using agent op", "WORKDIR "),
            "workdir_shell": count("falling back to shell", "WORKDIR "),
            "user_agent": count("using agent op", "USER "),
            "user_shell": count("falling back to shell", "USER "),
            "env_busybox": count("reading the base environment: using busybox env"),
            "env_proc": count("reading the base environment: falling back to /proc/self/environ"),
            "kvm_lines": count("Detected hypervisor platform: Kvm"),
            "tcg_lines": count("Tcg"),
        }
        builds_run = 6
        expected = {
            # a: 4 COPYs, b copies one. The shell takes c (owner not in passwd), d (u+x), f (bare uid).
            "copy_agent": 4 + 1,
            "copy_shell": 3,
            "workdir_agent": 2, "workdir_shell": 0,
            "user_agent": 1, "user_shell": 1,
            "env_busybox": builds_run if has_shell else 0,
            "env_proc": 0 if has_shell else builds_run,
        }
        report["log_counts"] = counts
        report["log_counts_expected"] = expected
        for key, want in expected.items():
            assert counts[key] == want, (key, counts[key], want)
        assert counts["tcg_lines"] == 0 and counts["kvm_lines"] >= 1, counts
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
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
