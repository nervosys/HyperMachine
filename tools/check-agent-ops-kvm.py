#!/usr/bin/env python3
"""Node-initiated guest commands through the guest agent's file operations, on real KVM.

Runs an owned hv2-sandboxd with a template, a --machine-dir and one --git-remote
(a repository served over plain HTTP on loopback), its node initrd being the image
under test, and checks through the API:

a. A sandbox's processes (GET /sandboxes/{id}/processes): PID 1, the guest agent with
   its command line, every field present, kernel threads only with ?all=true.
b. A machine's processes (GET /machines/{name}/processes), the same way.
c. An environment's file and Git repository written into a sandbox, then read back
   through its checkpoint (GET /sandboxes/{id}/checkpoints/{name}/files and /file): the
   file's bytes, the repository's tree with modes and one owner, no archive left, a
   missing path 404, a file listed 400, a directory read 400.
d. GET .../archive of /etc/app: 200 and a tar holding config.toml with its bytes, on every
   image, shell or none: the host walks the checkpoint with the agent's stat, list_dir,
   read_file and read_link (an old agent: the shell's tar).
e. GET .../archive of the repository /srv/app, which holds two symlinks (one to a file, one
   to a directory): the tar holds both as symlink entries with the right targets, the
   x-hv2-archive-skipped header is present, and the checkpoint's `files` listing names
   them symlinks (the unpack made them with make_symlink; an old agent: the shell's tar).
f. The checkpoint reader's freeze: a long-running process is started (`setsid sleep 4321`,
   images with a shell only), a checkpoint is read, and the daemon log must show the freeze
   took the agent path (`using agent op (freeze)`) on a new agent and the shell path
   (`falling back to shell (freeze)`) on an old one. Where the reader's /proc/<pid>/stat
   can be read through the checkpoint's file route, its state must be `T` (stopped).

--agent-ops says which code path the image must take: `new` (the agent has the file
operations: the daemon log must show "using agent op" and never "falling back to
shell") or `old` (an agent from before them: the daemon falls back to the shell, and
the log must show that and never "using agent op"). --shell says whether the image
has a /bin/sh. Writes report.json and the daemon's log into --output, with the number
of `Kvm` and `Tcg` platform lines the log holds.
"""
import argparse, hashlib, http.server, io, json, os, shutil, signal, socket, subprocess, tarfile, tempfile, threading, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

