import json, os, re, socket, subprocess, sys, time, urllib.request, urllib.error

DAEMON = "/var/tmp/hm-trial-target/release/hv2-sandboxd"
KERNEL = "/var/tmp/hm-docker/bzImage-kvm"
INITRD = "/var/tmp/hm-docker/docker-kvm.cpio.gz"
OUT = "/var/tmp/nested-diag3"
os.makedirs(OUT, exist_ok=True)
BASE = "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1"
# (tag, guest MiB, kernel command line): the baseline repeated, a small guest, and a fixed delay-loop value.
VARIANTS = [("long256", "256", BASE)]


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


api, proxy = port(), port()
base = f"http://127.0.0.1:{api}"
env = dict(os.environ, HV2_KERNEL=KERNEL, HV2_INITRD=INITRD, RUST_LOG="info")
log = open(f"{OUT}/daemon.log", "wb")
daemon = subprocess.Popen([DAEMON, "--port", str(api), "--proxy-port", str(proxy), "--memory-mb", "2048",
                           "--cpu-cores", "2", "--capacity", "2"],
                          env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)


def request(method, path, body=None, timeout=300):
    req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                 data=None if body is None else json.dumps(body).encode())
    try:
        response = urllib.request.urlopen(req, timeout=timeout)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        return response.status, json.loads(raw) if raw else None


def sh(box, cmd, within=120):
    status, value = request("POST", f"/sandboxes/{box}/exec", {"cmd": cmd, "timeout_secs": within}, timeout=within + 60)
    assert status == 200, (status, value)
    return value


box = None
try:
    deadline = time.monotonic() + 240
    while True:
        assert daemon.poll() is None, "the daemon exited"
        try:
            if request("GET", "/sandboxes", timeout=5)[0] == 200:
                break
        except OSError:
            pass
        assert time.monotonic() < deadline, "daemon readiness"
        time.sleep(0.2)
    status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 900})
    assert status in (200, 201), (status, made)
    box = made["sandboxID"]
    for tag, mem, cmdline in VARIANTS:
        cmd = ("cd /opt/nested && HV2_CMDLINE='" + cmdline + "' HV2_INITRD=/opt/nested/inner.cpio.gz "
               "HV2_MEMORY_MIB=" + mem + " HV2_SETTLE_SECS=240 timeout 330 ./linux_boot_probe /opt/nested/bzImage "
               "> /tmp/probe.out 2>/tmp/probe.err; echo rc=$?")
        began = time.monotonic()
        sh(box, cmd, 360)
        wall = round(time.monotonic() - began, 1)
        out = sh(box, "cat /tmp/probe.out", 60)["stdout"]
        out = re.sub(r"\x1b\[[0-9;]*m", "", out)
        open(f"{OUT}/{tag}.out", "w").write(out)
        stamps = re.findall(r"^\[\s*(\d+\.\d+)\]", out, re.M)
        exits = re.findall(r"^exits\s*:\s*(\d+)", out, re.M)
        last = stamps[-1] if stamps else None
        reached_init = "NESTED-GUEST-USERSPACE-UP" in out
        print(f"{tag}: mem={mem} MiB wall={wall}s last_guest_time={last}s console_lines={len(stamps)} "
              f"exits={exits[:1]} userspace={reached_init}", flush=True)
finally:
    if box:
        try:
            request("DELETE", f"/sandboxes/{box}", timeout=60)
        except Exception as e:
            print("delete failed", e)
    if daemon.poll() is None:
        daemon.terminate()
        try:
            daemon.wait(timeout=30)
        except subprocess.TimeoutExpired:
            daemon.kill()
    log.close()
