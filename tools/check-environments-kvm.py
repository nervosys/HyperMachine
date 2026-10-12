#!/usr/bin/env python3
"""Environments on real KVM: a named set of variables and files a sandbox
starts with.

Runs an owned hv2-sandboxd with a template and checks:

1. An environment is made; what it returns names its variables and files
   and holds none of their values.
2. A sandbox created with it has its variables in every command, its files
   with their bytes, owned by the template's user, and the request's own
   envVars over the environment's.
3. A sandbox created without it has none of it.
4. After a reboot the sandbox still has the variables and the files; a
   fork has them too.
5. A changed environment applies to sandboxes created after the change, and
   not to one already running.
6. What cannot be is refused: an environment that is not there, one that
   breaks the rules, and deleting it leaves nothing to start with.

Writes report.json and the daemon's log into --output.
"""
import argparse, base64, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

BINARY = bytes(range(256)) * 4 + b"\x00tail"


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
              "purpose": "functional verification of environments on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-environments-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "6"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=180):
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

    def create(**more):
        status, sandbox = request("POST", "/sandboxes", dict(templateID="base", timeout=600, **more))
        assert status in (200, 201), (status, sandbox)
        return sandbox["sandboxID"]

    def sh(sandbox, cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value

    def seen(sandbox):
        """What a command in the sandbox finds of the environment."""
        out = sh(sandbox, 'echo "API=$API_URL MODE=$MODE"; cat /etc/app/config.toml 2>/dev/null || echo no-config')["stdout"]
        binary = sh(sandbox, "base64 /srv/data/blob.bin 2>/dev/null | tr -d '\\n' || true")["stdout"]
        return out, binary

    def put_env(name, body):
        return request("PUT", f"/environments/{name}", body)

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

        def made():
            status, value = put_env("web", {
                "envVars": {"API_URL": "https://api.example", "MODE": "from-environment", "SECRET_TOKEN": "tok-9f2a"},
                "files": [
                    {"path": "/etc/app/config.toml", "content": "port = 8080\n"},
                    {"path": "/srv/data/blob.bin", "contentBase64": base64.b64encode(BINARY).decode()},
                ],
            })
            assert status == 201, (status, value)
            assert value["envVarNames"] == ["API_URL", "MODE", "SECRET_TOKEN"], value
            assert value["files"] == [{"path": "/etc/app/config.toml", "bytes": 12},
                                      {"path": "/srv/data/blob.bin", "bytes": len(BINARY)}], value
            status, listed = request("GET", "/environments")
            text = json.dumps(listed)
            assert status == 200 and "tok-9f2a" not in text and "port = 8080" not in text, listed
            return {"described": value}
        case("an environment is made and described without its values", made)

        def started_with_it():
            sandbox = create(environment="web", envVars={"MODE": "from-request"})
            state["with"] = sandbox
            out, binary = seen(sandbox)
            assert out == "API=https://api.example MODE=from-request\nport = 8080\n", out
            assert base64.b64decode(binary) == BINARY, binary[:80]
            owner = sh(sandbox, "stat -c '%U %u' /etc/app/config.toml /srv/data/blob.bin /srv/data")["stdout"]
            whoami = sh(sandbox, "id -un; id -u")["stdout"].split()
            # Owned by whoever the template's commands run as.
            assert all(line.split()[1] == whoami[1] for line in owner.splitlines()), (owner, whoami)
            # The secret is in the guest, which is where it is for.
            assert sh(sandbox, 'echo "$SECRET_TOKEN"')["stdout"] == "tok-9f2a\n"
            return {"seen": out, "owner": owner.splitlines(), "commands_run_as": whoami}
        case("a sandbox made with it has its variables and files, and its own envVars win", started_with_it)

        def without_it():
            sandbox = create()
            out, binary = seen(sandbox)
            assert out == "API= MODE=\nno-config\n" and binary == "", (out, binary)
            request("DELETE", f"/sandboxes/{sandbox}")
            return None
        case("a sandbox made without it has none of it", without_it)

        def across_a_reboot_and_a_fork():
            sandbox = state["with"]
            status, value = request("POST", f"/sandboxes/{sandbox}/reboot", {})
            assert status in (200, 204), (status, value)
            deadline = time.monotonic() + 120
            while True:
                status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": "true", "timeout_secs": 10})
                if status == 200:
                    break
                assert time.monotonic() < deadline, (status, value)
                time.sleep(0.5)
            out, binary = seen(sandbox)
            assert out == "API=https://api.example MODE=from-request\nport = 8080\n", out
            assert base64.b64decode(binary) == BINARY
            status, forks = request("POST", f"/sandboxes/{sandbox}/fork", {"count": 1})
            assert status in (200, 201), (status, forks)
            # One result per fork asked for: the fork, or why it was not made.
            assert "sandbox" in forks[0], forks
            fork = forks[0]["sandbox"]["sandboxID"]
            out, binary = seen(fork)
            assert out == "API=https://api.example MODE=from-request\nport = 8080\n", out
            assert base64.b64decode(binary) == BINARY
            state["fork"] = fork
            return {"fork": fork}
        case("after a reboot it still has them, and a fork has them too", across_a_reboot_and_a_fork)

        def a_change_applies_to_what_comes_after():
            status, value = put_env("web", {"envVars": {"API_URL": "https://v2.example"},
                                            "files": [{"path": "/etc/app/config.toml", "content": "port = 9090\n"}]})
            assert status == 200, (status, value)
            later = create(environment="web")
            out, binary = seen(later)
            assert out == "API=https://v2.example MODE=\nport = 9090\n" and binary == "", (out, binary)
            out, _ = seen(state["with"])
            assert out == "API=https://api.example MODE=from-request\nport = 8080\n", out
            request("DELETE", f"/sandboxes/{later}")
            return {"later": out}
        case("a change applies to sandboxes made after it, not one already running", a_change_applies_to_what_comes_after)

        def refusals():
            results = {}
            for label, method, path, body, want in [
                ("no such environment", "POST", "/sandboxes", {"templateID": "base", "environment": "absent"}, 404),
                ("a path that is not absolute", "PUT", "/environments/bad", {"files": [{"path": "rel", "content": "x"}]}, 400),
                ("a path that climbs", "PUT", "/environments/bad", {"files": [{"path": "/a/../etc/shadow", "content": "x"}]}, 400),
                ("a variable name that is not one", "PUT", "/environments/bad", {"envVars": {"A-B": "x"}}, 400),
                ("a name that is not one", "PUT", "/environments/..", {}, 400),
                ("a field that is not one", "PUT", "/environments/bad", {"secrets": {}}, 422),
            ]:
                status, value = request(method, path, body)
                assert status == want, (label, status, value)
                results[label] = value["message"] if isinstance(value, dict) and "message" in value else str(value)[:160]
            status, value = request("GET", "/environments/bad")
            assert status == 404, "a refused environment was kept"
            status, value = request("DELETE", "/environments/web")
            assert status == 204, (status, value)
            status, value = request("POST", "/sandboxes", {"templateID": "base", "environment": "web"})
            assert status == 404, (status, value)
            for sandbox in (state["with"], state["fork"]):
                request("DELETE", f"/sandboxes/{sandbox}")
            return results
        case("what cannot be is refused, and a deleted environment starts nothing", refusals)

        log.flush()
        logged = (args.output / "daemon.log").read_bytes()
        assert b"tok-9f2a" not in logged, "the environment's secret is in the node's log"
        report["secret_in_log"] = False
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
