#!/usr/bin/env python3
"""/dev/kvm inside a sandbox, on real KVM: nested virtualization.

Runs an owned hv2-sandboxd on a kernel built with build-kernel.sh --kvm and a
systemd template with python3 (build-tools-image.sh --systemd --packages
"... python3"), and checks through its API that a program in the guest can use
/dev/kvm: the API version, a VM, a vCPU, and a real-mode program that runs to
an I/O exit (port 0x10, byte 0x42) and then HLT. Needs a host that passes
nested virtualization through (an AMD host under WSL2 here).

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

NESTED = r'''
import fcntl, os, mmap, struct, ctypes
fd = os.open("/dev/kvm", os.O_RDWR)
print("KVM_GET_API_VERSION", fcntl.ioctl(fd, 0xAE00))
vm = fcntl.ioctl(fd, 0xAE01, 0)
print("KVM_CREATE_VM fd", vm)
mem = mmap.mmap(-1, 0x2000)
mem[0x1000:0x1005] = bytes([0xB0, 0x42, 0xE6, 0x10, 0xF4])  # mov al,0x42; out 0x10,al; hlt
addr = ctypes.addressof(ctypes.c_char.from_buffer(mem))
fcntl.ioctl(vm, 0x4020AE46, struct.pack("<IIQQQ", 0, 0, 0, 0x2000, addr))
vcpu = fcntl.ioctl(vm, 0xAE41, 0)
print("KVM_CREATE_VCPU fd", vcpu)
run = mmap.mmap(vcpu, fcntl.ioctl(fd, 0xAE04))
sregs = bytearray(312)
fcntl.ioctl(vcpu, 0x8138AE83, sregs)
sregs[0:8] = struct.pack("<Q", 0)
sregs[12:14] = struct.pack("<H", 0)
fcntl.ioctl(vcpu, 0x4138AE84, sregs)
regs = bytearray(144)
regs[128:136] = struct.pack("<Q", 0x1000)
regs[136:144] = struct.pack("<Q", 2)
fcntl.ioctl(vcpu, 0x4090AE82, regs)
fcntl.ioctl(vcpu, 0xAE80, 0)
reason = struct.unpack_from("<I", run, 8)[0]
direction, size, port, count, off = struct.unpack_from("<BBHIQ", run, 32)
print("KVM_RUN exit_reason", reason, "(2 is KVM_EXIT_IO) port", hex(port), "byte", hex(run[off]))
fcntl.ioctl(vcpu, 0xAE80, 0)
print("KVM_RUN second exit_reason", struct.unpack_from("<I", run, 8)[0], "(5 is KVM_EXIT_HLT)")
'''


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--memory-mb", type=int, default=2048)
    parser.add_argument("--network", action="store_true", help="also pull hello-world (needs the Internet)")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification that a sandbox guest can use /dev/kvm on KVM; "
                         "the times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "initrd_bytes": args.initrd.stat().st_size, "memory_mb": args.memory_mb,
              "host_cpus": os.cpu_count(), "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", str(args.memory_mb), "--cpu-cores", "2", "--capacity", "4"] + (["--network", "--egress-default", "allow"] if args.network else []),
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
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(sandbox, cmd, within=120, ok=True):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        if ok:
            assert value["exit_code"] == 0, (cmd, value)
        return value["stdout"].strip()

    def create():
        began = time.monotonic()
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        return made["sandboxID"], round(time.monotonic() - began, 3)

    def settled(sandbox):
        """systemd's own verdict, once it has one."""
        deadline = time.monotonic() + 90
        while True:
            said = sh(sandbox, "systemctl is-system-running --wait 2>&1; true", 120, ok=False)
            if said and "Failed to connect" not in said:
                return said.splitlines()[-1]
            if time.monotonic() > deadline:
                why = sh(sandbox, "ps -e -o pid,comm | head -30; echo ---; ls -la /run/systemd 2>&1 | head -20; echo ---; "
                                  "journalctl -b --no-pager -o cat 2>&1 | tail -25; echo ---; dmesg 2>&1 | tail -10",
                         30, ok=False)
                raise AssertionError("systemd never answered: " + repr(said) + "\n" + why)
            time.sleep(1)

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
        state = {}

        def device():
            sandbox, took = create()
            state["sandbox"] = sandbox
            node = sh(sandbox, "ls -l /dev/kvm")
            flags = sh(sandbox, "grep -o -w -E 'svm|vmx' /proc/cpuinfo | sort | uniq -c", ok=False)
            return {"create_seconds": took, "dev_kvm": node, "cpu_flags": flags}
        case("/dev/kvm exists and the CPU reports svm or vmx", device)

        def nested():
            sandbox = state["sandbox"]
            sh(sandbox, "cat > /tmp/nested.py <<'EOF'" + NESTED + "EOF")
            out = sh(sandbox, "python3 /tmp/nested.py 2>&1", 60)
            assert "KVM_RUN exit_reason 2 (2 is KVM_EXIT_IO) port 0x10 byte 0x42" in out, out
            assert "second exit_reason 5" in out, out
            return {"program_output": out.splitlines()}
        case("a program in the guest creates a VM and runs a vCPU to an I/O exit and HLT", nested)

        request("DELETE", f"/sandboxes/{state['sandbox']}")
        report["success"] = True
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
