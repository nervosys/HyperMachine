#!/usr/bin/env python3
"""Prompts on real KVM: a coding agent's command line run in a machine.

No coding agent is run here, and no key is used. A stand-in is installed in
the guest under the names `claude` and `codex`: a shell script that records
the arguments it was given and prints lines of JSON. What is checked is
everything between a caller and a harness, not the harness.

Runs an owned hv2-sandboxd with a --machine-dir and one machine from the base
template, then checks, through its API:

1. A prompt for a harness that is not in the machine ends, saying so.
2. Each provider's command line reaches a program of that name with the
   arguments it should have, and the prompt as one argument, exactly, with
   nothing in it run by a shell.
3. Events are read as the agent prints them, from where the last read
   stopped; a line that is not JSON is kept as text; the agent's session is
   reported; its exit code is the prompt's.
4. Half a line is not an event until the rest of it arrives.
5. An interrupt reaches the agent itself, which ends as it chooses to.
6. A key given in `env` reaches the agent and is in nothing the API returns
   and nothing the node logs.
7. Prompts are listed in order, without events; a prompt is a run, and a run
   that is not a prompt is not listed as one.
8. A custom command is run as it is, with the prompt in its environment.
9. What cannot be done is refused, each with why.
10. Nothing is prompted in a stopped machine, and what was is still read.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "agent-01"
KEY = "sk-stand-in-0123456789abcdef"
# Everything a shell would act on, were it ever to see it.
TRICKY = ('fix "the" tests; $(touch /tmp/standin/pwned-1) `touch /tmp/standin/pwned-2` '
          "$HOME 'single' \\ && touch /tmp/standin/pwned-3\nand a second line")

STAND_IN = r'''#!/bin/sh
# A stand-in for a coding agent. It is not one: it records what it was given
# and prints lines of JSON, so that a check can see what reached it.
cat > /dev/null
umask 000
out=/tmp/standin/last
rm -rf $out; mkdir -p $out
n=0
for a in "$@"; do n=$((n+1)); printf '%s' "$a" > $out/arg-$n; done
echo $n > $out/argc
echo "stand-in starting: this line is not JSON"
echo "{\"type\":\"system\",\"session_id\":\"stand-in-$$\",\"name\":\"${0##*/}\",\"cwd\":\"$PWD\",\"uid\":$(id -u),\"key_length\":${#AGENT_KEY}}"
trap 'echo "{\"type\":\"result\",\"interrupted\":true}"; exit 130' INT
case "$STANDIN" in
  wait) echo '{"type":"assistant","waiting":true}'
        i=0; while [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done ;;
  half) printf '{"type":"half",'; sleep 3; echo '"done":true}' ;;
  *) for i in 1 2 3; do echo "{\"type\":\"assistant\",\"step\":$i}"; sleep 1; done ;;
esac
echo "a warning on standard error" >&2
echo "{\"type\":\"result\",\"interrupted\":false}"
exit ${STANDIN_EXIT:-0}
'''

INSTALL = r'''mkdir -p /tmp/standin && cat > /tmp/standin/agent && chmod 755 /tmp/standin/agent || exit 1
chmod 777 /tmp/standin
# A user that is not root, for the harness that will not run unattended as one.
grep -q '^dev:' /etc/passwd 2>/dev/null || echo 'dev:x:1000:1000:dev:/tmp:/bin/sh' >> /etc/passwd
for d in $(echo "$PATH" | tr : ' '); do
  if [ -d "$d" ] && [ -w "$d" ]; then
    ln -sf /tmp/standin/agent "$d/claude" && ln -sf /tmp/standin/agent "$d/codex" && break
  fi
done
command -v claude && command -v codex
'''


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
              "purpose": "functional verification of machine prompts on KVM with a stand-in "
                         "for the coding agent; no agent, no key, no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-prompts-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="debug")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon.log"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=60):
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

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 20})
        assert status == 200, (status, value)
        return value

    def prompt(**body):
        status, started = request("POST", f"/machines/{NAME}/prompts", body)
        assert status == 201, (status, started)
        assert started["state"] == "running" and started["promptID"] >= 1, started
        # What was asked, and nothing of how it is run.
        assert started["provider"] == body["provider"] and started["prompt"] == body["prompt"], started
        assert not {"cmd", "env", "runID", "command"} & set(started), started
        return started

    def read(prompt_id, after=0):
        status, value = request("GET", f"/machines/{NAME}/prompts/{prompt_id}?after={after}")
        assert status == 200, (status, value)
        return value

    def until_ended(prompt_id, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            value = read(prompt_id)
            if value["state"] != "running":
                return value
            assert time.monotonic() < deadline, f"prompt {prompt_id} still running: {value}"
            time.sleep(0.2)

    def events_of(value, kind=None):
        found = [e["event"] for e in value["events"] if "event" in e]
        return found if kind is None else [e for e in found if e["type"] == kind]

    def arguments():
        count = int(guest("cat /tmp/standin/last/argc")["stdout"])
        return [guest(f"cat /tmp/standin/last/arg-{n}")["stdout"] for n in range(1, count + 1)]

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
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
        assert status == 201 and created["state"] == "running", (status, created)
        state = {}

        def not_installed():
            started = prompt(provider="claude", prompt="hello")
            ended = until_ended(started["promptID"])
            assert (ended["state"], ended["exitCode"]) == ("exited", 127), ended
            assert ended["events"] == [] and ended["session"] is None, ended
            status, run = request("GET", f"/machines/{NAME}/runs/{started['promptID']}?stdout=0&stderr=0")
            assert status == 200 and "not found" in run["stderr"]["text"], (status, run)
            return {"exitCode": 127, "stderr": run["stderr"]["text"].strip()}
        case("a prompt for a harness that is not in the machine ends, saying so", not_installed)

        status, run = request("POST", f"/machines/{NAME}/runs", {"cmd": INSTALL, "stdin": STAND_IN})
        assert status == 201, (status, run)
        state["plain_run"] = run["runID"]
        deadline = time.monotonic() + 30
        while True:
            status, run = request("GET", f"/machines/{NAME}/runs/{state['plain_run']}?stdout=0&stderr=0")
            if run["state"] != "running":
                break
            assert time.monotonic() < deadline, run
            time.sleep(0.2)
        assert run["exitCode"] == 0 and len(run["stdout"]["text"].split()) == 2, run
        report["stand_in_installed_as"] = run["stdout"]["text"].split()

        def command_lines():
            seen = {}
            for label, body, want in [
                ("claude", dict(provider="claude"),
                 ["-p", TRICKY, "--output-format", "stream-json", "--verbose"]),
                ("claude, all of it",
                 dict(provider="claude", model="a-model", resume="a-session", unattended=True, user="dev",
                      # A caller cannot put something else where the prompt goes.
                      env={"HM_PROMPT": "not this", "HM_MODEL": "nor this"}),
                 ["-p", TRICKY, "--output-format", "stream-json", "--verbose",
                  "--dangerously-skip-permissions", "--resume", "a-session", "--model", "a-model"]),
                ("codex", dict(provider="codex"),
                 ["exec", "--json", "--skip-git-repo-check", TRICKY]),
                ("codex, all of it", dict(provider="codex", model="a-model", unattended=True),
                 ["exec", "--json", "--skip-git-repo-check",
                  "--dangerously-bypass-approvals-and-sandbox", "-m", "a-model", TRICKY]),
            ]:
                started = prompt(prompt=TRICKY, **body)
                ended = until_ended(started["promptID"])
                assert ended["exitCode"] == 0, (label, ended)
                system = events_of(ended, "system")[0]
                assert system["name"] == body["provider"], (label, ended)
                assert system["uid"] == (1000 if body.get("user") else 0), (label, system)
                got = arguments()
                assert got == want, (label, got)
                seen[label] = len(got)
            # Nothing in the prompt was run.
            listing = guest("ls /tmp/standin")["stdout"].split()
            assert sorted(listing) == ["agent", "last"], listing
            return {"arguments": seen, "prompt_bytes": len(TRICKY.encode())}
        case("each provider's command line reaches its program, the prompt one argument and never shell",
             command_lines)

        def as_they_come():
            started = prompt(provider="claude", prompt="take three steps", env={"STANDIN_EXIT": "7"})
            prompt_id = started["promptID"]
            gathered, after, reads_with_events, reads = [], 0, 0, 0
            deadline = time.monotonic() + 30
            while True:
                value = read(prompt_id, after)
                reads += 1
                assert all(e["offset"] >= after for e in value["events"]), (after, value)
                assert value["next"] >= after, value
                if value["events"]:
                    reads_with_events += 1
                    gathered += value["events"]
                after = value["next"]
                if value["state"] != "running":
                    break
                assert time.monotonic() < deadline, value
                time.sleep(0.3)
            offsets = [e["offset"] for e in gathered]
            assert offsets == sorted(set(offsets)), offsets
            assert gathered[0] == {"offset": 0, "text": "stand-in starting: this line is not JSON"}, gathered[0]
            kinds = [e["event"]["type"] for e in gathered[1:]]
            assert kinds == ["system", "assistant", "assistant", "assistant", "result"], kinds
            assert [e["event"]["step"] for e in gathered[2:5]] == [1, 2, 3], gathered
            # As they were printed, not all at the end.
            assert reads_with_events >= 2, (reads_with_events, reads)
            assert (value["state"], value["exitCode"]) == ("exited", 7), value
            # The harness is the run's own process: its session carries its PID.
            whole = read(prompt_id)
            assert whole["events"] == gathered, whole
            assert whole["session"] == f"stand-in-{started['pid']}", (whole["session"], started)
            assert read(prompt_id, after)["events"] == [], "nothing after the end"
            # Standard error is the run's, and is not an event.
            assert "standard error" not in json.dumps(whole["events"]), whole
            status, run = request("GET", f"/machines/{NAME}/runs/{prompt_id}?stdout=0&stderr=0")
            assert run["stderr"]["text"] == "a warning on standard error\n", run
            state["ended"] = prompt_id
            return {"promptID": prompt_id, "events": len(gathered), "reads": reads,
                    "reads_with_events": reads_with_events, "session": whole["session"]}
        case("events are read as the agent prints them, from where the last read stopped", as_they_come)

        def half_a_line():
            started = prompt(provider="codex", prompt="print half a line", env={"STANDIN": "half"})
            seen_waiting = 0
            deadline = time.monotonic() + 30
            while True:
                value = read(started["promptID"])
                if value["state"] != "running":
                    break
                # Never as text, which is what a line cut short would be; as
                # an event only once it is whole.
                for entry in value["events"]:
                    assert "half" not in entry.get("text", ""), entry
                    assert entry.get("event", {}).get("type") != "half" or entry["event"]["done"], entry
                if events_of(value, "system"):
                    seen_waiting += 1
                assert time.monotonic() < deadline, value
                time.sleep(0.3)
            # It was read while the half was written and the rest was not.
            assert seen_waiting >= 3, seen_waiting
            assert events_of(value, "half") == [{"type": "half", "done": True}], value
            return {"reads_while_half_written": seen_waiting}
        case("half a line is not an event until the rest of it arrives", half_a_line)

        def interrupted():
            started = prompt(provider="claude", prompt="wait to be interrupted", env={"STANDIN": "wait"})
            prompt_id = started["promptID"]
            deadline = time.monotonic() + 20
            while not events_of(read(prompt_id), "assistant"):
                assert time.monotonic() < deadline
                time.sleep(0.2)
            status, value = request("POST", f"/machines/{NAME}/prompts/{prompt_id}/interrupt")
            assert status == 204, (status, value)
            ended = until_ended(prompt_id, 20)
            # Its own handler ran: it said so and chose its exit code. A shell
            # killed around it would have ended by the signal with no word.
            assert (ended["state"], ended["exitCode"], ended["signal"]) == ("exited", 130, None), ended
            assert events_of(ended)[-1] == {"type": "result", "interrupted": True}, ended
            status, value = request("POST", f"/machines/{NAME}/prompts/{prompt_id}/interrupt")
            assert status == 409, (status, value)
            return {"promptID": prompt_id, "exitCode": 130}
        case("an interrupt reaches the agent itself, which ends as it chooses to", interrupted)

        def key():
            started = prompt(provider="claude", prompt="use the key", cwd="/tmp", env={"AGENT_KEY": KEY})
            ended = until_ended(started["promptID"])
            system = events_of(ended, "system")[0]
            assert system["key_length"] == len(KEY) and system["cwd"] == "/tmp", system
            returned = [started, ended,
                        request("GET", f"/machines/{NAME}/prompts")[1],
                        request("GET", f"/machines/{NAME}/runs")[1],
                        request("GET", f"/machines/{NAME}/runs/{started['promptID']}?stdout=0&stderr=0")[1]]
            assert KEY not in json.dumps(returned), "the key was returned"
            log.flush()
            logged = log_path.read_bytes()
            assert KEY.encode() not in logged, "the key was logged"
            # The prompt is the caller's and is returned to them; it is not
            # written to the node's log either.
            assert b"wait to be interrupted" not in logged and b"pwned" not in logged, "a prompt was logged"
            state["keyed"] = started["promptID"]
            return {"key_length_seen_by_agent": system["key_length"], "daemon_log_bytes": len(logged)}
        case("a key in env reaches the agent and is in nothing returned or logged", key)

        def listed():
            status, prompts = request("GET", f"/machines/{NAME}/prompts")
            assert status == 200, (status, prompts)
            ids = [p["promptID"] for p in prompts]
            assert ids == sorted(ids) and len(ids) == 9, ids
            assert all(not {"events", "cmd", "env"} & set(p) for p in prompts), prompts
            assert [p["provider"] for p in prompts].count("codex") == 3, prompts
            # A run that is not a prompt is not one.
            assert state["plain_run"] not in ids, ids
            status, value = request("GET", f"/machines/{NAME}/prompts/{state['plain_run']}")
            assert status == 404, (status, value)
            status, value = request("POST", f"/machines/{NAME}/prompts/{state['plain_run']}/interrupt")
            assert status == 404, (status, value)
            # A prompt is a run: its command line names the prompt's variable
            # and does not hold the prompt.
            status, run = request("GET", f"/machines/{NAME}/runs/{state['keyed']}")
            assert status == 200 and '"$HM_PROMPT"' in run["cmd"] and "use the key" not in run["cmd"], run
            return {"prompts": len(ids), "run_cmd": run["cmd"]}
        case("prompts are listed in order without events; a prompt is a run, a run is not a prompt", listed)

        def custom():
            line = 'echo "{\\"type\\":\\"echo\\",\\"length\\":${#HM_PROMPT},\\"model\\":\\"$HM_MODEL\\"}"; exit 5'
            started = prompt(provider="custom", command=line, prompt="twelve chars", model="any-model")
            ended = until_ended(started["promptID"])
            assert events_of(ended) == [{"type": "echo", "length": 12, "model": "any-model"}], ended
            assert ended["exitCode"] == 5, ended
            return {"event": events_of(ended)[0]}
        case("a custom command is run as it is, with the prompt in its environment", custom)

        def refusals():
            results = {}
            for label, method, path, body, want in [
                ("empty prompt", "POST", "/prompts", {"provider": "claude", "prompt": "  "}, 400),
                ("a prompt too long", "POST", "/prompts", {"provider": "claude", "prompt": "x" * (129 << 10)}, 400),
                ("no such provider", "POST", "/prompts", {"provider": "hal", "prompt": "open"}, 400),
                ("custom without a command", "POST", "/prompts", {"provider": "custom", "prompt": "x"}, 400),
                ("a command for a provider that has one", "POST", "/prompts",
                 {"provider": "claude", "prompt": "x", "command": "true"}, 400),
                ("resume for codex", "POST", "/prompts", {"provider": "codex", "prompt": "x", "resume": "s"}, 400),
                ("claude unattended as root", "POST", "/prompts",
                 {"provider": "claude", "prompt": "x", "unattended": True}, 400),
                ("an empty model", "POST", "/prompts", {"provider": "claude", "prompt": "x", "model": ""}, 400),
                ("no such prompt", "GET", "/prompts/99999", None, 404),
                ("interrupting one that has ended", "POST", f"/prompts/{state['ended']}/interrupt", None, 409),
                ("no such machine", "POST", None, {"provider": "claude", "prompt": "x"}, 404),
            ]:
                target = "/machines/absent-99/prompts" if path is None else f"/machines/{NAME}{path}"
                status, value = request(method, target, body)
                assert status == want, (label, status, value)
                results[label] = value["message"]
            # None of them started anything.
            assert len(request("GET", f"/machines/{NAME}/prompts")[1]) == 10
            return results
        case("what cannot be done is refused, each with why", refusals)

        def stopped():
            status, value = request("POST", f"/machines/{NAME}/stop", timeout=120)
            assert status == 200 and value["state"] == "stopped", (status, value)
            status, value = request("POST", f"/machines/{NAME}/prompts", {"provider": "claude", "prompt": "x"})
            assert status == 409, (status, value)
            kept = read(state["ended"])
            assert kept["exitCode"] == 7 and len(kept["events"]) == 6, kept
            return {"refused": value["message"]}
        case("nothing is prompted in a stopped machine, and what was is still read", stopped)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
