"""The client against an HTTP fixture that answers as a node does.

    python3 -m unittest discover -s sdk/python/tests
"""
import base64
import http.server
import json
import os
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from hypermachine import ApiError, Client  # noqa: E402

MACHINE = {"machineID": "vm-1", "name": "web-01", "state": "running"}


class Node(http.server.BaseHTTPRequestHandler):
    """Records each request and answers from `ANSWERS` by method and path."""

    seen = []
    answers = {}

    def log_message(self, *args):
        pass

    def _answer(self):
        length = int(self.headers.get("content-length") or 0)
        body = self.rfile.read(length) if length else b""
        Node.seen.append((self.command, self.path, {k.lower(): v for k, v in self.headers.items()}, body))
        status, payload = Node.answers.get((self.command, self.path), (404, {"code": 404, "message": "no route"}))
        if callable(payload):
            payload = payload()
        data = payload if isinstance(payload, bytes) else json.dumps(payload).encode() if payload is not None else b""
        self.send_response(status)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = _answer


class ClientTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Node)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.endpoint = f"http://127.0.0.1:{cls.server.server_address[1]}"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def setUp(self):
        Node.seen.clear()
        Node.answers.clear()
        self.hm = Client(self.endpoint, api_key="k-secret")

    def last(self):
        method, path, headers, body = Node.seen[-1]
        return method, path, headers, json.loads(body) if body[:1] in (b"{", b"[") else body

    def test_the_key_is_sent_as_a_header_and_shown_nowhere(self):
        Node.answers[("GET", "/machines")] = (200, [MACHINE])
        [machine] = self.hm.machines.list()
        self.assertEqual(machine.name, "web-01")
        self.assertEqual(Node.seen[-1][2].get("x-api-key"), "k-secret")
        self.assertNotIn("k-secret", repr(self.hm))

    def test_a_refusal_raises_with_the_node_s_words(self):
        Node.answers[("POST", "/machines")] = (409, {"code": 409, "message": "machine web-01 exists"})
        with self.assertRaises(ApiError) as caught:
            self.hm.machines.create("web-01")
        self.assertEqual(caught.exception.status, 409)
        self.assertEqual(caught.exception.message, "machine web-01 exists")

    def test_create_sends_only_what_was_given(self):
        Node.answers[("POST", "/machines")] = (201, MACHINE)
        self.hm.machines.create("web-01", cpu_count=2, disk_gib=20, start=False)
        self.assertEqual(self.last()[3], {"name": "web-01", "start": False, "cpuCount": 2, "diskGiB": 20})
        self.hm.machines.create("ai-01", image="noble.raw", guest_agent=True, agents=("claude", "codex"))
        self.assertEqual(self.last()[3], {"name": "ai-01", "start": True, "image": "noble.raw",
                                          "guestAgent": True, "agents": ["claude", "codex"]})

    def test_a_name_cannot_step_into_another_route(self):
        Node.answers[("GET", "/machines/a%2F..%2Fb")] = (200, dict(MACHINE, name="a/../b"))
        machine = self.hm.machines.get("a/../b")
        self.assertEqual(Node.seen[-1][1], "/machines/a%2F..%2Fb")
        Node.answers[("POST", "/machines/a%2F..%2Fb/stop")] = (200, dict(MACHINE, name="a/../b", state="stopped"))
        self.assertEqual(machine.stop().state, "stopped")

    def test_exec_and_resize(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("POST", "/machines/web-01/exec")] = (
            200, {"exit_code": 3, "signal": None, "stdout": "out\n", "stderr": "err\n"})
        result = machine.exec("false", timeout=5)
        self.assertEqual((result.exit_code, result.stdout, result.stderr), (3, "out\n", "err\n"))
        self.assertEqual(self.last()[3], {"cmd": "false", "timeout_secs": 5})
        Node.answers[("PATCH", "/machines/web-01")] = (200, dict(MACHINE, cpuCount=4))
        machine.resize(cpu_count=4)
        self.assertEqual(self.last()[3], {"cpuCount": 4})

    def test_a_run_is_followed_from_where_the_last_read_stopped(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("POST", "/machines/web-01/runs")] = (201, {"runID": 7, "state": "running"})
        run = machine.runs.start("make", cwd="/srv", env={"CI": "1"})
        self.assertEqual(self.last()[3], {"cmd": "make", "cwd": "/srv", "env": {"CI": "1"}})
        Node.answers[("GET", "/machines/web-01/runs/7?stdout=0&stderr=0")] = (
            200, {"runID": 7, "state": "running", "stdout": {"text": "a", "next": 1}})
        Node.answers[("GET", "/machines/web-01/runs/7?stdout=1&stderr=0")] = (
            200, {"runID": 7, "state": "exited", "exitCode": 0, "stdout": {"text": "bc", "next": 3}})
        self.assertEqual("".join(run.follow(interval=0)), "abc")
        self.assertEqual(run.exit_code, 0)

    def test_prompt_events_are_read_on_from_next(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("POST", "/machines/web-01/prompts")] = (201, {"promptID": 2, "state": "running"})
        prompt = machine.prompts.start("claude", "fix it", user="dev", unattended=True, env={"K": "v"})
        self.assertEqual(self.last()[3], {"provider": "claude", "prompt": "fix it", "user": "dev",
                                          "env": {"K": "v"}, "unattended": True})
        Node.answers[("GET", "/machines/web-01/prompts/2?after=0")] = (
            200, {"state": "running", "next": 5, "events": [{"offset": 0, "event": {"type": "a"}}]})
        Node.answers[("GET", "/machines/web-01/prompts/2?after=5")] = (
            200, {"state": "exited", "next": 9, "events": [{"offset": 5, "text": "plain"}]})
        self.assertEqual(list(prompt.events(interval=0)),
                         [{"offset": 0, "event": {"type": "a"}}, {"offset": 5, "text": "plain"}])

    def test_a_conversation_is_spoken_to_and_its_turns_are_prompts(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("POST", "/machines/web-01/conversations")] = (
            201, {"conversationID": 3, "name": "fix", "state": "idle"})
        talk = machine.conversations.create("codex", name="fix", user="dev", from_=1)
        self.assertEqual(self.last()[3], {"provider": "codex", "name": "fix", "user": "dev", "from": "1"})
        Node.answers[("POST", "/machines/web-01/conversations/3/messages")] = (
            202, {"conversationID": 3, "promptID": 8, "steered": True})
        turn = talk.say("and the docs", env={"K": "v"})
        self.assertEqual(self.last()[3], {"prompt": "and the docs", "env": {"K": "v"}})
        self.assertEqual((turn.id, turn.info["steered"]), (8, True))
        self.assertEqual(turn._path, "/machines/web-01/prompts/8")
        Node.answers[("GET", "/machines/web-01/conversations/3/transcript")] = (
            200, [{"role": "user", "text": "hi", "promptID": 8}])
        self.assertEqual(talk.transcript()[0]["role"], "user")
        Node.answers[("GET", "/machines/web-01/conversations/a%2Fb")] = (
            200, {"conversationID": 4, "name": "a/b"})
        self.assertEqual(machine.conversations.get("a/b").id, 4)

    def test_the_desktop_is_pictured_and_acted_on(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("GET", "/machines/web-01/desktop/screenshot")] = (200, b"\x89PNG\r\n\x1a\n...")
        self.assertEqual(machine.desktop.screenshot()[:4], b"\x89PNG")
        Node.answers[("POST", "/machines/web-01/desktop/actions")] = (200, {"cursor": [5, 6]})
        self.assertEqual(machine.desktop.drag(1, 2, 5, 6), [5, 6])
        self.assertEqual(self.last()[3], {"action": "drag", "x": 1, "y": 2, "toX": 5, "toY": 6, "button": "left"})
        machine.desktop.key("ctrl+c")
        self.assertEqual(self.last()[3], {"action": "key", "keys": "ctrl+c"})

    def test_files_go_in_and_out_as_bytes(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("PUT", "/machines/web-01/files?path=%2Fa+b%2Fc&user=dev")] = (204, None)
        machine.files.write("/a b/c", b"\x00\xffdata", user="dev")
        self.assertEqual(Node.seen[-1][3], b"\x00\xffdata")
        Node.answers[("GET", "/machines/web-01/files?path=%2Fetc%2Fhostname")] = (200, b"web-01\n\x00")
        self.assertEqual(machine.files.read("/etc/hostname"), b"web-01\n\x00")

    def test_a_snapshot_is_read_and_downloaded_with_holes(self):
        Node.answers[("GET", "/machines/web-01")] = (200, MACHINE)
        machine = self.hm.machines.get("web-01")
        Node.answers[("POST", "/machines/web-01/snapshots")] = (201, {"snapshotID": "snap-1", "name": "x"})
        snap = machine.snapshots.take("x")
        Node.answers[("GET", "/machines/web-01/snapshots/snap-1/files?path=%2Fetc")] = (
            200, {"entries": [{"name": "hostname", "type": "file"}]})
        self.assertEqual(snap.listdir("/etc")[0]["name"], "hostname")
        Node.answers[("GET", "/machines/web-01/snapshots/snap-1/file?path=%2Fetc%2Fhostname&partition=1")] = (
            200, b"web-01\n")
        self.assertEqual(snap.read("/etc/hostname", partition=1), b"web-01\n")
        disk = b"head" + bytes(3 << 20) + b"tail"
        Node.answers[("GET", "/machines/web-01/snapshots/snap-1/disk")] = (200, disk)
        with tempfile.TemporaryDirectory() as scratch:
            target = os.path.join(scratch, "disk.img")
            self.assertEqual(snap.download(target), len(disk))
            with open(target, "rb") as written:
                self.assertEqual(written.read(), disk)
            # Never over a file that is there.
            with self.assertRaises(FileExistsError):
                snap.download(target)

    def test_usage_and_environments(self):
        Node.answers[("GET", "/usage?from=2026-10-01T00%3A00%3A00Z&by=team")] = (200, {"total": {"sandboxes": 1}})
        self.assertEqual(self.hm.usage(start="2026-10-01T00:00:00Z", by="team")["total"]["sandboxes"], 1)
        Node.answers[("PUT", "/environments/web")] = (201, {"name": "web"})
        self.hm.put_environment("web", {"A": "1"}, {"/etc/x": b"\x00bin"})
        self.assertEqual(self.last()[3], {"envVars": {"A": "1"},
                                          "files": [{"path": "/etc/x",
                                                     "contentBase64": base64.b64encode(b"\x00bin").decode()}]})


if __name__ == "__main__":
    unittest.main()
