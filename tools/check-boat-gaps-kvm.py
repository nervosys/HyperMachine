#!/usr/bin/env python3
"""Four claims in docs/BOAT_PARITY.md that had no kept report, on real KVM.

Each case starts its own hv2-sandboxd where it needs different flags, and is
recorded pass/fail with its evidence; a failure does not stop the others.

1. A full node answers 503 at once by default: `--capacity 1`, one sandbox
   created, a second create is answered 503 (the time is recorded).
2. Per-sandbox variables: a sandbox created with `envVars` shows them to an
   `exec`, the API never returns their values, and a sandbox created without
   them does not have them.
3. Changing a network list while the VM runs: with `--network
   --egress-default deny`, a sandbox allowed one local TCP target reaches it;
   `PUT /sandboxes/{id}/network` with no allow list makes the next
   connection fail; a PUT allowing it makes the next connection succeed (a
   fourth PUT, naming it in both lists, is recorded, not asserted); the
   VM is never restarted (same boot id), and
   `GET /sandboxes/{id}/network/decisions` records both verdicts. The target
   is a TCP listener on this host's own private address, which the operator
   opens to sandboxes with `--tenant-reserved-cidr` (as
   check-machine-network-kvm.py does).
4. A sandbox's terminal and standard input: through the sandbox's own
   process.Process listener, spoken as the Connect protocol (what E2B's client
   speaks), a process started with a pty gets a terminal; input sent to it is
   echoed and run; a process started without one reads standard input until
   its end is closed.

Not exercised: the E2B SDK itself, the Internet, name-based rules, DNS, a
cluster. Writes report.json and each daemon's log into --output. Needs root
and /dev/kvm. Exit status is 1 if anything failed.
"""
import argparse, base64, hashlib, http.client, json, os, re, socket, struct, subprocess, threading, time
import urllib.error, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Daemon:
    def __init__(self, args, label, extra):
        self.label = label
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        self.log_path = args.output / f"{label}.log"
        self.log = self.log_path.open("wb")
        self.argv = [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()),
                     "--memory-mb", "1024", "--cpu-cores", "1"] + extra
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
        self.proc = subprocess.Popen(self.argv, env=env, stdin=subprocess.DEVNULL,
                                     stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 300
        while True:
            assert self.proc.poll() is None, f"{label}: the daemon exited"
            try:
                if self.request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, f"{label}: daemon readiness"
            time.sleep(0.2)
        # A snapshot template makes creates quick; wait for it if one is made.
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            try:
                status, templates = self.request("GET", "/templates", timeout=5)
                if status == 200 and any(t.get("snapshot") for t in templates):
                    break
            except OSError:
                pass
            time.sleep(0.5)

    def request(self, method, path, body=None, timeout=300):
        req = urllib.request.Request(self.base + path, method=method,
                                     headers={"content-type": "application/json"},
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

    def run(self, sandbox, cmd, within=60):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        return value["exit_code"], (value["stdout"] + value["stderr"]).strip()

    def create(self, **extra):
        status, value = self.request("POST", "/sandboxes", {"templateID": "base", "timeout": 3600, **extra})
        assert status in (200, 201), (status, value)
        return value["sandboxID"], value

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()


class Stream:
    """A Connect server-streaming call: one request envelope, then frames back."""

    def __init__(self, host_port, token, method, message):
        self.frames, self.end, self.error = [], None, None
        self.conn = http.client.HTTPConnection("127.0.0.1", host_port, timeout=120)
        payload = json.dumps(message).encode()
        self.conn.request("POST", f"/process.Process/{method}", body=b"\x00" + struct.pack(">I", len(payload)) + payload,
                          headers={"content-type": "application/connect+json", "connect-protocol-version": "1",
                                   "x-access-token": token})
        self.response = self.conn.getresponse()
        self.status = self.response.status
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.read, daemon=True)
        self.thread.start()

    def read(self):
        try:
            while True:
                head = self.response.read(5)
                if len(head) < 5:
                    break
                flags, size = head[0], struct.unpack(">I", head[1:])[0]
                body = json.loads(self.response.read(size) or b"{}")
                with self.lock:
                    if flags & 2:
                        self.end = body
                        break
                    self.frames.append(body.get("event", body))
        except Exception as error:  # reported by whoever waits
            self.error = repr(error)

    def pid(self, within=30):
        end = time.monotonic() + within
        while time.monotonic() < end:
            with self.lock:
                for f in self.frames:
                    if "start" in f:
                        return f["start"]["pid"]
            time.sleep(0.05)
        raise AssertionError(f"no start event: {self.frames} {self.end} {self.error}")

    def output(self, kind):
        with self.lock:
            return b"".join(base64.b64decode(f["data"][kind]) for f in self.frames
                            if "data" in f and kind in f["data"]).decode(errors="replace")

    def wait_for(self, kind, needle, within=30):
        end = time.monotonic() + within
        while time.monotonic() < end:
            if needle in self.output(kind):
                return self.output(kind)
            time.sleep(0.05)
        raise AssertionError(f"{needle!r} not seen on {kind}: {self.output(kind)!r} end={self.end} err={self.error}")

    def exit(self, within=30):
        end = time.monotonic() + within
        while time.monotonic() < end:
            with self.lock:
                for f in self.frames:
                    if "end" in f:
                        return f["end"]
            time.sleep(0.05)
        raise AssertionError(f"process did not end: {self.frames[-3:]} {self.end} {self.error}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    assert os.geteuid() == 0 and Path("/dev/kvm").exists(), "root and /dev/kvm required"
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of four BOAT_PARITY claims on KVM; one 503 latency, no throughput claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    daemons = []

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        try:
            detail = action()
            row["success"] = True
            if detail is not None:
                row["detail"] = detail
        except BaseException as error:
            row["error"] = f"{type(error).__name__}: {str(error)[:800]}"
        row["seconds"] = round(time.monotonic() - began, 3)
        print(("ok:" if row["success"] else "FAIL:"), name, "" if row["success"] else row["error"], flush=True)

    def expect(condition, message):
        if not condition:
            raise AssertionError(message)

    def with_daemon(label, extra):
        d = Daemon(args, label, extra)
        daemons.append(d)
        return d

    # ---- 1. a full node answers 503 at once --------------------------------
    def full_node():
        d = with_daemon("capacity-1", ["--capacity", "1"])
        try:
            first, _ = d.create()
            expect(d.run(first, "echo up")[1] == "up", "the first sandbox did not answer")
            began = time.monotonic()
            status, value = d.request("POST", "/sandboxes", {"templateID": "base", "timeout": 60}, timeout=120)
            took = time.monotonic() - began
            expect(status == 503, (status, value))
            expect(took < 2.0, f"503 took {took:.3f}s")
            status2, listed = d.request("GET", "/sandboxes")
            expect(status2 == 200 and len(listed) == 1, listed)
            # Room again once the first ends: the refusal was about capacity.
            expect(d.request("DELETE", f"/sandboxes/{first}")[0] == 204, "delete")
            again, _ = d.create()
            expect(d.run(again, "echo up")[1] == "up", "no room after the first ended")
            return {"status": status, "seconds_to_503": round(took, 4), "message": value}
        finally:
            d.stop()
    case("1 --capacity 1: the second create is answered 503 at once", full_node)

    # ---- 2. per-sandbox variables ------------------------------------------
    def per_sandbox_env():
        d = with_daemon("envvars", ["--capacity", "4"])
        try:
            with_vars, created = d.create(envVars={"BOAT_SECRET": "s3cret-value-42", "BOAT_OTHER": "two"})
            without, _ = d.create()
            out = d.run(with_vars, 'echo "a=$BOAT_SECRET b=$BOAT_OTHER"')
            expect(out == (0, "a=s3cret-value-42 b=two"), out)
            via_env = d.run(with_vars, "env | grep ^BOAT_ | sort")
            expect(via_env[1].splitlines() == ["BOAT_OTHER=two", "BOAT_SECRET=s3cret-value-42"], via_env)
            bare = d.run(without, 'echo "a=[$BOAT_SECRET] b=[$BOAT_OTHER]"')
            expect(bare == (0, "a=[] b=[]"), bare)
            # A command run a second time, and through a shell of its own, still sees them.
            expect(d.run(with_vars, "sh -c 'echo $BOAT_SECRET'")[1] == "s3cret-value-42", "child shell")
            replies = [json.dumps(created)]
            for path in ["/sandboxes", f"/sandboxes/{with_vars}", "/v2/sandboxes"]:
                replies.append(json.dumps(d.request("GET", path)[1]))
            expect(all("s3cret-value-42" not in r for r in replies), "a variable's value was returned by the API")
            return {"with_vars": out[1], "env": via_env[1].splitlines(), "without": bare[1],
                    "values_in_api_replies": False}
        finally:
            d.stop()
    case("2 envVars: an exec sees them, the API does not return them, another sandbox lacks them",
         per_sandbox_env)

    # ---- 3. changing a list while the VM runs ------------------------------
    def live_network_change():
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
            probe.connect(("192.0.2.1", 9))
            private = probe.getsockname()[0]
        listener = socket.socket()
        listener.bind((private, 0))
        listener.listen(16)
        target_port = listener.getsockname()[1]
        accepted = []
        stop = threading.Event()

        def serve():
            listener.settimeout(0.5)
            while not stop.is_set():
                try:
                    conn, peer = listener.accept()
                except OSError:
                    continue
                accepted.append(time.time())
                conn.sendall(b"TARGET-TOKEN\n")
                conn.close()
        threading.Thread(target=serve, daemon=True).start()
        d = with_daemon("network", ["--capacity", "2", "--network", "--egress-default", "deny",
                                    "--tenant-reserved-cidr", f"{private}/32"])
        probe_cmd = ("python3 -c 'import socket\n"
                     "try:\n"
                     f" c = socket.create_connection((\"{private}\", {target_port}), 8)\n"
                     " print(\"GOT\", c.recv(64).decode().strip())\n"
                     "except Exception as e:\n"
                     " print(\"ERR\", repr(e))'")
        try:
            sandbox, _ = d.create(network={"allowOut": [f"{private}/32"]})
            boot = d.run(sandbox, "cat /proc/sys/kernel/random/boot_id")[1]

            def reach():
                return d.run(sandbox, probe_cmd)[1]
            out = {}
            out["1_allowed_at_creation"] = reach()
            expect(out["1_allowed_at_creation"] == "GOT TARGET-TOKEN", out)
            seen = len(accepted)

            status, value = d.request("PUT", f"/sandboxes/{sandbox}/network", {})
            expect(status == 204, ("clear", status, value))
            out["2_after_PUT_with_no_allow_list"] = reach()
            expect(out["2_after_PUT_with_no_allow_list"].startswith("ERR"), out)
            expect(len(accepted) == seen, "the target saw a connection the policy should have refused")

            status, value = d.request("PUT", f"/sandboxes/{sandbox}/network", {"allowOut": [f"{private}/32"]})
            expect(status == 204, ("allow", status, value))
            out["3_after_PUT_allowing_it"] = reach()
            expect(out["3_after_PUT_allowing_it"] == "GOT TARGET-TOKEN", out)

            # Listing the same address in both: observed and recorded, not asserted
            # (the result is the precedence between the two lists, which is not
            # one of the claims).
            status, value = d.request("PUT", f"/sandboxes/{sandbox}/network",
                                      {"allowOut": [f"{private}/32"], "denyOut": [f"{private}/32"]})
            expect(status == 204, ("deny", status, value))
            out["4_after_PUT_listing_it_in_allowOut_and_denyOut"] = reach()

            expect(d.run(sandbox, "cat /proc/sys/kernel/random/boot_id")[1] == boot, "the VM restarted")
            status, log = d.request("GET", f"/sandboxes/{sandbox}/network/decisions")
            expect(status == 200, (status, log))
            mine = [f'{x["verdict"]} {x["destination"]} ({x["reason"]})' for x in log["decisions"]
                    if x["destination"] == f"{private}:{target_port}"]
            expect(any(m.startswith("allow ") for m in mine) and any(m.startswith("deny ") for m in mine), mine)
            # In order: allow, deny, allow -- one verdict per connection tried (then
            # a fourth, whichever way the both-lists case went).
            order = [m.split()[0] for m in mine]
            expect(len(order) == 4 and order[:3] == ["allow", "deny", "allow"], order)
            out["same_boot_id"] = True
            out["decisions_for_the_target"] = mine
            out["connections_the_target_accepted"] = len(accepted)
            expect(len(accepted) == 2 + (out["4_after_PUT_listing_it_in_allowOut_and_denyOut"] == "GOT TARGET-TOKEN"),
                   f"target accepted {len(accepted)} connections")
            return out
        finally:
            stop.set()
            listener.close()
            d.stop()
    case("3 PUT /sandboxes/{id}/network changes a list on a running VM, with both verdicts logged",
         live_network_change)

    # ---- 4. a terminal and standard input -----------------------------------
    def terminal_and_stdin():
        d = with_daemon("terminal", ["--capacity", "2"])
        try:
            sandbox, created = d.create()
            host_port, token = created["processPort"], created["envdAccessToken"]
            out = {}

            def unary(method, message):
                conn = http.client.HTTPConnection("127.0.0.1", host_port, timeout=60)
                conn.request("POST", f"/process.Process/{method}", body=json.dumps(message),
                             headers={"content-type": "application/json", "connect-protocol-version": "1",
                                      "x-access-token": token})
                response = conn.getresponse()
                return response.status, response.read().decode()

            def b64(text):
                return base64.b64encode(text.encode()).decode()

            # A process with a terminal.
            pty = Stream(host_port, token, "Start", {"process": {"cmd": "/bin/sh", "args": ["-i"]},
                                                      "pty": {"size": {"cols": 100, "rows": 30}}})
            expect(pty.status == 200, pty.status)
            pid = pty.pid()
            status, reply = unary("SendInput", {"process": {"pid": pid}, "input": {"pty": b64("tty; echo pty-$((6*7)); stty size\n")}})
            expect(status == 200, (status, reply))
            seen = pty.wait_for("pty", "pty-42")
            expect("/dev/pts/" in seen or "/dev/tty" in seen, seen)
            pty.wait_for("pty", "30 100")
            expect("tty; echo pty-$((6*7))" in seen, "the terminal did not echo the typed line")
            out["pty_output"] = pty.output("pty")
            out["pty_events_are_pty_not_stdout"] = pty.output("stdout") == ""
            unary("SendInput", {"process": {"pid": pid}, "input": {"pty": b64("exit 7\n")}})
            ended = pty.exit()
            expect(ended.get("exitCode") == 7 and ended.get("exited") is True, ended)
            out["pty_end"] = ended

            # A process without one, reading standard input.
            plain = Stream(host_port, token, "Start", {"process": {"cmd": "/bin/sh", "args": ["-c", "cat; echo cat-done"]},
                                                        "stdin": True})
            expect(plain.status == 200, plain.status)
            pid = plain.pid()
            status, reply = unary("SendInput", {"process": {"pid": pid}, "input": {"stdin": b64("line one\nline two\n")}})
            expect(status == 200, (status, reply))
            plain.wait_for("stdout", "line two")
            status, reply = unary("CloseStdin", {"process": {"pid": pid}})
            expect(status == 200, (status, reply))
            ended = plain.exit()
            expect(ended.get("exitCode", 0) == 0 and ended.get("exited") is True, ended)
            text = plain.output("stdout")
            expect(text == "line one\nline two\ncat-done\n", text)
            out["stdin_output"] = text
            out["stdin_end"] = ended
            return out
        finally:
            d.stop()
    case("4 process.Process over Connect: a pty is a terminal, input is echoed and run, stdin reads to EOF",
         terminal_and_stdin)

    text = "".join(d.log_path.read_bytes().decode(errors="replace") for d in daemons)
    per_daemon = {d.label: d.log_path.read_bytes().decode(errors="replace").count("Detected hypervisor platform: Kvm")
                  for d in daemons}
    report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                "Tcg": text.count("Detected hypervisor platform: Tcg"),
                                "daemons": len(daemons), "Kvm_lines_per_daemon": per_daemon}
    report["daemon_flags"] = {d.label: d.argv[1:] for d in daemons}
    on_kvm = all(n >= 1 for n in per_daemon.values()) and report["platform_lines"]["Tcg"] == 0
    report["cases"].append({"name": "ran on KVM: every daemon logged platform Kvm and none Tcg",
                            "success": on_kvm, "detail": report["platform_lines"]})
    print(("ok:" if on_kvm else "FAIL:"), "ran on KVM", report["platform_lines"], flush=True)
    report["success"] = all(c["success"] for c in report["cases"])
    (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
