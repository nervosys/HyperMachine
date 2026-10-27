import json, os, socket, subprocess, sys, time, urllib.request, urllib.error

DAEMON = "/var/tmp/hm-trial-target/release/hv2-sandboxd"
KERNEL = "/var/tmp/hm-docker/bzImage-kvm"
INITRD = "/var/tmp/hm-docker/docker-kvm.cpio.gz"
OUT = "/var/tmp/nested-diag"
os.makedirs(OUT, exist_ok=True)


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
    return status, value


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
    status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 600})
    assert status in (200, 201), (status, made)
    box = made["sandboxID"]
    _, v = sh(box, "ls -l /dev/kvm; uname -r; nproc; echo ---; dmesg | grep -i -E 'kvm|svm|nested' | tail -8; "
                   "cat /sys/module/kvm_amd/parameters/nested 2>&1; ls -la /opt/nested", 60)
    print("L1 setup:\n", v["stdout"], v["stderr"])
    cmd = ("cd /opt/nested && RUST_LOG=info HV2_INITRD=/opt/nested/inner.cpio.gz HV2_MEMORY_MIB=256 "
           "HV2_SETTLE_SECS=60 timeout 150 ./linux_boot_probe /opt/nested/bzImage > /tmp/probe.out 2> /tmp/probe.err; "
           "echo rc=$?; wc -l /tmp/probe.out /tmp/probe.err")
    began = time.monotonic()
    _, v = sh(box, cmd, 200)
    print("probe ran", round(time.monotonic() - began, 1), "s:", v["stdout"], v["stderr"])
    for name in ("probe.err", "probe.out"):
        _, v = sh(box, f"cat /tmp/{name}", 60)
        open(f"{OUT}/{name}", "w").write(v["stdout"])
    _, v = sh(box, "grep -i -E 'pit|irqchip|warn|error|fail' /tmp/probe.err | head -20; echo --- console tail; tail -15 /tmp/probe.out", 60)
    print(v["stdout"])
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
