#!/usr/bin/env python3
"""Spike: boot an OpenBSD image as a machine and dump what the console shows."""
import json, os, re, signal, socket, subprocess, sys, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

T = "/var/tmp/hm-trial-target/release"
img, out, secs = sys.argv[1], Path(sys.argv[2]), int(sys.argv[3])
extra = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
out.mkdir(parents=True, exist_ok=False)
work = Path(tempfile.mkdtemp(prefix="hm-obsd-", dir="/var/tmp"))
(work / "images").mkdir()
subprocess.run(["cp", "--sparse=always", img, str(work / "images/obsd.raw")], check=True)
with socket.socket() as s:
    s.bind(("127.0.0.1", 0)); api = s.getsockname()[1]
with socket.socket() as s:
    s.bind(("127.0.0.1", 0)); proxy = s.getsockname()[1]
base = f"http://127.0.0.1:{api}"
log = (out / "daemon.log").open("wb")
d = subprocess.Popen([f"{T}/hv2-sandboxd", "--port", str(api), "--proxy-port", str(proxy), "--no-template",
    "--machine-dir", str(work / "machines"), "--firmware", "/var/tmp/hm-firmware/hypervisor-fw",
    "--image-dir", str(work / "images"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
    env=dict(os.environ, RUST_LOG="info", HV2_KERNEL="/var/tmp/hm-competitive/bzImage-known-uart-irq",
             HV2_INITRD="/var/tmp/hm-prompts-initrd.cpio.gz"),
    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)


def req(method, path, body=None, raw=None):
    data = raw if raw is not None else (None if body is None else json.dumps(body).encode())
    r = urllib.request.Request(base + path, method=method, data=data, headers={"content-type": "application/json"})
    try:
        resp = urllib.request.urlopen(r, timeout=60)
    except urllib.error.HTTPError as e:
        resp = e
    with resp:
        t = resp.read()
        try:
            return resp.status, json.loads(t) if t else None
        except ValueError:
            return resp.status, t.decode(errors="replace")


try:
    for _ in range(300):
        assert d.poll() is None, "daemon exited"
        try:
            if req("GET", "/machines")[0] == 200:
                break
        except OSError:
            pass
        time.sleep(0.2)
    body = {"name": "obsd-01", "image": "obsd.raw", "cpuCount": 1, "memoryMB": 1024}
    body.update(extra)
    st, created = req("POST", "/machines", body)
    print("create", st, created, flush=True)
    t0 = time.monotonic()
    last = ""
    for delay, text in json.loads(os.environ.get("KEYS", "[]")):
        time.sleep(delay)
        print("send", repr(text), req("POST", "/machines/obsd-01/console", raw=text.encode())[0], flush=True)
    while time.monotonic() - t0 < secs:
        st, text = req("GET", "/machines/obsd-01/console?tail=20000")
        text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text if isinstance(text, str) else json.dumps(text))
        if text != last:
            last = text
            (out / "console.txt").write_text(text)
        st2, m = req("GET", "/machines/obsd-01")
        if st2 == 200 and m.get("state") != "running":
            print("state", m.get("state")); break
        time.sleep(3)
    (out / "console.txt").write_text(last)
    print("console bytes", len(last)); print(last[-1500:])
    st, m = req("GET", "/machines/obsd-01"); print("final state", m)
finally:
    req("POST", "/machines/obsd-01/stop")
    time.sleep(3)
    req("DELETE", "/machines/obsd-01")
    if d.poll() is None:
        d.send_signal(signal.SIGTERM)
        try:
            d.wait(timeout=60)
        except subprocess.TimeoutExpired:
            d.kill()
    log.close()
