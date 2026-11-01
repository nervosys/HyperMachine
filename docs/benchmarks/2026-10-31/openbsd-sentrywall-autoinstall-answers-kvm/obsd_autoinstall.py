#!/usr/bin/env python3
"""SentryWall's autoinstall answers, typed into the OpenBSD installer over the serial console in whatever
order the installer asks. Afterwards, in the installer's own shell, print the installed files' state."""
import json, os, re, signal, socket, subprocess, sys, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

T = os.environ.get("HMT", "/var/tmp/hm-trace-target/release")
img, out, keep = sys.argv[1], Path(sys.argv[2]), sys.argv[3]
out.mkdir(parents=True, exist_ok=True)
PUB = Path("/var/tmp/ai-testkey.pub").read_text().strip()
# (prompt regex, answers in order of occurrence)
PROMPTS = [
    (r"\(S\)hell\?", ["I\n"]),
    (r"Terminal type\?", ["vt220\n"]),
    (r"System hostname\?", ["sentrywall\n"]),
    (r"Network interface to configure\?", ["vio0\n", "done\n"]),
    (r"IPv4 address for vio0\?", ["dhcp\n"]),
    (r"IPv6 address for vio0\?", ["none\n"]),
    (r"DNS domain name\?", ["local\n"]),
    (r"Password for root account\? \(will not echo\)", ["*************\n"]),
    (r"Password for root account\? \(again\)", ["*************\n"]),
    (r"Start sshd\(8\) by default\?", ["yes\n"]),
    (r"expect to run the X Window System\?", ["no\n"]),
    (r"Change the default console to com0\?", ["yes\n"]),
    (r"Which speed should com0 use\?", ["115200\n"]),
    (r"Setup a user\?", ["no\n"]),
    (r"Allow root ssh login\?", ["prohibit-password\n"]),
    (r"Public ssh key for root account\?", [PUB + "\n"]),
    (r"What timezone are you in\?", ["UTC\n"]),
    (r"Start ntpd\(8\) by default\?", ["yes\n"]),
    (r"Which disk is the root disk\?", ["\n"]),
    (r"Encrypt the root disk", ["no\n"]),
    (r"Use \(W\)hole disk MBR", ["G\n"]),
    (r"Use \(A\)uto layout", ["A\n"]),
    (r"Location of sets\?", ["\n", "\n"]),
    (r"HTTP proxy URL\?", ["\n"]),
    (r"HTTP Server\?", ["mirror.leaseweb.com\n"]),
    (r"Server directory\?", ["\n"]),
    (r"Set name\(s\)\?", ["-comp* -man* -game* -x*\n", "done\n"]),
    (r"Continue without verification", ["yes\n"]),
]
FINAL = r"Exit to \(S\)hell, \(H\)alt or \(R\)eboot\?"
CHECKS = r"""echo AI-BEGIN
mount | head -3
echo "--1 root password field"; awk -F: '$1=="root"{print $2}' /mnt/etc/master.passwd
echo "--4 console"; grep -n -E 'com0|console' /mnt/etc/ttys /mnt/etc/boot.conf
echo "--boot.conf"; cat /mnt/etc/boot.conf
echo "--sshd_config"; grep -n -i -E '^ *(permitrootlogin|passwordauthentication)' /mnt/etc/ssh/sshd_config
echo "--3 sshd -T"; chroot /mnt /usr/sbin/sshd -T 2>&1 | grep -i -E 'permitrootlogin|passwordauthentication|hostkeys|error'
echo "--authorized_keys"; cut -c1-50 /mnt/root/.ssh/authorized_keys; ls -la /mnt/root/.ssh
echo "--rc.conf.local"; cat /mnt/etc/rc.conf.local; cat /mnt/etc/myname
echo AI-END-$((6*7))
"""
work = Path(tempfile.mkdtemp(prefix="hm-obsd-ai-", dir="/var/tmp"))
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


def send(text, paced=False):
    if not paced:
        req("POST", "/machines/obsd-01/console", raw=text.encode())
        return
    for ln in text.splitlines(keepends=True):
        req("POST", "/machines/obsd-01/console", raw=ln.encode())
        time.sleep(0.4)


ok = False
steplog = out / "steps.log"
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
    pos, used = 0, {}
    t0 = time.monotonic()
    last_progress = time.monotonic()
    while True:
        c = console()
        tail = c[pos:]
        best = None
        for i, (pat, answers) in enumerate(PROMPTS):
            m = re.search(pat, tail, re.S)
            if m and (best is None or m.start() < best[0]):
                best = (m.start(), m.end(), i)
        fin = re.search(FINAL, tail, re.S)
        if fin and (best is None or fin.start() < best[0]):
            time.sleep(1.5)
            pos = len(console())
            send("S\n")
            steplog.open("a").write(f"{time.monotonic() - t0:7.1f}s final prompt -> S\n")
            break
        if best:
            pat, answers = PROMPTS[best[2]]
            n = used.get(best[2], 0)
            ans = answers[n] if n < len(answers) else answers[-1]
            used[best[2]] = n + 1
            time.sleep(1.5)
            pos = len(console())
            send(ans)
            shown = ans if "BEGIN" not in ans and "ssh-" not in ans else ans[:20] + "..."
            steplog.open("a").write(f"{time.monotonic() - t0:7.1f}s {pat!r} #{n} -> {shown!r}\n")
            last_progress = time.monotonic()
            continue
        # a question we have no answer for: report it, do not guess
        if time.monotonic() - last_progress > 600:
            raise AssertionError(f"no known prompt for 600 s; console ends {c[-600:]!r}")
        assert daemon.poll() is None
        time.sleep(1)
    time.sleep(5)
    pos = len(console())
    send(CHECKS, paced=True)
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline and "AI-END-42" not in console()[pos:].replace("AI-END-$((6*7))", ""):
        time.sleep(1)
    c = console()
    (out / "console.txt").write_text(c)
    i = c.rfind("AI-BEGIN", 0, c.rfind("AI-END-42") + 1)
    (out / "checks.txt").write_text(c[i:c.rfind("AI-END-42") + 9])
    send("halt\n")
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        st, m = req("GET", "/machines/obsd-01")
        if st == 200 and m.get("state") != "running":
            break
        time.sleep(2)
    files = [p for p in work.joinpath("machines").rglob("*") if p.is_file()]
    files.sort(key=lambda p: p.stat().st_size, reverse=True)
    subprocess.run(["cp", "--sparse=always", str(files[0]), keep], check=True)
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
