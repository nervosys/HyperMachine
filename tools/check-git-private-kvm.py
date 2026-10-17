#!/usr/bin/env python3
"""A private Git remote, cloned with the node's own credential, on real KVM.

Serves one Git repository over plain HTTP on loopback twice: under /private/,
where every request must carry the Basic credential `bot:<token>`, and under
/public/, which asks for nothing. The server records the Authorization header
of every request it gets. Runs an owned hv2-sandboxd with both remotes allowed
and `--git-credential http://127.0.0.1:PORT/private/=TOKEN_VARIABLE`, the token
being in the daemon's own environment. Then checks:

1. A sandbox created with an environment that has the private repository has
   its tree, and the server saw the right credential on the private path.
2. The token is nowhere in the guest: not in `.git/config`, not in any file
   under /srv, /root, /tmp, /etc or /home, and not in a process's environment.
3. The credential went only where it was meant: no request to /public/ carried
   an Authorization header.
4. A node given the wrong token, and a node given none for that remote, are
   refused with 502 before anything boots, no sandbox is left, and the error
   does not repeat the token.

Writes report.json and the logs into --output.
"""
import argparse, base64, hashlib, http.server, json, os, shutil, signal, socket, subprocess, tempfile, threading, time
import urllib.error, urllib.request
from pathlib import Path

