import json, os, re, socket, subprocess, sys, time, urllib.request, urllib.error

DAEMON = "/var/tmp/hm-trial-target/release/hv2-sandboxd"
KERNEL = "/var/tmp/hm-docker/bzImage-kvm"
INITRD = "/var/tmp/hm-docker/docker-kvm.cpio.gz"
OUT = "/var/tmp/nested-diag7"
os.makedirs(OUT, exist_ok=True)
BASE = "console=ttyS0,115200 earlyprintk=serial,ttyS0,115200 panic=1"
# (tag, guest MiB, kernel command line): the baseline repeated, a small guest, and a fixed delay-loop value.
VARIANTS = [("base256", "256", BASE),
            ("mem96", "96", BASE),
            ("lpj", "256", BASE + " lpj=4000000 tsc=reliable")]


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
    src = open("kvm_cpuid.py")  # next to this script.read()
    nl = chr(10)
    sh(box, "cat > /tmp/kvm_cpuid.py <<'PYEOF'" + nl + src + "PYEOF", 30)
    r = sh(box, "python3 /tmp/kvm_cpuid.py 2>&1; echo; grep -o -w hypervisor /proc/cpuinfo | head -1; dmesg | grep -i -E 'hypervisor detected|kvm-clock|Booting paravirtualized' | head -5", 60)
    print(r["stdout"], r["stderr"], flush=True)
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
