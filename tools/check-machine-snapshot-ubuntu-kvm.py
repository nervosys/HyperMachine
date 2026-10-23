#!/usr/bin/env python3
"""A snapshot of an Ubuntu cloud-image machine: a partitioned disk, read without booting it, on real KVM.

`check-machine-snapshot-archive-kvm.py` uses a template machine whose disk is
one bare ext4 filesystem. Ubuntu's cloud image is a GPT disk with several
partitions, and it is booted by firmware. Runs an owned hv2-sandboxd with
`--firmware` and `--image-dir`, creates a machine from the image with
cloud-init user data that, on first boot, writes what the guest sees of
`/dev/kvm` into a file, stops it, snapshots it, and checks through the
snapshot routes only:

1. `files?path=/etc` lists the root filesystem of the partitioned disk,
   without a partition named, and names `os-release`.
2. `archive?path=/etc/cloud` is a gzipped tar whose regular files are
   byte for byte the ones on the disk, which this script mounts read-only
   through a loop device as its own oracle (no file missing, none extra).
3. `archive?path=/etc/cloud&partition=N` for the root partition's number is
   the same archive; a partition number that is not a filesystem is refused.
4. The probe file the guest wrote on first boot is read back with `file`.
   It shows `/dev/kvm` in the guest, and a tiny real-mode program (`mov
   al,0x42; out 0x10,al; hlt`) run by the guest on its own KVM: the first
   `KVM_RUN` exits with an I/O on port 0x10 of byte 0x42, the second with
   HLT. That needs a host that passes nested virtualization through.

Needs root, losetup, mount and sgdisk-free tools only (it reads the GPT
itself through `losetup -P`). Writes report.json, the node's log and the
probe file into --output.
"""
import argparse, hashlib, io, json, os, shutil, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

NAME = "ubu-01"
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

USER_DATA = """#cloud-config
password: hmcheck
chpasswd:
  expire: false
runcmd:
  - |
    {
      echo "== cpuinfo"; grep -m1 -o -w 'svm\\|vmx' /proc/cpuinfo || echo "no svm/vmx flag"
      echo "== /dev/kvm before"; ls -l /dev/kvm 2>&1
      echo "== modprobe"; modprobe kvm_amd 2>&1; modprobe kvm_intel 2>&1
      echo "== /dev/kvm after"; ls -l /dev/kvm 2>&1
      echo "== kvm ioctls"
      if command -v python3 >/dev/null; then
        echo @NESTED@ | base64 -d | python3 2>&1
      else echo "no python3 in the image"; fi
      echo "== dmesg"; dmesg | grep -i -E 'kvm|svm' | head -20
      echo "== done"
    } > /var/log/hm-kvm-probe.txt 2>&1
    sync
"""


