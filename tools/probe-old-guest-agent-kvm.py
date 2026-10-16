#!/usr/bin/env python3
"""What an older guest agent does when asked to listen, through a node, on KVM.

Usage: probe-old-guest-agent-kvm.py DAEMON KERNEL INITRD

INITRD is a guest image from before reverse ports. Boots one sandbox, shows
that `exec` works, asks for a reverse port (expecting 502 with a message
saying to rebuild the image), then shows `exec` and the process list still
work. Prints what each answered; asserts nothing."""
import json, os, signal, socket, subprocess, sys, tempfile, time, urllib.request, urllib.error

daemon, kernel, initrd = sys.argv[1:4]


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


api = port()
base = f"http://127.0.0.1:{api}"
log = open("/var/tmp/old-agent-daemon.log", "wb")
proc = subprocess.Popen([daemon, "--port", str(api), "--proxy-port", str(port()), "--memory-mb", "256",
                         "--cpu-cores", "1", "--capacity", "4"],
                        env=dict(os.environ, HV2_KERNEL=kernel, HV2_INITRD=initrd, RUST_LOG="info"),
                        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)


def request(method, path, body=None):
    req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                 data=None if body is None else json.dumps(body).encode())
    try:
        r = urllib.request.urlopen(req, timeout=120)
    except urllib.error.HTTPError as e:
        r = e
    with r:
        raw = r.read()
        try:
            return r.status, json.loads(raw) if raw else None
        except ValueError:
            return r.status, raw.decode(errors="replace")


try:
    for _ in range(600):
        try:
            if request("GET", "/sandboxes")[0] == 200:
                break
        except OSError:
            time.sleep(0.2)
    status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 300})
    sbx = made["sandboxID"]
    status, out = request("POST", f"/sandboxes/{sbx}/exec", {"cmd": "echo alive"})
    print("exec:", status, out["stdout"].strip())
    sock = socket.create_connection(("127.0.0.1", api), timeout=30)
    sock.sendall((f"GET /sandboxes/{sbx}/ports/7000/reverse HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\n"
                  "Upgrade: hv2-reverse/1\r\n\r\n").encode())
    data = b""
    sock.settimeout(30)
    try:
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
    except OSError:
        pass
    head, _, body = data.partition(b"\r\n\r\n")
    print("reverse status line:", head.split(b"\r\n")[0].decode())
    print("reverse body:", body.decode(errors="replace")[:300])
    status, out = request("POST", f"/sandboxes/{sbx}/exec", {"cmd": "echo still-alive"})
    print("exec after:", status, out["stdout"].strip())
    status, out = request("GET", f"/sandboxes/{sbx}/processes")
    print("processes:", status, len(out["processes"]) if isinstance(out, dict) else out)
finally:
    proc.send_signal(signal.SIGTERM)
    try:
        proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        proc.kill()
