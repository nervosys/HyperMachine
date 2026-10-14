#!/usr/bin/env python3
"""A port of the caller's reached from inside a sandbox, on real KVM.

Runs an owned hv2-sandboxd and one sandbox with no network interface at
all, and checks:

1. Through the API: a caller opens `GET /sandboxes/{id}/ports/7000/reverse`
   upgraded to hv2-reverse/1; a program in the guest connects to its
   127.0.0.1:7000; the caller is sent `+` and then the program's bytes, and
   what the caller writes back reaches the program.
2. Through `hm sandbox vm reverse`: the caller serves HTTP on its own
   loopback, and `wget` in the guest fetches a file from 127.0.0.1:7001
   there, byte for byte, several at once.
3. The caller gone, the guest's port is closed a few seconds later, and a
   program connecting to it is refused.
4. What cannot be done is refused: port 0, a sandbox that is not there, a
   request that is not an upgrade.

Writes report.json and the daemon's log into --output.
"""
import argparse, concurrent.futures, hashlib, http.server, json, os, shutil, signal, socket, subprocess
import tempfile, threading, time, urllib.error, urllib.request
from pathlib import Path


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
              "purpose": "functional verification of a reverse port forward on KVM; the times are the check's "
                         "own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-reverse-port-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    children = []

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

    def sh(sandbox, cmd, timeout=60):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": timeout})
        assert status == 200, (status, value)
        return value

    def upgrade(sandbox, guest_port, protocol="hv2-reverse/1"):
        sock = socket.create_connection(("127.0.0.1", api), timeout=30)
        sock.sendall((f"GET /sandboxes/{sandbox}/ports/{guest_port}/reverse HTTP/1.1\r\n"
                      f"Host: 127.0.0.1\r\nConnection: Upgrade\r\nUpgrade: {protocol}\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = sock.recv(1)
            assert chunk, head
            head += chunk
        status = int(head.split(b" ", 2)[1])
        return sock, status, head.decode(errors="replace")

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
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        sandbox = made["sandboxID"]
        state = {}
        # No network card: loopback and the kernel's own tunnel devices
        # (sit0), none of them backed by a device.
        links = sh(sandbox, "ls /sys/class/net")["stdout"].split()
        cards = sh(sandbox, "for i in /sys/class/net/*; do [ -e $i/device ] && echo ${i##*/}; done; true")
        report["guest_interfaces"] = links
        assert cards["stdout"].split() == [], (links, cards)

        def through_api():
            sock, status, head = upgrade(sandbox, 7000)
            assert status == 101 and "hv2-reverse/1" in head, head
            # The guest program: say hello, print what comes back.
            result = {}
            program = threading.Thread(target=lambda: result.update(
                sh(sandbox, "printf 'hello from the guest\\n' | nc -w 5 127.0.0.1 7000")))
            began = time.monotonic()
            program.start()
            sock.settimeout(30)
            assert sock.recv(1) == b"+"
            got = b""
            while b"\n" not in got:
                got += sock.recv(4096)
            took = round(time.monotonic() - began, 3)
            assert got == b"hello from the guest\n", got
            sock.sendall(b"hello from the caller\n")
            sock.close()
            program.join(timeout=30)
            assert "hello from the caller" in result.get("stdout", ""), result
            return {"seconds_to_first_bytes": took}
        case("through the API: a guest program's connection reaches the caller, both ways", through_api)

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
                [str(args.hm), "sandbox", "vm", "--endpoint", base, "reverse", sandbox, "--port", "7001",
                 "--to", f"127.0.0.1:{local}", "--waiting", "4"],
                stdout=subprocess.PIPE, stderr=(args.output / "hm.log").open("wb"), text=True)
            children.append(hm)
            ready = json.loads(hm.stdout.readline())
            assert ready["guestPort"] == 7001, ready
            state["hm"], state["server"] = hm, server
            began = time.monotonic()
            fetched = sh(sandbox, "cd /tmp && for i in 1 2 3; do wget -q -O got-$i http://127.0.0.1:7001/file.bin & "
                                  "done; wait; sha256sum got-1 got-2 got-3")
            took = round(time.monotonic() - began, 3)
            sums = [line.split()[0] for line in fetched["stdout"].splitlines()]
            want = hashlib.sha256(payload).hexdigest()
            assert sums == [want] * 3, (sums, want, fetched)
            return {"fetches": 3, "bytes_each": len(payload), "seconds": took}
        case("through hm: wget in the guest fetches from the caller's server, several at once", through_hm)

        def caller_gone():
            state["hm"].send_signal(signal.SIGINT)
            state["hm"].wait(timeout=30)
            state["server"].shutdown()
            time.sleep(7)
            refused = sh(sandbox, "nc -w 2 127.0.0.1 7001 </dev/null; echo exit=$?")
            assert "exit=1" in refused["stdout"], refused
            return {"after": refused["stdout"].strip()}
        case("with the caller gone, the guest's port is closed and refuses a program", caller_gone)

        def refusals():
            refused = {}
            _, status, head = upgrade(sandbox, 0)
            assert status == 400, head
            refused["port 0"] = status
            _, status, head = upgrade("sbx-absent", 7000)
            assert status == 404, head
            refused["no such sandbox"] = status
            status, value = request("GET", f"/sandboxes/{sandbox}/ports/7000/reverse")
            assert status == 400, (status, value)
            refused["not an upgrade"] = status
            return {"refused": refused}
        case("what cannot be done is refused", refusals)

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