MACHINE = "ops-01"
AGENT_LOG = "using agent op"
SHELL_LOG = "falling back to shell"
REQUIRED = ["pid", "ppid", "uid", "state", "command", "args", "threads", "rssKiB", "cpuSeconds",
            "startedSecondsAgo"]


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
    parser.add_argument("--agent-ops", choices=["new", "old"], required=True)
    parser.add_argument("--shell", choices=["yes", "no"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    has_shell = args.shell == "yes"
    report = {"success": False, "label": args.label, "agent_ops": args.agent_ops, "shell": has_shell,
              "purpose": "functional verification of node-initiated guest commands through the agent's file "
                         "operations, on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-agent-ops-check-", dir="/var/tmp"))
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=check", "-c", "user.email=check@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env, capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)
        return done.stdout.strip()

    # The repository: two files in main, served as a dumb HTTP repository.
    src = work / "src"
    src.mkdir()
    git("init", "-q", "-b", "main", cwd=src)
    (src / "src").mkdir()
    (src / "src" / "app.py").write_text("print('main')\n")
    (src / "README.md").write_text("on main\n")
    # A symlink to a file and one to a directory, as the unpack and the archive must keep them.
    os.symlink("README.md", src / "link-to-readme")
    os.symlink("src", src / "link-to-src")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "main", cwd=src)
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
    rust_log = ("info,hv2_sandboxd::processes=debug,hv2_sandboxd::checkpoint_files=debug,"
                "hv2_sandboxd::env_vars=debug,hv2_sandboxd::machines::desktop=debug")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--machine-dir", str(work / "machines"), "--git-remote", remote,
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG=rust_log),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value["stdout"]

    def ps(path, all=False, want=200):
        status, value = request("GET", f"{path}/processes" + ("?all=true" if all else ""))
        assert status == want, (path, status, value)
        return value

    def checked_processes(listed):
        for p in listed:
            assert all(k in p for k in REQUIRED), p
            assert isinstance(p["pid"], int) and isinstance(p["args"], str) and p["state"], p
        init = [p for p in listed if p["pid"] == 1]
        assert init and init[0]["ppid"] == 0, init
        # With a shell the agent is a process of its own; with none it is PID 1,
        # which the image names /init.
        agents = [p for p in listed if "hv2-guest-agentd" in p["args"]]
        if not has_shell:
            agents = [p for p in listed if p["pid"] == 1 and p["args"] == "/init"]
        assert agents, [p["args"] for p in listed]
        # Each process once, in PID order, none of them the listing itself.
        pids = [p["pid"] for p in listed]
        assert pids == sorted(set(pids)), pids
        return init[0], agents[0]

    def query(sandbox, name, route, path, want=200):
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints/{name}/{route}?"
                                + urllib.parse.urlencode({"path": path}), raw=True)
        assert status == want, (route, path, status, value)
        return value

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

        status, value = request("PUT", "/environments/ops", {
            "files": [{"path": "/etc/app/config.toml", "content": "port = 9090\n"}],
            "repositories": [dict(kind="git", remote=remote, path="/srv/app")]})
        assert status == 201, (status, value)
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200,
                                                      "environment": "ops"})
        assert status in (200, 201), (status, made)
        sandbox = made["sandboxID"]
        sbx = f"/sandboxes/{sandbox}"

        def sandbox_processes():
            plain = ps(sbx)["processes"]
            init, agent = checked_processes(plain)
            every = ps(sbx, all=True)["processes"]
            kernel = [p for p in every if p["pid"] == 2 or p["ppid"] == 2]
            assert kernel and not [p for p in plain if p["pid"] == 2 or p["ppid"] == 2]
            assert len(every) == len(plain) + len(kernel)
            detail = {"count": len(plain), "with_kernel_threads": len(every), "init": init, "agent": agent}
            if has_shell:
                sh(sandbox, "setsid sleep 4321 >/dev/null 2>&1 &")
                time.sleep(0.5)
                sleeper = [p for p in ps(sbx)["processes"] if p["args"] == "sleep 4321"]
                assert len(sleeper) == 1 and sleeper[0]["state"] == "S" and sleeper[0]["command"] == "sleep", sleeper
                detail["sleeper"] = sleeper[0]
            return detail
        case("a sandbox's processes: PID 1, the agent's command line, kernel threads only with all", sandbox_processes)

        def machine_processes():
            status, created = request("POST", "/machines", {"name": MACHINE, "diskGiB": 1, "memoryMB": 512,
                                                            "cpuCount": 1}, timeout=240)
            assert status == 201, (status, created)
            path = f"/machines/{MACHINE}"
            deadline = time.monotonic() + 120
            while True:
                status, value = request("GET", f"{path}/processes")
                if status == 200:
                    break
                assert time.monotonic() < deadline, ("machine agent", status, value)
                time.sleep(0.5)
            init, agent = checked_processes(value["processes"])
            return {"count": len(value["processes"]), "init": init, "agent": agent}
        case("a machine's processes, the same way", machine_processes)

        status, value = request("POST", f"{sbx}/checkpoints", {"name": "one"})
        assert status in (200, 201, 204), (status, value)

        def checkpoint_files():
            listed = json.loads(query(sandbox, "one", "files", "/etc/app"))
            names = {e["name"]: e for e in listed["entries"]}
            assert set(names) == {"config.toml"}, names
            entry = names["config.toml"]
            assert entry["type"] == "file" and entry["size"] == 12, entry
            assert query(sandbox, "one", "file", "/etc/app/config.toml") == b"port = 9090\n"
            root = {e["name"] for e in json.loads(query(sandbox, "one", "files", "/"))["entries"]}
            assert "etc" in root, sorted(root)
            refused = {}
            for label, route, path, want in [
                    ("no such path", "files", "/etc/nothing", 404),
                    ("no such file", "file", "/etc/nothing", 404),
                    ("a file listed", "files", "/etc/app/config.toml", 400),
                    ("a directory read", "file", "/etc/app", 400),
                    ("a relative path", "file", "etc/app/config.toml", 400)]:
                refused[label] = query(sandbox, "one", route, path, want)["message"]
            return {"entry": entry, "root": sorted(root), "refused": refused}
        case("an environment's file is listed and read through the checkpoint", checkpoint_files)

        def repository():
            listed = json.loads(query(sandbox, "one", "files", "/srv/app"))["entries"]
            names = {e["name"]: e for e in listed}
            assert {"README.md", "src", ".git", "link-to-readme", "link-to-src"} <= set(names), sorted(names)
            order = [e["name"] for e in listed]
            assert order.index(".git") > order.index("src"), order  # dot files after the rest, as `* .*`
            assert names["README.md"]["type"] == "file" and names["README.md"]["mode"] == "644", names["README.md"]
            assert names["src"]["type"] == "directory" and names["src"]["mode"] == "755", names["src"]
            assert query(sandbox, "one", "file", "/srv/app/README.md") == b"on main\n"
            assert query(sandbox, "one", "file", "/srv/app/src/app.py") == b"print('main')\n"
            inner = json.loads(query(sandbox, "one", "files", "/srv/app/src"))["entries"]
            assert [e["name"] for e in inner] == ["app.py"], inner
            # One owner for the whole tree: the template's user, as `chown -R` left it.
            git_dir = json.loads(query(sandbox, "one", "files", "/srv/app/.git"))["entries"]
            owners = {(e["uid"], e["gid"]) for e in listed + inner + git_dir}
            assert len(owners) == 1, owners
            # Every file the repository holds, byte for byte.
            tracked = git("ls-files", cwd=src).split()
            links = {rel for rel in tracked if (src / rel).is_symlink()}
            assert links == {"link-to-readme", "link-to-src"}, links
            for rel in [t for t in tracked if t not in links]:
                assert query(sandbox, "one", "file", f"/srv/app/{rel}") == (src / rel).read_bytes(), rel
            # No archive left behind.
            tmp = [e["name"] for e in json.loads(query(sandbox, "one", "files", "/tmp"))["entries"]]
            assert not [n for n in tmp if "hm-repository" in n], tmp
            return {"tree_owner": sorted(owners), "git_entries": len(git_dir), "tracked": tracked,
                    "symlinks_listed": {n: names[n]["type"] for n in sorted(links)}}
        case("an environment's Git repository is unpacked, one owner, modes kept, no archive left", repository)

        def fetch_archive(sandbox_id, name, path, want=200):
            req = urllib.request.Request(base + f"/sandboxes/{sandbox_id}/checkpoints/{name}/archive?"
                                         + urllib.parse.urlencode({"path": path}))
            try:
                response = urllib.request.urlopen(req, timeout=300)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                data = response.read()
                assert response.status == want, (path, response.status, data[:300])
                return response.headers, data

        def tar_of(data):
            return tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")

        def archive():
            headers, data = fetch_archive(sandbox, "one", "/etc/app")
            tar = tar_of(data)
            members = {m.name.removeprefix("./"): m for m in tar.getmembers()}
            assert "config.toml" in members, sorted(members)
            assert members["config.toml"].isfile(), members["config.toml"]
            assert tar.extractfile(members["config.toml"]).read() == b"port = 9090\n"
            assert headers.get("content-type") == "application/gzip", headers
            assert headers.get("x-hv2-archive-skipped") is not None, dict(headers)
            return {"status": 200, "entries": sorted(members), "skipped_header": headers["x-hv2-archive-skipped"]}
        case("the archive route: 200 on every image, config.toml with its bytes", archive)

        def repository_archive():
            headers, data = fetch_archive(sandbox, "one", "/srv/app")
            tar = tar_of(data)
            members = {m.name.removeprefix("./"): m for m in tar.getmembers()}
            for link, target in [("link-to-readme", "README.md"), ("link-to-src", "src")]:
                assert link in members, sorted(members)[:40]
                assert members[link].issym() and members[link].linkname == target, (
                    link, members[link].type, members[link].linkname)
            assert tar.extractfile(members["README.md"]).read() == b"on main\n"
            assert tar.extractfile(members["src/app.py"]).read() == b"print('main')\n"
            assert members["src"].isdir() and members["README.md"].isfile()
            # The checkpoint's own listing names them symlinks.
            listed = {e["name"]: e for e in json.loads(query(sandbox, "one", "files", "/srv/app"))["entries"]}
            assert listed["link-to-readme"]["type"] == "symlink", listed["link-to-readme"]
            assert listed["link-to-src"]["type"] == "symlink", listed["link-to-src"]
            skipped = headers.get("x-hv2-archive-skipped")
            assert skipped is not None, dict(headers)
            return {"status": 200, "members": len(members),
                    "symlinks": {n: members[n].linkname for n in ("link-to-readme", "link-to-src")},
                    "listing_types": {n: listed[n]["type"] for n in ("link-to-readme", "link-to-src")},
                    "skipped_header": skipped}
        case("the archive of the repository holds both symlinks; the listing names them symlinks",
             repository_archive)

        def freeze():
            def counts():
                text = log_path.read_text(errors="replace")
                return (text.count("using agent op (freeze)"), text.count("falling back to shell (freeze)"))
            detail = {}
            pid = None
            if has_shell:
                # Case a left one running; start another only if it is gone.
                sleeper = [p for p in ps(sbx)["processes"] if p["args"] == "sleep 4321"]
                if not sleeper:
                    sh(sandbox, "setsid sleep 4321 >/dev/null 2>&1 &")
                    time.sleep(0.5)
                    sleeper = [p for p in ps(sbx)["processes"] if p["args"] == "sleep 4321"]
                assert len(sleeper) == 1 and sleeper[0]["state"] == "S", sleeper
                pid = sleeper[0]["pid"]
                detail["sleeper_pid"] = pid
            else:
                detail["sleeper"] = ("none: a guest with no shell runs only its agent here, "
                                     "so only the code path is shown")
            status, value = request("POST", f"{sbx}/checkpoints", {"name": "two"})
            assert status in (200, 201, 204), (status, value)
            before = counts()
            query(sandbox, "two", "file", "/etc/app/config.toml")  # opens a reader: restore, freeze
            after = counts()
            agent_delta, shell_delta = after[0] - before[0], after[1] - before[1]
            detail["freeze_log_lines"] = {"using agent op (freeze)": agent_delta,
                                          "falling back to shell (freeze)": shell_delta}
            if args.agent_ops == "new":
                assert agent_delta >= 1 and shell_delta == 0, detail
            else:
                assert shell_delta >= 1 and agent_delta == 0, detail
            detail["stopped_state"] = "not observed"
            if pid is not None:
                status, value = request("GET", f"{sbx}/checkpoints/two/file?"
                                        + urllib.parse.urlencode({"path": f"/proc/{pid}/stat"}), raw=True)
                if status == 200:
                    text = value.decode(errors="replace")
                    state_letter = text.rsplit(")", 1)[1].split()[0]
                    detail["proc_stat"] = text.strip()[:120]
                    assert state_letter == "T", ("the frozen process is not stopped", text)
                    detail["stopped_state"] = "T"
                else:
                    detail["stopped_state"] = f"not observed: file of /proc/{pid}/stat answered {status}: {value}"
            return detail
        case("the checkpoint reader's freeze takes the expected path (state T where readable)", freeze)

        request("DELETE", sbx)
        request("POST", f"/machines/{MACHINE}/stop", {})
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
        report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                    "Tcg": text.count("Detected hypervisor platform: Tcg")}
        report["code_path_lines"] = {"using agent op": text.count(AGENT_LOG),
                                     "falling back to shell": text.count(SHELL_LOG)}
        if all(row["success"] for row in report["cases"]) and report["cases"]:
            try:
                if args.agent_ops == "new":
                    assert text.count(AGENT_LOG) > 0 and text.count(SHELL_LOG) == 0, report["code_path_lines"]
                else:
                    assert text.count(SHELL_LOG) > 0 and text.count(AGENT_LOG) == 0, report["code_path_lines"]
                assert report["platform_lines"]["Tcg"] == 0, report["platform_lines"]
                report["success"] = len(report["cases"]) == 7
                print("ok: the daemon took the expected code path", report["code_path_lines"], flush=True)
            except AssertionError as error:
                report["code_path_error"] = repr(error)
                print("FAIL: code path", error, flush=True)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("success:", report["success"])
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
