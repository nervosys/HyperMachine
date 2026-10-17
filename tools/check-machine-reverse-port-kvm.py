#!/usr/bin/env python3
"""A port of the caller's reached from inside a machine, on real KVM.

Runs an owned hv2-sandboxd with a --machine-dir and one 512 MiB machine,
booted from the guest initrd carrying this change's guest agent, and checks:

1. Through the API: `GET /machines/{name}/ports/7000/reverse` upgraded to
   hv2-reverse/1; a program in the guest connects to its 127.0.0.1:7000;
   the caller is sent `+`, then the program's line, and its reply reaches
   the program.
2. Through `hm sandbox vm machine reverse`: the caller serves a random
   512 KiB file over HTTP on its own loopback, and three `wget`s in the
   guest fetch it at once through 127.0.0.1:7001, each byte-exact.
3. The caller gone, the guest's port is closed a few seconds later and a
   program connecting to it is refused.
4. Refusals: port 0 (400), a machine that is not there (404), a plain GET
   that is not an upgrade (400).
5. A machine stopped while 64 connections wait: all 64 end, and after it is
   started again a new one is accepted, not refused for the 64 it once had.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, http.server, json, os, shutil, signal, socket, subprocess, tempfile, threading, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

NAME = "rev-01"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "hm", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a reverse port forward from a machine on KVM; the times are "
                         "the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-reverse-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    children = []
    state = {}

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

    def sh(cmd, timeout=60):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": timeout})
        assert status == 200, (status, value)
        return value

    def upgrade(guest_port, path=None, protocol="hv2-reverse/1"):
        sock = socket.create_connection(("127.0.0.1", api), timeout=30)
        path = path or f"/machines/{NAME}/ports/{guest_port}/reverse"
        sock.sendall((f"GET {path} HTTP/1.1\r\n"
                      f"Host: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: {protocol}\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = sock.recv(1)
            assert chunk, head
            head += chunk
        return sock, int(head.split(b" ", 2)[1]), head.decode(errors="replace")

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
        assert status == 201, (status, created)
        deadline = time.monotonic() + 120
        while True:
            status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": "true", "timeout_secs": 10})
            if status == 200:
                break
            assert time.monotonic() < deadline, (status, value)
            time.sleep(0.5)

        def through_api():
            sock, status, head = upgrade(7000)
            assert status == 101 and "hv2-reverse/1" in head, head
            result = {}
            program = threading.Thread(target=lambda: result.update(
                sh("printf 'hello from the machine\\n' | nc -w 5 127.0.0.1 7000")))
            began = time.monotonic()
            program.start()
            sock.settimeout(30)
            assert sock.recv(1) == b"+"
            got = b""
            while b"\n" not in got:
                got += sock.recv(4096)
            took = round(time.monotonic() - began, 3)
            assert got == b"hello from the machine\n", got
            sock.sendall(b"hello from the caller\n")
            sock.close()
            program.join(timeout=30)
            assert "hello from the caller" in result.get("stdout", ""), result
            return {"seconds_to_first_bytes": took}
        case("through the API: a machine program's connection reaches the caller, both ways", through_api)

        def through_hm():
            served = work / "served"
            served.mkdir()
            payload = os.urandom(512 * 1024)
            (served / "file.bin").write_bytes(payload)
            handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(served), **k)
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            local = server.server_address[1]
            hm = subprocess.Popen(
                [str(args.hm), "sandbox", "vm", "--endpoint", base, "machine", "reverse", NAME, "--port", "7001",
                 "--to", f"127.0.0.1:{local}", "--waiting", "4"],
                stdout=subprocess.PIPE, stderr=(args.output / "hm.log").open("wb"), text=True)
            children.append(hm)
            ready = json.loads(hm.stdout.readline())
            assert ready["guestPort"] == 7001, ready
            state["hm"], state["server"] = hm, server
            want = hashlib.sha256(payload).hexdigest()
            began = time.monotonic()
            fetched = sh("cd /tmp && for i in 1 2 3; do wget -q -O got-$i http://127.0.0.1:7001/file.bin & "
                         "done; wait; sha256sum got-1 got-2 got-3")
            took = round(time.monotonic() - began, 3)
            sums = [line.split()[0] for line in fetched["stdout"].splitlines()]
            assert sums == [want] * 3, (sums, want, fetched)
            return {"fetches": 3, "bytes_each": len(payload), "seconds": took}
        case("through hm: wget in the machine fetches from the caller's server, several at once", through_hm)

        def caller_gone():
            state["hm"].send_signal(signal.SIGINT)
            state["hm"].wait(timeout=30)
            state["server"].shutdown()
            time.sleep(7)
            refused = sh("nc -w 2 127.0.0.1 7001 </dev/null; echo exit=$?")
            assert "exit=1" in refused["stdout"], refused
            return {"after": refused["stdout"].strip()}
        case("with the caller gone, the machine's port is closed and refuses a program", caller_gone)

        def refusals():
            refused = {}
            _, status, head = upgrade(0)
            assert status == 400, head
            refused["port 0"] = status
            _, status, head = upgrade(7000, path="/machines/rev-absent/ports/7000/reverse")
            assert status == 404, head
            refused["no such machine"] = status
            status, value = request("GET", f"/machines/{NAME}/ports/7000/reverse")
            assert status == 400, (status, value)
            refused["not an upgrade"] = status
            return {"refused": refused}
        case("what cannot be done is refused", refusals)

        def stop_releases_waiting():
            # A caller keeps connections waiting; the node allows 64 at once.
            waiting = []
            for _ in range(64):
                sock, status, head = upgrade(7002)
                assert status == 101, head
                waiting.append(sock)
            over, status, head = upgrade(7002)
            over.close()
            assert status == 429, ("the 65th was not refused", status, head)
            status, value = request("POST", f"/machines/{NAME}/stop", {})
            assert status in (200, 204), (status, value)
            # Every waiting connection ends with the machine.
            ended = 0
            deadline = time.monotonic() + 20
            for sock in waiting:
                sock.settimeout(max(0.2, deadline - time.monotonic()))
                try:
                    if sock.recv(1) == b"":
                        ended += 1
                except socket.timeout:
                    pass  # still open
                except OSError:
                    ended += 1  # reset
                sock.close()
            assert ended == 64, ("waiting connections still open after the stop", ended)
            status, value = request("POST", f"/machines/{NAME}/start", {}, timeout=180)
            assert status in (200, 204), (status, value)
            deadline = time.monotonic() + 120
            while True:
                status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": "true", "timeout_secs": 10})
                if status == 200:
                    break
                assert time.monotonic() < deadline, (status, value)
                time.sleep(0.5)
            # The table forgot them: one more is accepted, not refused with 429.
            again, status, head = upgrade(7002)
            again.close()
            assert status == 101, ("the stopped machine's waiting connections still count", status, head)
            return {"waiting_closed": ended, "after_restart": status}
        case("a stopped machine closes its waiting connections and forgets them", stop_releases_waiting)

        report["success"] = True
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
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
