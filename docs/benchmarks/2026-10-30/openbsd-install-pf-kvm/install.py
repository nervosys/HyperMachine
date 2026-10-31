#!/usr/bin/env python3
"""Install OpenBSD 8.0 from the miniroot onto a 4 GiB virtio disk over the serial console, by answering
the installer's prompts in order, then halt and keep the installed disk as a fixture."""
import json, os, re, shutil, signal, socket, subprocess, sys, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

T = os.environ.get("HMT", "/var/tmp/hm-trace-target/release")
img, out = sys.argv[1], Path(sys.argv[2])
keep = sys.argv[3]
out.mkdir(parents=True, exist_ok=True)
PW = "swtest-pw1"
STEPS = [
    (r"\(S\)hell\?", "I\n"),
    (r"Terminal type\?", "vt220\n"),
    (r"System hostname\?", "obsd-pf\n"),
    (r"Network interface to configure\?", "\n"),
    (r"IPv4 address for vio0\?", "\n"),
    (r"IPv6 address for vio0\?", "none\n"),
    (r"Network interface to configure\?.*\[done\]", "\n"),
    (r"Password for root account\? \(will not echo\)", PW + "\n"),
    (r"Password for root account\? \(again\)", PW + "\n"),
    (r"Start sshd\(8\) by default\?", "no\n"),
    (r"Change the default console to com0\?", "\n"),
    (r"Which speed should com0 use\?", "\n"),
    (r"Setup a user\?", "\n"),
    (r"What timezone are you in\?", "UTC\n"),
    (r"Which disk is the root disk\?", "\n"),
    (r"Encrypt the root disk", "\n"),
    (r"Use \(W\)hole disk MBR", "G\n"),
    (r"Use \(A\)uto layout", "\n"),
    (r"Location of sets\?", "\n"),
    (r"HTTP proxy URL\?", "\n"),
    (r"HTTP Server\?", "mirror.leaseweb.com\n"),
    (r"Server directory\?", "\n"),
    (r"Set name\(s\)\?", "-comp* -man* -game* -x*\n"),
    (r"Set name\(s\)\?", "\n"),
    (r"Location of sets\?.*\[done\]", "\n"),
    (r"Exit to \(S\)hell, \(H\)alt or \(R\)eboot\?", "H\n"),
]
work = Path(tempfile.mkdtemp(prefix="hm-obsd-inst-", dir="/var/tmp"))
(work / "images").mkdir()
subprocess.run(["cp", "--sparse=always", img, str(work / "images/obsd.raw")], check=True)


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


api = port()
base = f"http://127.0.0.1:{api}"
log = (out / "daemon.log").open("wb")
daemon = subprocess.Popen([f"{T}/hv2-sandboxd", "--port", str(api), "--proxy-port", str(port()), "--no-template",
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


def clean(text):
    text = re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text).replace("\x00", "")
    text = re.sub(r"(\x08 \x08.)+", "", text)
    return text.replace("\r\n", "\n").replace("\r", "\n")


def console():
    st, text = req("GET", "/machines/obsd-01/console?tail=900000")
    return clean(text) if isinstance(text, str) else ""


ok = False
try:
    for _ in range(300):
        assert daemon.poll() is None
        try:
            if req("GET", "/machines")[0] == 200:
                break
        except OSError:
            pass
        time.sleep(0.2)
    st, created = req("POST", "/machines", {"name": "obsd-01", "image": "obsd.raw", "cpuCount": 1, "memoryMB": 1024,
                                              "diskGiB": 4, "network": {"allowInternetAccess": True}})
    assert st == 201, (st, created)
    pos = 0
    t_start = time.monotonic()
    for i, (pat, send) in enumerate(STEPS):
        deadline = time.monotonic() + 900
        while True:
            c = console()
            m = re.search(pat, c[pos:], re.S)
            if m:
                break
            assert time.monotonic() < deadline, f"step {i} {pat!r}: not seen; console ends {c[-400:]!r}"
            time.sleep(1)
        time.sleep(1.5)
        # the next prompt is looked for after what is on the console now: it can print the
        # moment the answer is read, so the position is taken before the answer is sent
        pos = len(console())
        req("POST", "/machines/obsd-01/console", raw=send.encode())
        (out / "steps.log").open("a").write(f"{i} {time.monotonic() - t_start:7.1f}s saw {pat!r} sent {send!r}\n")
    # the installer halts the machine
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        st, m = req("GET", "/machines/obsd-01")
        if st == 200 and m.get("state") != "running":
            break
        time.sleep(2)
    (out / "console.txt").write_text(console())
    print("final state", m.get("state"))
    files = [p for p in work.joinpath("machines").rglob("*") if p.is_file()]
    files.sort(key=lambda p: p.stat().st_size, reverse=True)
    print("machine files", [(str(p.relative_to(work)), p.stat().st_size) for p in files[:5]])
    disk = files[0]
    subprocess.run(["cp", "--sparse=always", str(disk), keep], check=True)
    print("kept", keep, os.path.getsize(keep))
    ok = True
finally:
    (out / "console.txt").write_text(console()) if not (out / "console.txt").exists() else None
    req("POST", "/machines/obsd-01/stop")
    time.sleep(3)
    req("DELETE", "/machines/obsd-01")
    if daemon.poll() is None:
        daemon.send_signal(signal.SIGTERM)
        try:
            daemon.wait(timeout=60)
        except subprocess.TimeoutExpired:
            daemon.kill()
    log.close()
    (out / "result.txt").write_text("ok" if ok else "failed")
