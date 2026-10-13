#!/usr/bin/env python3
"""Coding agents installed in a machine at its first boot, on real KVM.

A machine from a distribution's image, created with `guestAgent` and
`agents: ["claude", "codex"]`, should come up with Claude Code and Codex
installed, and a prompt should reach each of them. No key is given: what is
checked is that the real programs are there and are what a prompt runs, up to
where each says it has no credentials. This reaches the Internet (the
distribution's archive and npm), so it is for a run by hand.

1. The machine is created with the agents named in its record, and the guest
   agent answers `exec` whether or not the install has finished.
2. The install finishes: its marker is written, `claude` and `codex` are on
   the path of a user that is not root, and each says its version.
3. A prompt for `claude`, unattended, as that user: the real Claude Code
   starts, reports its session, and ends without a key, saying so.
4. A prompt for `codex`: the real Codex starts a thread and ends without a
   key, saying so.
5. Refused, each with why and no machine made: an agent that is not offered,
   and agents for a machine without the guest agent.
6. Started again, the machine still has its agents and does not install them
   a second time.

Writes report.json, the node's log and the install's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "agents-01"
USER = "ubuntu"
READY = "/var/lib/hv2/agents-ready"
LOG = "/var/log/hv2-agents.log"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "firmware", "image", "guest-agent", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--disk-gib", type=int, default=8)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of coding agents installed at a machine's first boot and "
                         "reached by prompts, on KVM; no key, no timing claim",
              "input_sha256": {k: digest(getattr(args, k.replace("-", "_")))
                               for k in ["daemon", "firmware", "image", "guest-agent"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-agents-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(images / "distro.raw")], check=True)
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--guest-agent", str(args.guest_agent), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, RUST_LOG="info"), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=120):
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

    def run(command, seconds=60):
        return request("POST", f"/machines/{NAME}/exec", {"cmd": command, "timeout_secs": seconds},
                       timeout=seconds + 30)

    def out(command, seconds=60):
        status, value = run(command, seconds)
        assert status == 200, (status, value)
        return value["stdout"].strip()

    def as_user(command):
        return out(f"su - {USER} -c '{command}' 2>&1")

    def wait_for_exec(seconds):
        started = time.monotonic()
        while True:
            status, value = run("true")
            if status == 200 and value["exit_code"] == 0:
                return round(time.monotonic() - started, 1)
            assert status == 409 and "no guest agent answering" in value["message"], (status, value)
            assert time.monotonic() - started < seconds, f"exec still refused after {seconds}s: {value}"
            time.sleep(3)

    def prompt(seconds=300, **body):
        status, started = request("POST", f"/machines/{NAME}/prompts", body)
        assert status == 201, (status, started)
        deadline = time.monotonic() + seconds
        while True:
            status, value = request("GET", f"/machines/{NAME}/prompts/{started['promptID']}")
            assert status == 200, (status, value)
            if value["state"] != "running":
                break
            assert time.monotonic() < deadline, f"prompt still running: {value}"
            time.sleep(1)
        status, ran = request("GET", f"/machines/{NAME}/runs/{started['promptID']}?stdout=0&stderr=0")
        assert status == 200, (status, ran)
        return value, ran["stderr"]["text"]

    def events_of(value):
        return [e["event"] for e in value["events"] if "event" in e]

    def save_install_log():
        status, value = run(f"cat {LOG}")
        if status == 200:
            (args.output / "agents-install.log").write_text(value["stdout"])

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

        def created():
            status, made = request("POST", "/machines", {
                "name": NAME, "image": "distro.raw", "cpuCount": 2, "memoryMB": 2048, "diskGiB": args.disk_gib,
                "network": {"allowInternetAccess": True}, "guestAgent": True,
                "userData": "#cloud-config\nusers: [default]\n",
                "agents": ["claude", "codex", "claude"]})
            assert status == 201, (status, made)
            # Each once, in the order first asked for.
            assert made["guestAgent"] is True and made["agents"] == ["claude", "codex"], made
            status, listed = request("GET", f"/machines/{NAME}")
            assert status == 200 and listed["agents"] == made["agents"], listed
            waited = wait_for_exec(600)
            ready_then = run(f"test -e {READY}")[1]["exit_code"] == 0
            return {"seconds_until_exec_answered": waited, "install_finished_when_exec_first_answered": ready_then}
        case("the machine is created with its agents, and exec answers before or while they install", created)

        def installed():
            started = time.monotonic()
            while run(f"test -e {READY}")[1]["exit_code"] != 0:
                status, value = run("cloud-init status 2>/dev/null | head -n1")
                if status == 200 and ("done" in value["stdout"] or "error" in value["stdout"]):
                    save_install_log()
                    raise AssertionError(f"cloud-init finished without the marker: {value['stdout']!r}")
                assert time.monotonic() - started < 1200, "the agents were not installed in 20 minutes"
                time.sleep(10)
            save_install_log()
            found = {
                "seconds_after_exec_answered": round(time.monotonic() - started, 1),
                "marker": out(f"cat {READY}"),
                "node": as_user("node --version"),
                "npm": as_user("npm --version"),
                "claude_path": as_user("command -v claude"),
                "codex_path": as_user("command -v codex"),
                "claude_version": as_user("claude --version"),
                "codex_version": as_user("codex --version"),
                "installed_once": int(out(f"grep -c 'added [0-9]* packages' {LOG} || true")),
            }
            assert found["claude_path"] and found["codex_path"], found
            assert "Claude Code" in found["claude_version"], found
            assert "codex" in found["codex_version"].lower(), found
            return found
        case("the install finishes, and claude and codex are on a user's path and say their versions", installed)

        def claude():
            value, stderr = prompt(provider="claude", prompt="Say hello.", user=USER, unattended=True,
                                   cwd=f"/home/{USER}")
            events = events_of(value)
            init = [e for e in events if e.get("type") == "system" and e.get("subtype") == "init"]
            results = [e for e in events if e.get("type") == "result"]
            assert init and init[0].get("session_id"), value
            assert value["session"] == init[0]["session_id"], value
            assert results, value
            assert value["state"] == "exited" and value["exitCode"] != 0, value
            said = json.dumps(results[-1])
            assert "log" in said.lower() or "key" in said.lower() or "auth" in said.lower(), results[-1]
            return {"exitCode": value["exitCode"], "session": value["session"],
                    "claude_code_version": init[0].get("claude_code_version"),
                    "tools_offered": len(init[0].get("tools", [])),
                    "result": results[-1].get("result"), "stderr": stderr.strip()[-400:]}
        case("a prompt reaches the real Claude Code, which reports its session and ends without a key", claude)

        def codex():
            value, stderr = prompt(provider="codex", prompt="Say hello.", user=USER, cwd=f"/home/{USER}")
            events = events_of(value)
            kinds = [e.get("type") for e in events]
            assert "thread.started" in kinds, value
            errors = [e for e in events if e.get("type") in ("error", "turn.failed")]
            assert errors, value
            assert value["state"] == "exited" and value["exitCode"] != 0, value
            return {"exitCode": value["exitCode"], "event_types": kinds, "session": value["session"],
                    "last_error": json.dumps(errors[-1])[:400], "stderr": stderr.strip()[-400:]}
        case("a prompt reaches the real Codex, which starts a thread and ends without a key", codex)

        def refusals():
            results = {}
            for label, body in [
                ("an agent that is not offered",
                 {"name": "agents-02", "image": "distro.raw", "guestAgent": True, "agents": ["gemini"]}),
                ("agents without the guest agent",
                 {"name": "agents-03", "image": "distro.raw", "agents": ["claude"]}),
                ("agents for a machine that is not from an image",
                 {"name": "agents-04", "diskGiB": 1, "agents": ["codex"]}),
            ]:
                status, value = request("POST", "/machines", body)
                assert status == 400, (label, status, value)
                results[label] = value["message"]
            status, machines = request("GET", "/machines")
            assert [m["name"] for m in machines] == [NAME], machines
            return results
        case("what cannot be installed is refused, each with why, and no machine is made", refusals)

        def again():
            before = out(f"cat {READY}; md5sum {LOG}")
            status, value = request("POST", f"/machines/{NAME}/stop", timeout=180)
            assert status == 200 and value["state"] == "stopped", (status, value)
            status, value = request("POST", f"/machines/{NAME}/start", timeout=180)
            assert status == 200 and value["state"] == "running", (status, value)
            waited = wait_for_exec(420)
            after = out(f"cat {READY}; md5sum {LOG}")
            assert after == before, (before, after)
            version = as_user("claude --version")
            assert "Claude Code" in version, version
            return {"seconds_until_exec_answered": waited, "claude_version": version}
        case("started again, the machine has its agents and does not install them again", again)

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