TOKEN = "tok-3f9a1c7e5b2d4a60"


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
              "purpose": "functional verification of a private Git remote cloned with a node credential on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "git": subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-git-private-check-", dir="/var/tmp"))
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=check", "-c", "user.email=check@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env, capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)
        return done.stdout.strip()

    src = work / "src"
    src.mkdir()
    git("init", "-q", "-b", "main", cwd=src)
    (src / "SECRET_PLANS.md").write_text("the private repository\n")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "private", cwd=src)
    served = work / "served"
    for side in ("private", "public"):
        (served / side).mkdir(parents=True)
        shutil.copytree(src / ".git", served / side / "app.git")
        git("update-server-info", cwd=served / side / "app.git")

    wanted = "Basic " + base64.b64encode(f"bot:{TOKEN}".encode()).decode()
    seen = []  # (path, Authorization header or None)

    class Handler(http.server.SimpleHTTPRequestHandler):
        def guard(self):
            header = self.headers.get("Authorization")
            seen.append((self.path, header))
            if self.path.startswith("/private/") and header != wanted:
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="private"')
                self.end_headers()
                return False
            return True

        def do_GET(self):
            if self.guard():
                super().do_GET()

        def do_HEAD(self):
            if self.guard():
                super().do_HEAD()

        def log_message(self, *a):
            pass

    git_port = port()
    web = http.server.ThreadingHTTPServer(
        ("127.0.0.1", git_port), lambda *a, **k: Handler(*a, directory=str(served), **k))
    threading.Thread(target=web.serve_forever, daemon=True).start()
    root = f"http://127.0.0.1:{git_port}"
    private, public = f"{root}/private/app.git", f"{root}/public/app.git"

    def start(name, credentials):
        api = port()
        log = (args.output / f"{name}.log").open("wb")
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info",
                   CHECK_GIT_TOKEN=credentials or "")
        if not credentials:
            env.pop("CHECK_GIT_TOKEN")
        cmd = [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
               "--git-remote", f"{root}/private/", "--git-remote", f"{root}/public/",
               "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"]
        if credentials:
            cmd += ["--git-credential", f"{root}/private/=CHECK_GIT_TOKEN"]
        proc = subprocess.Popen(cmd, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        return proc, log, f"http://127.0.0.1:{api}"

    def request(base, method, path, body=None, timeout=300):
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

    def ready(proc, base):
        deadline = time.monotonic() + 180
        while True:
            assert proc.poll() is None, "the daemon exited"
            try:
                if request(base, "GET", "/sandboxes", timeout=5)[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

    def stop(proc, log):
        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                proc.kill()
        log.close()

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def environment(base, name, remote):
        status, value = request(base, "PUT", f"/environments/{name}", {"repositories": [
            dict(kind="git", remote=remote, path="/srv/app")]})
        assert status == 201, (status, value)

    running = []
    try:
        proc, log, base = start("with-credential", f"bot:{TOKEN}")
        running.append((proc, log))
        ready(proc, base)

        def private_clone():
            seen.clear()
            environment(base, "private", private)
            status, created = request(base, "POST", "/sandboxes", {"templateID": "base", "environment": "private"})
            assert status in (200, 201), (status, created)
            sandbox = created["sandboxID"]

            def sh(cmd):
                status, value = request(base, "POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 60})
                assert status == 200, (status, value)
                return value["stdout"]
            tree = sh("cat /srv/app/SECRET_PLANS.md; ls -a /srv/app")
            assert tree.startswith("the private repository\n") and ".git" in tree.split(), tree
            authed = [h for p, h in seen if p.startswith("/private/")]
            assert authed and all(h == wanted for h in authed), seen
            state["sandbox"] = sandbox
            state["sh"] = sh
            return {"private requests": len(authed), "all with the credential": True}
        state = {}
        case("a private remote is cloned with the node's credential, which the server saw", private_clone)

        def not_in_the_guest():
            sh = state["sh"]
            # A control first: the search must find a token it is shown.
            found = sh(f"echo {TOKEN} > /tmp/.control; grep -rIl {TOKEN} /tmp 2>&1; rm -f /tmp/.control").strip()
            assert found == "/tmp/.control", f"the search cannot find a token: {found!r}"
            files = sh(f"grep -rIl {TOKEN} /srv /root /tmp /etc /home 2>/dev/null; echo done").strip()
            assert files == "done", files
            raw = sh(f"cat /srv/app/.git/config; for p in /proc/[0-9]*/environ; do cat $p 2>/dev/null; done | tr '\\0' '\\n' | grep -c {TOKEN} || true")
            assert TOKEN not in raw, "the token is in the guest"
            assert sh("grep -c extraHeader /srv/app/.git/config || true").strip() == "0", "the header was kept"
            return {"searched": ["/srv", "/root", "/tmp", "/etc", "/home", "/proc/*/environ"]}
        case("the token is nowhere in the guest, nor in the clone's config", not_in_the_guest)

        def only_where_meant():
            seen.clear()
            environment(base, "public", public)
            status, created = request(base, "POST", "/sandboxes", {"templateID": "base", "environment": "public"})
            assert status in (200, 201), (status, created)
            asked = [(p, h) for p, h in seen if p.startswith("/public/")]
            assert asked, seen
            assert all(h is None for p, h in asked), asked
            request(base, "DELETE", f"/sandboxes/{created['sandboxID']}")
            return {"public requests": len(asked), "with an Authorization header": 0}
        case("a remote outside the credential's prefix is sent no Authorization header", only_where_meant)

        request(base, "DELETE", f"/sandboxes/{state['sandbox']}")
        stop(proc, log)
        running.clear()

        def refused(label, credentials, why):
            proc, log, base = start(label, credentials)
            running.append((proc, log))
            ready(proc, base)
            environment(base, "private", private)
            before = request(base, "GET", "/sandboxes")[1]
            status, value = request(base, "POST", "/sandboxes", {"templateID": "base", "environment": "private"})
            assert status == 502, (status, value)
            message = value.get("message") if isinstance(value, dict) else str(value)
            assert TOKEN not in json.dumps(value), value
            assert request(base, "GET", "/sandboxes")[1] == before, "a sandbox was left"
            stop(proc, log)
            running.clear()
            return {why: message}

        def wrong_token():
            return refused("wrong-token", "bot:tok-not-the-one", "refused with a wrong token")
        case("a wrong token is refused with 502 before anything boots", wrong_token)

        def no_token():
            return refused("no-token", None, "refused with no credential")
        case("a node with no credential for the remote is refused with 502", no_token)

        report["success"] = True
    finally:
        for proc, log in running:
            stop(proc, log)
        web.shutdown()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
