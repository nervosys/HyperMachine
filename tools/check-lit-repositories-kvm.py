#!/usr/bin/env python3
"""An environment's Lit repositories, in a sandbox on real KVM.

Makes a Lit repository with two branches, serves it with `lit server serve`
on loopback with one account, and runs an owned hv2-sandboxd given `--lit`
and that server as its one `--lit-remote`. Then checks:

1. An environment with the repository twice, once on its default branch and
   once on `feature`, describes them without the password.
2. A sandbox created with it has each tree in its place, the right branch's
   files in each, `.lit/` with them, owned by the user commands run as.
3. What cannot be is refused before anything boots: a remote the operator
   did not allow, a wrong password, and the password is in no log.

Writes report.json and the logs into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

PASSWORD = "a-long-admin-password-for-the-check"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "lit", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    lit_version = subprocess.run([str(args.lit), "--version"], capture_output=True, text=True).stdout.strip()
    report = {"success": False,
              "purpose": "functional verification of Lit repositories in environments on KVM; no timing claim",
              "lit": lit_version,
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd", "lit"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-lit-repos-check-", dir="/var/tmp"))
    procs, logs = {}, {}

    def lit(*argv, cwd, stdin=None):
        out = subprocess.run([str(args.lit), *argv], cwd=cwd, input=stdin, capture_output=True, text=True,
                             env={"HOME": str(work / "home"), "PATH": "/usr/bin:/bin"})
        answer = json.loads(out.stdout or "{}")
        assert answer.get("status") == "ok", (argv, out.stdout[:400], out.stderr[:400])
        return answer

    # The repository: main with two files, then a branch with one more.
    repo = work / "repo"
    repo.mkdir()
    (work / "home").mkdir()
    lit("init", cwd=repo)
    (repo / "src").mkdir()
    (repo / "src" / "app.py").write_text("print('main')\n")
    (repo / "README.md").write_text("on main\n")
    lit("add", ".", cwd=repo)
    report["main_commit"] = lit("commit", "-m", "main", cwd=repo)["short_hash"]
    lit("branch", "feature", cwd=repo)
    lit("checkout", "feature", cwd=repo)
    (repo / "FEATURE.md").write_text("only on feature\n")
    lit("add", ".", cwd=repo)
    report["feature_commit"] = lit("commit", "-m", "feature", cwd=repo)["short_hash"]
    lit("checkout", "main", cwd=repo)
    lit("server", "user", "add", "admin1", "--role", "admin", "--password-stdin", cwd=repo, stdin=PASSWORD)

    lit_port = port()
    remote = f"http://127.0.0.1:{lit_port}"

    def start(name, argv, env, cwd=None):
        logs[name] = (args.output / f"{name}.log").open("wb")
        procs[name] = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                       stdout=logs[name], stderr=subprocess.STDOUT)

    start("lit-server", [str(args.lit), "server", "serve", "--bind", "127.0.0.1", "--port", str(lit_port),
                         "--no-audit"], {"HOME": str(work / "home"), "PATH": "/usr/bin:/bin"}, cwd=repo)
    api = port()
    base = f"http://127.0.0.1:{api}"
    start("daemon", [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
                     "--lit", str(args.lit), "--lit-remote", remote,
                     "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
          dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info"))

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

    try:
        deadline = time.monotonic() + 180
        while True:
            assert procs["daemon"].poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {}
        account = {"username": "admin1", "password": PASSWORD}

        def environment():
            status, value = request("PUT", "/environments/with-repos", {"repositories": [
                dict(remote=remote, path="/srv/app", **account),
                dict(remote=remote, path="/srv/feature", branch="feature", **account),
            ]})
            assert status == 201, (status, value)
            assert [r["path"] for r in value["repositories"]] == ["/srv/app", "/srv/feature"], value
            assert PASSWORD not in json.dumps(value), "the password was returned"
            assert PASSWORD not in json.dumps(request("GET", "/environments")[1])
            return {"described": value["repositories"]}
        case("an environment with a Lit repository twice is described without its password", environment)

        def in_the_sandbox():
            status, created = request("POST", "/sandboxes", {"templateID": "base", "environment": "with-repos"})
            assert status in (200, 201), (status, created)
            sandbox = created["sandboxID"]
            state["sandbox"] = sandbox
            main = sh(sandbox, "cat /srv/app/README.md /srv/app/src/app.py; ls -a /srv/app")
            assert main.startswith("on main\nprint('main')\n"), main
            assert ".lit" in main.split() and "FEATURE.md" not in main, main
            feature = sh(sandbox, "cat /srv/feature/FEATURE.md /srv/feature/README.md")
            assert feature == "only on feature\non main\n", feature
            owners = sh(sandbox, "stat -c '%u' /srv/app /srv/app/src/app.py /srv/app/.lit /srv/feature")
            uid = sh(sandbox, "id -u").strip()
            assert set(owners.split()) == {uid}, (owners, uid)
            assert sh(sandbox, "ls /tmp | grep -c hm-repository || true").strip() == "0", "an archive was left"
            request("DELETE", f"/sandboxes/{sandbox}")
            return {"main": main.splitlines()[:2], "owners_uid": uid}
        case("a sandbox has each tree in its place, on its branch, with .lit, owned by its user", in_the_sandbox)

        def refusals():
            results = {}
            for label, repo_body, want in [
                ("a remote the operator did not allow",
                 dict(remote=f"http://127.0.0.1:{lit_port + 1}", path="/srv/x", **account), 403),
                ("a wrong password",
                 dict(remote=remote, path="/srv/x", username="admin1", password="not-the-password-at-all"), 502),
                ("a branch that is not there",
                 dict(remote=remote, path="/srv/x", branch="absent", **account), 502),
            ]:
                status, value = request("PUT", "/environments/bad", {"repositories": [repo_body]})
                assert status in (200, 201), (status, value)
                before = request("GET", "/sandboxes")[1]
                status, value = request("POST", "/sandboxes", {"templateID": "base", "environment": "bad"})
                assert status == want, (label, status, value)
                assert request("GET", "/sandboxes")[1] == before, f"{label}: a sandbox was left"
                results[label] = value["message"] if isinstance(value, dict) else str(value)[:200]
            for name in ["daemon", "lit-server"]:
                logs[name].flush()
                assert PASSWORD.encode() not in (args.output / f"{name}.log").read_bytes(), f"{name} logged it"
            return results
        case("what cannot be is refused before anything boots, and no log holds the password", refusals)

        report["success"] = True
    finally:
        for name, proc in procs.items():
            if proc.poll() is None:
                proc.send_signal(signal.SIGTERM)
                try:
                    proc.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    proc.kill()
            logs[name].close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
