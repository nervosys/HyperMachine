#!/usr/bin/env python3
"""An environment's Git repositories, in a sandbox on real KVM.

Makes a Git repository with two branches, serves it over plain HTTP on
loopback (a redirect from an allowed path to one outside it too), and runs an
owned hv2-sandboxd given two `--git-remote` prefixes: the repository's, and
the redirecting one's. Then checks:

1. An environment with the repository twice, once on its default branch and
   once on `feature`, is described without credentials.
2. A sandbox created with it has each tree in its place, the right branch's
   files in each, `.git/` with them, owned by the user commands run as, and
   no archive left behind.
3. What cannot be is refused before anything boots, and no sandbox is left:
   credentials on a Git repository; a remote the operator did not allow; a
   remote that answers with a redirect out of what it allowed.

Writes report.json and the logs into --output.
"""
import argparse, hashlib, http.server, json, os, shutil, signal, socket, subprocess, tempfile, threading, time
import urllib.error, urllib.request
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
              "purpose": "functional verification of Git repositories in environments on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "git": subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-git-repositories-check-", dir="/var/tmp"))
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=check", "-c", "user.email=check@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env,
                              capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)
        return done.stdout.strip()

    # The repository: main with two files, then a branch with one more, served
    # as a dumb HTTP repository (the bare clone with its info files written).
    src = work / "src"
    src.mkdir()
    git("init", "-q", "-b", "main", cwd=src)
    (src / "src").mkdir()
    (src / "src" / "app.py").write_text("print('main')\n")
    (src / "README.md").write_text("on main\n")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "main", cwd=src)
    git("checkout", "-q", "-b", "feature", cwd=src)
    (src / "FEATURE.md").write_text("only on feature\n")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "feature", cwd=src)
    git("checkout", "-q", "main", cwd=src)
    served = work / "served"
    served.mkdir()
    shutil.copytree(src / ".git", served / "app.git")
    git("update-server-info", cwd=served / "app.git")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def do_GET(self):
            if self.path.startswith("/go/"):
                # A path under the redirecting prefix answers with one out of it.
                self.send_response(302)
                self.send_header("Location", "/app.git" + self.path[len("/go"):])
                self.end_headers()
                return
            super().do_GET()

        def log_message(self, *a):
            pass

    git_port = port()
    handler = lambda *a, **k: Handler(*a, directory=str(served), **k)
    web = http.server.ThreadingHTTPServer(("127.0.0.1", git_port), handler)
    threading.Thread(target=web.serve_forever, daemon=True).start()
    remote = f"http://127.0.0.1:{git_port}/app.git"
    redirecting = f"http://127.0.0.1:{git_port}/go/app.git"
    other = f"http://127.0.0.1:{git_port}/elsewhere.git"

    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--git-remote", remote, "--git-remote", f"http://127.0.0.1:{git_port}/go",
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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

    state = {}
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

        def environment():
            status, value = request("PUT", "/environments/with-git", {"repositories": [
                dict(kind="git", remote=remote, path="/srv/app"),
                dict(kind="git", remote=remote, path="/srv/feature", branch="feature"),
            ]})
            assert status == 201, (status, value)
            assert [r["path"] for r in value["repositories"]] == ["/srv/app", "/srv/feature"], value
            return {"described": value["repositories"]}
        case("an environment with a Git repository twice is described without credentials", environment)

        def in_the_sandbox():
            status, created = request("POST", "/sandboxes", {"templateID": "base", "environment": "with-git"})
            assert status in (200, 201), (status, created)
            sandbox = created["sandboxID"]
            state["sandbox"] = sandbox
            main = sh(sandbox, "cat /srv/app/README.md /srv/app/src/app.py; ls -a /srv/app")
            assert main.startswith("on main\nprint('main')\n"), main
            assert ".git" in main.split() and "FEATURE.md" not in main, main
            feature = sh(sandbox, "cat /srv/feature/FEATURE.md /srv/feature/README.md")
            assert feature == "only on feature\non main\n", feature
            uid = sh(sandbox, "id -u").strip()
            owners = sh(sandbox, "stat -c '%u' /srv/app /srv/app/src/app.py /srv/app/.git /srv/feature").split()
            assert set(owners) == {uid}, (owners, uid)
            assert sh(sandbox, "ls /tmp | grep -c hm-repository || true").strip() == "0", "an archive was left"
            return {"main": main.split()[:3], "feature": feature.split()}
        case("a sandbox created with it has each tree, the right branch's files, owned by its user", in_the_sandbox)

        def refusals():
            refused = {}
            before = request("GET", "/sandboxes")[1]

            status, value = request("PUT", "/environments/with-password", {"repositories": [
                dict(kind="git", remote=remote, path="/srv/app", username="a", password="b")]})
            assert status == 400, (status, value)
            refused["credentials on a Git repository"] = value.get("message") or value

            cases = [
                ("a remote the operator did not allow", other, 403),
                ("a remote that redirects out of what it allowed", redirecting, 502),
            ]
            for label, url, want in cases:
                env = f"bad-{want}"
                status, value = request("PUT", f"/environments/{env}", {"repositories": [
                    dict(kind="git", remote=url, path="/srv/app")]})
                assert status == 201, (status, value)
                status, value = request("POST", "/sandboxes", {"templateID": "base", "environment": env})
                assert status == want, (label, status, value)
                refused[label] = value.get("message") or value
                assert request("GET", "/sandboxes")[1] == before, f"{label}: a sandbox was left"
            return {"refused": refused}
        case("what cannot be is refused before anything boots, and no sandbox is left", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        web.shutdown()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