def user_data():
    import base64
    return USER_DATA.replace("@NESTED@", base64.b64encode(NESTED.encode()).decode())


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "firmware", "image", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    assert os.geteuid() == 0, "root required (loop devices)"
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "cases": [],
              "purpose": "functional verification of reading a partitioned Ubuntu disk's snapshot; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "firmware", "image"]}}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-ubuntu-snapshot-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(images / "ubuntu.raw")], check=True)
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, RUST_LOG="info"), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    loop = None
    mount = work / "mnt"

    def raw(method, path, body=None, timeout=900):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, {k.lower(): v for k, v in response.headers.items()}, response.read()

    def request(method, path, body=None):
        status, _, data = raw(method, path, body)
        try:
            return status, json.loads(data) if data else None
        except ValueError:
            return status, data.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        detail = action()
        row["seconds"] = round(time.monotonic() - began, 3)
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {}
        status, created = request("POST", "/machines", {
            "name": NAME, "image": "ubuntu.raw", "cpuCount": 2, "memoryMB": 2048, "userData": user_data()})
        assert status == 201, (status, created)
        # cloud-init runs the user data's runcmd once per instance, late in the first boot, and
        # stopping the machine earlier would cut it off for good. Wait it out, then stop.
        time.sleep(240)
        status, _ = request("POST", f"/machines/{NAME}/stop")
        assert status == 200, status
        status, snap = request("POST", f"/machines/{NAME}/snapshots", {"name": "ubuntu"})
        assert status == 201, (status, snap)
        sid = snap["snapshotID"]
        state["snap"] = sid
        status, _, body = raw("GET", f"/machines/{NAME}/snapshots/{sid}/file?" + urllib.parse.urlencode(
            {"path": "/var/log/hm-kvm-probe.txt"}))
        probe = body.decode(errors="replace") if status == 200 and b"== done" in body else None
        if not probe:
            report["probe_file_status"] = [status, body[:300].decode(errors="replace")]
        assert probe, "cloud-init never wrote the probe file"
        (args.output / "hm-kvm-probe.txt").write_text(probe)
        sid = state["snap"]

        def listing():
            status, value = request("GET", f"/machines/{NAME}/snapshots/{sid}/files?" + urllib.parse.urlencode(
                {"path": "/etc"}))
            assert status == 200, (status, value)
            names = {e["name"] for e in value["entries"]} if isinstance(value, dict) and "entries" in value \
                else {e["name"] for e in value}
            assert "os-release" in names, sorted(names)[:20]
            return {"entries": len(names)}
        case("the root filesystem of the partitioned disk is listed with no partition named", listing)

        # The oracle: the same disk, mounted read-only through a loop device.
        status, _, disk = raw("GET", f"/machines/{NAME}/snapshots/{sid}/disk")
        assert status == 200
        image = work / "snapshot.raw"
        image.write_bytes(disk)
        loop = subprocess.check_output(["losetup", "-rP", "--show", "-f", str(image)], text=True).strip()
        partitions = sorted(p for p in Path("/dev").glob(Path(loop).name + "p*"))
        assert partitions, "no partitions seen on the loop device"
        root = None
        for part in partitions:
            kind = subprocess.run(["blkid", "-o", "value", "-s", "TYPE", str(part)], capture_output=True, text=True).stdout.strip()
            label = subprocess.run(["blkid", "-o", "value", "-s", "LABEL", str(part)], capture_output=True, text=True).stdout.strip()
            report.setdefault("partitions", []).append({"device": part.name, "type": kind, "label": label})
            if kind == "ext4" and label == "cloudimg-rootfs":
                root = part
        assert root, report["partitions"]
        number = int(root.name.rsplit("p", 1)[1])
        mount.mkdir()
        subprocess.run(["mount", "-o", "ro,noload", str(root), str(mount)], check=True)
        oracle = {}
        for path in sorted((mount / "etc/cloud").rglob("*")):
            if path.is_file() and not path.is_symlink():
                oracle[str(path.relative_to(mount / "etc/cloud"))] = hashlib.sha256(path.read_bytes()).hexdigest()
        assert oracle, "the oracle found nothing under /etc/cloud"

        def archived(extra=""):
            status, headers, body = raw("GET", f"/machines/{NAME}/snapshots/{sid}/archive?" + urllib.parse.urlencode(
                {"path": "/etc/cloud"}) + extra)
            assert status == 200, (status, body[:200])
            tar = tarfile.open(fileobj=io.BytesIO(body), mode="r:gz")
            got = {}
            for member in tar.getmembers():
                if member.isfile():
                    got[member.name.removeprefix("./")] = hashlib.sha256(tar.extractfile(member).read()).hexdigest()
            assert got == oracle, {"missing": sorted(set(oracle) - set(got)), "extra": sorted(set(got) - set(oracle)),
                                    "differ": sorted(k for k in got if k in oracle and got[k] != oracle[k])}
            return {"files": len(got), "archive_bytes": len(body)}
        case("an archive of /etc/cloud equals the mounted disk, file for file", archived)
        case(f"naming the root partition (number {number}) gives the same archive", lambda: archived(f"&partition={number}"))

        def refused():
            other = next(int(p.name.rsplit("p", 1)[1]) for p in partitions if p != root)
            results = {}
            for label, extra in [("another partition", f"&partition={other}"), ("no such partition", "&partition=99")]:
                status, _, body = raw("GET", f"/machines/{NAME}/snapshots/{sid}/archive?" + urllib.parse.urlencode(
                    {"path": "/etc/cloud"}) + extra)
                assert status in (400, 404, 422), (label, status, body[:200])
                results[label] = status
            return results
        case("a partition that is not the root filesystem, or absent, is refused", refused)

        def probed():
            kvm = {"ioctls": probe.split("== kvm ioctls")[1].split("== dmesg")[0].strip(),
                   "svm_in_guest": "svm" in probe.split("== /dev/kvm before")[0],
                   "dev_kvm_before": probe.split("== /dev/kvm before")[1].split("== modprobe")[0].strip(),
                   "dev_kvm_after": probe.split("== /dev/kvm after")[1].split("== dmesg")[0].strip()}
            report["dev_kvm_in_guest"] = kvm
            # On a host whose CPU and hypervisor pass nested virtualization through (this
            # one: AMD under WSL2), a guest of the machine runs a guest of its own.
            assert "KVM_RUN exit_reason 2" in probe and "second exit_reason 5" in probe, kvm["ioctls"]
            return kvm
        case("what the guest saw of /dev/kvm on first boot was read back through the snapshot", probed)
        report["success"] = True
    finally:
        subprocess.run(["umount", str(mount)], capture_output=True)
        if loop:
            subprocess.run(["losetup", "-d", loop], capture_output=True)
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
