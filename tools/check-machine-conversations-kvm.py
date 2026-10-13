#!/usr/bin/env python3
"""Conversations on real KVM: turns that carry a session on, steering, and a
move from one agent to another.

Two parts. The first runs everywhere this runs: a machine from the template
with a stand-in installed as `claude` and `codex`, shell scripts that read
and print what each harness does, and take as long as they are told to. That
makes a turn slow enough to steer, every time, and lets the check see what
reached the agent. It checks:

1. A conversation is made with nothing running; what cannot be made is
   refused, each with why.
2. A message to it starts a turn. A second, sent while the turn works, goes
   into the same turn: the same process, the same session. The turn is
   answered twice, and its input is closed only after both answers.
3. A message after the turn ended starts a new one, resuming the session.
4. The transcript is both sides, in order, read from the agent's events.
5. An interrupt ends the running turn; there is then none to interrupt.
6. Codex is a turn at a time: a message while one runs is refused, and the
   next turn resumes its thread.
7. A conversation made `from` another, with the other provider, gives the
   new agent what was said before its first message, both ways.
8. Conversations are listed, found by number or name, and deleted only
   when nothing runs.

With `--image`, `--firmware` and `--guest-agent` a second machine is made
from that distribution image with both agents installed at its first boot,
and the same is done with the real Claude Code and Codex. No key is given:
each answers every turn by saying it has none. That shows the harnesses take
what a conversation sends, and keep one session across its turns; it does
not show what a model does with a message that steers it.

Writes report.json and the node's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

NAME = "talk-01"
REAL = "talk-real"

CLAUDE = r'''#!/bin/sh
# A stand-in for Claude Code reading messages as lines of JSON. It is not
# one: it says each message back, works for $STANDIN_WORK seconds, answers,
# and reads the next, until its input is closed.
mkdir -p /tmp/standin/claude
log=/tmp/standin/claude/$$
printf '%s\n' "$@" > $log.args
case " $* " in *" --input-format stream-json "*) ;; *) echo "not streamed" >&2; exit 2;; esac
if [ -n "$STANDIN_FAIL" ]; then
  # An agent that gives up before reading anything.
  exec 0<&-
  echo "stand-in giving up at once" >&2
  sleep 1
  exit 3
fi
sid="standin-$$"
prev=""
for a in "$@"; do [ "$prev" = "--resume" ] && sid="$a"; prev="$a"; done
echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$sid\"}"
n=0
while IFS= read -r line; do
  n=$((n+1))
  printf '%s\n' "$line" > $log.in-$n
  # The text, whatever order the fields come in; these messages hold no quotes.
  text=$(printf '%s' "$line" | sed -e 's/.*"text":"//' -e 's/"[,}].*//')
  echo "{\"type\":\"user\",\"isReplay\":true,\"session_id\":\"$sid\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"$text\"}]}}"
  i=0; while [ $i -lt $((${STANDIN_WORK:-3}*10)) ]; do sleep 0.1; i=$((i+1)); done
  echo "{\"type\":\"assistant\",\"session_id\":\"$sid\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"answer $n\"}]}}"
  echo "{\"type\":\"result\",\"session_id\":\"$sid\",\"result\":\"answer $n\"}"
done
echo "{\"type\":\"standin\",\"input\":\"closed\",\"messages\":$n}"
'''

CODEX = r'''#!/bin/sh
# A stand-in for `codex exec`: a turn at a time, the prompt its last argument.
mkdir -p /tmp/standin/codex
log=/tmp/standin/codex/$$
printf '%s\n' "$@" > $log.args
printf '%s' "$HM_PROMPT" > $log.prompt
tid="standin-thread-$$"
prev=""
for a in "$@"; do [ "$prev" = "resume" ] && tid="$a"; prev="$a"; done
echo "{\"type\":\"thread.started\",\"thread_id\":\"$tid\"}"
echo '{"type":"turn.started"}'
i=0; while [ $i -lt $((${STANDIN_WORK:-3}*10)) ]; do sleep 0.1; i=$((i+1)); done
echo "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"codex heard ${#HM_PROMPT} bytes\"}}"
echo '{"type":"turn.completed"}'
'''

INSTALL = r'''cd /tmp/standin && chmod 755 claude.sh codex.sh || exit 1
for d in $(echo "$PATH" | tr : ' '); do
  if [ -d "$d" ] && [ -w "$d" ]; then
    ln -sf /tmp/standin/claude.sh "$d/claude" && ln -sf /tmp/standin/codex.sh "$d/codex" && break
  fi
done
command -v claude && command -v codex
'''


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
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ["image", "firmware", "guest-agent"]:
        parser.add_argument("--" + name, type=Path)
    args = parser.parse_args()
    real = args.image is not None
    assert not real or (args.firmware and args.guest_agent), "--image needs --firmware and --guest-agent"
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = ["daemon", "kernel", "initrd"] + (["image", "firmware", "guest_agent"] if real else [])
    report = {"success": False,
              "purpose": "functional verification of machine conversations on KVM: a stand-in for each "
                         "agent, and the real agents with no key when an image is given; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in inputs},
              "real_agents": real,
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-conversations-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    if real:
        subprocess.run(["cp", "--sparse=always", str(args.image), str(images / "distro.raw")], check=True)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"]
        + (["--firmware", str(args.firmware), "--guest-agent", str(args.guest_agent)] if real else []),
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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

    def at(machine, path=""):
        return f"/machines/{machine}/conversations{path}"

    def make(machine, **body):
        status, made = request("POST", at(machine), body)
        assert status == 201, (status, made)
        return made

    def say(machine, conversation, prompt, want=201, **more):
        status, value = request("POST", at(machine, f"/{conversation}/messages"), dict(prompt=prompt, **more))
        if status != want and isinstance(value, dict) and "started" in value.get("message", ""):
            # A turn that started and failed says why on its standard error.
            run_id = value["message"].split()[1]
            ran = request("GET", f"/machines/{machine}/runs/{run_id}")[1]
            raise AssertionError((status, value, ran))
        assert status == want, (status, value)
        return value

    def describe(machine, conversation):
        status, value = request("GET", at(machine, f"/{conversation}"))
        assert status == 200, (status, value)
        return value

    def until_idle(machine, conversation, seconds=120):
        deadline = time.monotonic() + seconds
        while True:
            value = describe(machine, conversation)
            if value["state"] == "idle":
                return value
            assert time.monotonic() < deadline, f"conversation still running: {value}"
            time.sleep(0.3)

    def turn(machine, prompt_id):
        status, value = request("GET", f"/machines/{machine}/prompts/{prompt_id}")
        assert status == 200, (status, value)
        return value

    def events(value, kind=None):
        found = [e["event"] for e in value["events"] if "event" in e]
        return found if kind is None else [e for e in found if e.get("type") == kind]

    def guest(machine, cmd):
        status, value = request("POST", f"/machines/{machine}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value

    def put(machine, path, text):
        """Write `text` to `path` in the guest, through a run's standard input."""
        status, run = request("POST", f"/machines/{machine}/runs",
                              {"cmd": f"mkdir -p $(dirname {path}) && cat > {path}", "stdin": text})
        assert status == 201, (status, run)
        deadline = time.monotonic() + 30
        while True:
            status, run = request("GET", f"/machines/{machine}/runs/{run['runID']}?stdout=0&stderr=0")
            if run["state"] != "running":
                break
            assert time.monotonic() < deadline, run
            time.sleep(0.2)
        assert run["exitCode"] == 0, run

    def wait_ready(seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)

    try:
        wait_ready()
        status, created = request("POST", "/machines", {"name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1},
                                  timeout=180)
        assert status == 201 and created["state"] == "running", (status, created)
        put(NAME, "/tmp/standin/claude.sh", CLAUDE)
        put(NAME, "/tmp/standin/codex.sh", CODEX)
        installed = guest(NAME, INSTALL)
        assert installed["exit_code"] == 0, installed
        report["stand_in_installed_as"] = installed["stdout"].split()
        state = {}

        def made():
            fix = make(NAME, provider="claude", name="fix")
            assert fix["state"] == "idle" and fix["turns"] == [] and fix["messages"] == [], fix
            assert fix["session"] is None and fix["conversationID"] >= 1, fix
            state["fix"] = fix["conversationID"]
            refused = {}
            for label, body, want in [
                ("a custom provider", {"provider": "custom"}, 400),
                ("no such provider", {"provider": "gpt"}, 400),
                ("a name already taken", {"provider": "codex", "name": "fix"}, 409),
                ("a name that is a number", {"provider": "codex", "name": "12"}, 400),
                ("a name with a slash", {"provider": "codex", "name": "a/b"}, 400),
                ("claude unattended as root", {"provider": "claude", "unattended": True}, 400),
                ("from a conversation that is not there", {"provider": "codex", "from": "absent"}, 404),
                ("an empty model", {"provider": "claude", "model": " "}, 400),
            ]:
                status, value = request("POST", at(NAME), body)
                assert status == want, (label, status, value)
                refused[label] = value["message"]
            status, value = request("POST", at("absent-99"), {"provider": "claude"})
            assert status == 404, (status, value)
            assert len(request("GET", at(NAME))[1]) == 1
            return {"refused": refused}
        case("a conversation is made with nothing running; what cannot be is refused, with why", made)

        def steered():
            started = time.monotonic()
            first = say(NAME, "fix", "first message", env={"STANDIN_WORK": "3"})
            assert first["steered"] is False, first
            time.sleep(1)
            second = say(NAME, "fix", "second message", want=202)
            assert second == {"conversationID": state["fix"], "promptID": first["promptID"], "steered": True}, second
            running = describe(NAME, "fix")
            assert running["state"] == "running" and running["runningPromptID"] == first["promptID"], running
            ended = until_idle(NAME, "fix")
            took = round(time.monotonic() - started, 1)
            value = turn(NAME, first["promptID"])
            assert (value["state"], value["exitCode"]) == ("exited", 0), value
            kinds = [e["type"] for e in events(value)]
            assert kinds == ["system", "user", "assistant", "result", "user", "assistant", "result", "standin"], kinds
            # Its input was closed after the second answer, not before.
            assert events(value, "standin")[0]["messages"] == 2, value
            sessions = {e["session_id"] for e in events(value) if "session_id" in e}
            assert len(sessions) == 1, sessions
            assert ended["turns"] == [first["promptID"]], ended
            assert [(m["prompt"], m["steered"]) for m in ended["messages"]] == [
                ("first message", False), ("second message", True)], ended
            assert ended["session"] == sessions.pop(), ended
            # One process: one argument list, and both messages in it.
            pids = guest(NAME, "ls /tmp/standin/claude | grep -c args")["stdout"].strip()
            assert pids == "1", pids
            state["turn1"], state["session"] = first["promptID"], ended["session"]
            return {"promptID": first["promptID"], "events": kinds, "seconds_for_both": took,
                    "session": ended["session"]}
        case("a message while a turn works goes into it: one process, one session, answered twice", steered)

        def resumed():
            third = say(NAME, "fix", "third message", env={"STANDIN_WORK": "1"})
            assert third["steered"] is False and third["promptID"] != state["turn1"], third
            ended = until_idle(NAME, "fix")
            assert ended["turns"] == [state["turn1"], third["promptID"]], ended
            assert ended["session"] == state["session"], ended
            value = turn(NAME, third["promptID"])
            assert value["resume"] == state["session"], value
            args = guest(NAME, "cat $(ls -t /tmp/standin/claude/*.args | head -1)")["stdout"].split("\n")
            assert "--resume" in args and args[args.index("--resume") + 1] == state["session"], args
            assert "--replay-user-messages" in args and "-p" in args, args
            state["turn2"] = third["promptID"]
            return {"promptID": third["promptID"], "arguments": [a for a in args if a]}
        case("a message after the turn ended starts a new one, resuming the session", resumed)

        def transcript():
            status, lines = request("GET", at(NAME, "/fix/transcript"))
            assert status == 200, (status, lines)
            got = [(line["role"], line["text"], line["promptID"]) for line in lines]
            t1, t2 = state["turn1"], state["turn2"]
            assert got == [("user", "first message", t1), ("assistant", "answer 1", t1),
                           ("user", "second message", t1), ("assistant", "answer 2", t1),
                           ("user", "third message", t2), ("assistant", "answer 1", t2)], got
            return {"lines": len(lines)}
        case("the transcript is both sides, in order, from the agent's own events", transcript)

        def interrupted():
            busy = say(NAME, state["fix"], "take a long time", env={"STANDIN_WORK": "60"})
            time.sleep(2)
            status, value = request("POST", at(NAME, f"/{state['fix']}/interrupt"))
            assert status == 204, (status, value)
            ended = until_idle(NAME, "fix", 20)
            value = turn(NAME, busy["promptID"])
            assert value["state"] == "exited" and (value["signal"] or value["exitCode"]), value
            status, value = request("POST", at(NAME, "/fix/interrupt"))
            assert status == 409, (status, value)
            return {"promptID": busy["promptID"], "turns": len(ended["turns"])}
        case("an interrupt ends the running turn, and then there is none to interrupt", interrupted)

        def codex():
            made = make(NAME, provider="codex", name="cx", model="a-model")
            first = say(NAME, "cx", "codex one", env={"STANDIN_WORK": "3"})
            status, value = request("POST", at(NAME, "/cx/messages"), {"prompt": "too soon"})
            assert status == 409 and "codex" in value["message"], (status, value)
            ended = until_idle(NAME, "cx")
            thread = ended["session"]
            assert thread and thread.startswith("standin-thread-"), ended
            second = say(NAME, "cx", "codex two", env={"STANDIN_WORK": "1"})
            ended = until_idle(NAME, "cx")
            assert ended["turns"] == [first["promptID"], second["promptID"]], ended
            assert ended["session"] == thread, ended
            assert [m["prompt"] for m in ended["messages"]] == ["codex one", "codex two"], ended
            args = guest(NAME, "cat $(ls -t /tmp/standin/codex/*.args | head -1)")["stdout"].split("\n")
            assert args[:3] == ["exec", "--json", "--skip-git-repo-check"], args
            assert args[3:7] == ["-m", "a-model", "resume", thread] and args[7] == "codex two", args
            status, lines = request("GET", at(NAME, "/cx/transcript"))
            assert [line["role"] for line in lines] == ["user", "assistant", "user", "assistant"], lines
            state["cx"] = made["conversationID"]
            return {"thread": thread, "arguments": [a for a in args if a], "refused": value["message"]}
        case("codex is a turn at a time: refused while one runs, and the next resumes its thread", codex)

        def moved():
            # From Claude Code's conversation to Codex.
            onward = make(NAME, provider="codex", name="onward", **{"from": "fix"})
            assert onward["from"] == state["fix"], onward
            say(NAME, "onward", "carry on please", env={"STANDIN_WORK": "1"})
            until_idle(NAME, "onward")
            told = guest(NAME, "cat $(ls -t /tmp/standin/codex/*.prompt | head -1)")["stdout"]
            assert "another agent (claude)" in told, told
            assert "User: first message\n\nAssistant: answer 1\n\nUser: second message" in told, told
            assert told.endswith("The user's next message follows.\n\ncarry on please"), told[-200:]
            # What is carried is not the caller's message, as recorded.
            onward_now = describe(NAME, "onward")
            assert onward_now["messages"][0]["prompt"] == "carry on please", onward_now
            # And back, from Codex's to Claude Code's.
            back = make(NAME, provider="claude", **{"from": str(state["cx"])})
            said = say(NAME, back["conversationID"], "and back", env={"STANDIN_WORK": "1"})
            until_idle(NAME, back["conversationID"])
            replayed = events(turn(NAME, said["promptID"]), "user")[0]["message"]["content"][0]["text"]
            assert "another agent (codex)" in replayed and "Assistant: codex heard" in replayed, replayed
            assert replayed.endswith("and back"), replayed[-100:]
            return {"carried_to_codex_bytes": len(told), "carried_to_claude_bytes": len(replayed)}
        case("a conversation made from another gives the new agent what was said, both ways", moved)

        def listed():
            status, all_ = request("GET", at(NAME))
            assert status == 200 and len(all_) == 4, all_
            assert all("messages" not in c for c in all_), all_
            ids = [c["conversationID"] for c in all_]
            assert ids == sorted(ids), ids
            by_number = describe(NAME, str(state["cx"]))
            assert by_number["name"] == "cx" and describe(NAME, "cx")["conversationID"] == state["cx"]
            busy = say(NAME, "cx", "keep busy", env={"STANDIN_WORK": "30"})
            status, value = request("DELETE", at(NAME, "/cx"))
            assert status == 409, (status, value)
            request("POST", at(NAME, "/cx/interrupt"))
            until_idle(NAME, "cx", 20)
            status, _ = request("DELETE", at(NAME, "/cx"))
            assert status == 204, status
            assert request("GET", at(NAME, "/cx"))[0] == 404
            # Its turns are still prompts of the machine.
            assert turn(NAME, busy["promptID"])["provider"] == "codex"
            return {"conversations": len(all_)}
        case("conversations are listed, found by number or name, and deleted only when idle", listed)

        def gave_up():
            make(NAME, provider="claude", name="quits")
            status, value = request("POST", at(NAME, "/quits/messages"),
                                    {"prompt": "hello", "env": {"STANDIN_FAIL": "1"}})
            if status == 502:
                # The agent closed its input before the message reached it:
                # the reply says what it said.
                assert "stand-in giving up at once" in value["message"], value
                assert "exit code 3" in value["message"], value
                branch, said = "refused, with the agent's words", value["message"]
            else:
                # The message reached the pipe first: the turn says it.
                assert status == 201, (status, value)
                until_idle(NAME, "quits", 20)
                ran = request("GET", f"/machines/{NAME}/runs/{value['promptID']}")[1]
                assert ran["exitCode"] == 3 and "giving up" in ran["stderr"]["text"], ran
                branch, said = "taken, and the turn ended saying why", ran["stderr"]["text"]
            assert describe(NAME, "quits")["state"] == "idle"
            return {"branch": branch, "said": said}
        case("an agent that ends before its first message is written says why", gave_up)

        if real:
            def real_machine():
                status, made = request("POST", "/machines", {
                    "name": REAL, "image": "distro.raw", "cpuCount": 2, "memoryMB": 2048, "diskGiB": 8,
                    "network": {"allowInternetAccess": True}, "guestAgent": True,
                    "userData": "#cloud-config\nusers: [default]\n", "agents": ["claude", "codex"]})
                assert status == 201, (status, made)
                deadline = time.monotonic() + 1500
                while True:
                    status, value = request("POST", f"/machines/{REAL}/exec",
                                            {"cmd": "test -e /var/lib/hv2/agents-ready", "timeout_secs": 10})
                    if status == 200 and value["exit_code"] == 0:
                        break
                    assert time.monotonic() < deadline, "the agents were not installed"
                    time.sleep(10)
                versions = guest(REAL, "su - ubuntu -c 'claude --version; codex --version'")["stdout"].split("\n")
                return {"versions": [v for v in versions if v]}
            case("real agents: a machine from the image with both installed", real_machine)

            def real_claude():
                make(REAL, provider="claude", name="real", user="ubuntu", unattended=True, cwd="/home/ubuntu")
                first = say(REAL, "real", "Say hello.")
                second = request("POST", at(REAL, "/real/messages"), {"prompt": "And goodbye."})
                assert second[0] in (201, 202), second
                until_idle(REAL, "real")
                third = say(REAL, "real", "Once more.")
                ended = until_idle(REAL, "real")
                sessions, results = set(), 0
                for prompt_id in ended["turns"]:
                    value = turn(REAL, prompt_id)
                    assert value["state"] == "exited", value
                    sessions |= {e["session_id"] for e in events(value) if e.get("session_id")}
                    results += len(events(value, "result"))
                # Every message answered, all in one session.
                assert results == 3 and len(sessions) == 1, (results, sessions)
                assert turn(REAL, third["promptID"])["resume"] == ended["session"], ended
                status, lines = request("GET", at(REAL, "/real/transcript"))
                users = [line["text"] for line in lines if line["role"] == "user"]
                assert users == ["Say hello.", "And goodbye.", "Once more."], lines
                return {"second_message_steered": second[1]["steered"], "turns": len(ended["turns"]),
                        "session": ended["session"],
                        "answers": [line["text"] for line in lines if line["role"] == "assistant"]}
            case("real Claude Code: three messages answered in one session, the second into a turn if one ran",
                 real_claude)

            def real_codex():
                make(REAL, provider="codex", name="real-cx", user="ubuntu", cwd="/home/ubuntu")
                first = say(REAL, "real-cx", "Say hello.")
                until_idle(REAL, "real-cx", 180)
                second = say(REAL, "real-cx", "And goodbye.")
                ended = until_idle(REAL, "real-cx", 180)
                threads = [events(turn(REAL, p), "thread.started")[0]["thread_id"] for p in ended["turns"]]
                assert len(threads) == 2 and threads[0] == threads[1] == ended["session"], (threads, ended)
                return {"thread": threads[0], "turns": [first["promptID"], second["promptID"]]}
            case("real Codex: the second turn resumes the first's thread", real_codex)

            def real_moved():
                make(REAL, provider="codex", name="real-moved", user="ubuntu", cwd="/home/ubuntu",
                     **{"from": "real"})
                say(REAL, "real-moved", "What did we say?")
                ended = until_idle(REAL, "real-moved", 180)
                assert events(turn(REAL, ended["turns"][0]), "thread.started"), ended
                found = guest(REAL, "grep -rl 'another agent (claude)' /home/ubuntu/.codex /root/.codex 2>/dev/null | wc -l")
                assert int(found["stdout"].strip()) >= 1, found
                return {"codex_session_files_with_the_carried_transcript": int(found["stdout"].strip())}
            case("real agents: Codex is given what was said to Claude Code", real_moved)

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
