#!/usr/bin/env python3
"""Volume quotas and the host-disk reserve guard (O7), from inside a guest, on real KVM.

Three owned hv2-sandboxd daemons run in turn from one binary and one guest image:

  quota    --volume-max-bytes 8 MiB --volume-max-inodes 24, volume dir on a plain directory.
           a. a volume mounts over 9P and works (write, read back by SHA-256, a second sandbox reads it)
           b. a guest write past the byte quota stops with EDQUOT at the limit
           c. deleting frees quota: the same write works again
           d. creates stop at the inode quota with EDQUOT
           e. deleting entries frees inodes
  guard    --volume-dir on a 64 MiB tmpfs, default reserve (5% = ~3.2 MiB). A host-side filler leaves
           ~9 MiB free; the guest writes 32 MiB and must be stopped with ENOSPC while the *host* still
           has at least the reserve free. Deleting the guest's file lets it write again.
  noguard  the same with --volume-reserve-percent 0: the guest runs the tmpfs to (nearly) 0 free.

Every guest command is run through POST /sandboxes/{id}/exec; exit codes and stderr are recorded.
Writes report.json and daemon logs into --output, with the number of `Kvm` and `Tcg` platform lines.
"""
import argparse, glob, hashlib, json, os, shutil, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

MIB = 1 << 20
QUOTA_BYTES = 8 * MIB
QUOTA_INODES = 24


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tree(root):
    """(apparent bytes of regular files, entries) under root, as the host sees them."""
    size = entries = 0
    for d, dirs, files in os.walk(root):
        entries += len(dirs) + len(files)
        for f in files:
            size += os.lstat(os.path.join(d, f)).st_size
    return size, entries


class Node:
    def __init__(self, args, work, label, extra):
        self.label = label
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
        self.log_path = work / f"daemon-{label}.log"
        self.log = self.log_path.open("wb")
        self.process = subprocess.Popen(
            [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()), "--memory-mb", "512",
             "--cpu-cores", "1", "--capacity", "4", "--disk-dir", str(work / f"disks-{label}")] + extra,
            env=env, stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT)

    def request(self, method, path, body=None, timeout=180):
        req = urllib.request.Request(self.base + path, method=method, headers={"content-type": "application/json"},
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

    def ready(self):
        deadline = time.monotonic() + 300
        while True:
            assert self.process.poll() is None, f"{self.label} exited"
            try:
                status, templates = self.request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

    def volume(self, name):
        status, value = self.request("POST", "/volumes", {"name": name})
        assert status in (200, 201), (status, value)
        return value

    def create(self, volume, path="/vol"):
        status, value = self.request("POST", "/sandboxes", {"timeout": 900, "volumeMounts": [{"name": volume, "path": path}]})
        assert status == 201, f"create answered {status}: {value}"
        return value["sandboxID"]

    def sh(self, sandbox, line, timeout=120):
        """Run a shell line in the guest; returns (exit_code, stdout, stderr). Never asserts."""
        status, value = self.request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": line, "timeout_secs": timeout})
        assert status == 200, (status, value)
        return value["exit_code"], value.get("stdout", ""), value.get("stderr", "")

    def delete(self, sandbox):
        status, _ = self.request("DELETE", f"/sandboxes/{sandbox}")
        assert status == 204, status

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for n in ["daemon", "kernel", "initrd", "output"]:
        p.add_argument("--" + n, type=Path, required=True)
    p.add_argument("--base", type=Path, default=Path("/var/tmp/hm-o7"))
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    work = args.output
    report = {"success": False, "purpose": "functional verification of O7 volume quotas and the reserve guard from inside a guest, on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "limits": {"max_bytes": QUOTA_BYTES, "max_inodes": QUOTA_INODES}, "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    failures = []

    def case(name, row):
        ok = row.pop("ok")
        row = {"name": name, "ok": ok, **row}
        report["cases"].append(row)
        if not ok:
            failures.append(name)
        print(("ok: " if ok else "FAIL: ") + name, json.dumps(row)[:400], flush=True)

    def last(s):
        return (s or "").strip().splitlines()[-1:] and (s or "").strip().splitlines()[-1] or ""

    nodes = []
    mounts = []
    try:
        # ---------------- quota ----------------
        qdir = args.base / "quota-vols"
        shutil.rmtree(qdir, ignore_errors=True)
        qdir.mkdir(parents=True)
        node = Node(args, work, "quota", ["--volume-dir", str(qdir), "--volume-max-bytes", str(QUOTA_BYTES),
                                          "--volume-max-inodes", str(QUOTA_INODES)])
        nodes.append(node)
        node.ready()
        node.volume("q")
        data = glob.glob(str(qdir / "*" / "data"))
        assert len(data) == 1, (data, os.listdir(qdir))
        data = data[0]
        sb = node.create("q")
        rc, out, err = node.sh(sb, "grep ' /vol ' /proc/mounts")
        mount_line = out.strip()
        rc2, o2, e2 = node.sh(sb, "dd if=/dev/urandom of=/vol/small bs=64k count=16 conv=fsync 2>&1; echo rc=$?; sha256sum /vol/small")
        sha_w = [l for l in o2.splitlines() if "/vol/small" in l][0].split()[0]
        node.delete(sb)
        sb2 = node.create("q")
        rc3, o3, e3 = node.sh(sb2, "sha256sum /vol/small")
        sha_r = o3.split()[0] if o3.strip() else ""
        case("a. volume works: 9p mount, 1 MiB write read back, a second sandbox reads the same bytes",
             {"ok": "9p" in mount_line and rc == 0 and "rc=0" in o2 and sha_w == sha_r and tree(data)[0] == MIB,
              "mount": mount_line, "sha256": sha_w, "second_sandbox_sha256": sha_r, "host_bytes": tree(data)[0]})

        # b. byte quota
        rc, out, err = node.sh(sb2, "dd if=/dev/zero of=/vol/big bs=64k count=256 conv=fsync; echo rc=$?; ls -l /vol/big | awk '{print $5}'")
        hb, he = tree(data)
        guest_size = int(last(out)) if last(out).isdigit() else None
        case("b. a write past the byte quota stops with EDQUOT at the limit (1 MiB used + big file; limit 8 MiB)",
             {"ok": "Disk quota exceeded" in (out + err) and "rc=0" not in out and hb <= QUOTA_BYTES and hb >= QUOTA_BYTES - 128 * 1024,
              "exit_code": rc, "dd_output": (out + err).strip()[-400:], "guest_big_size": guest_size, "host_total_bytes": hb,
              "limit": QUOTA_BYTES})
        # one more byte-grow should also fail
        rc, out, err = node.sh(sb2, "dd if=/dev/zero of=/vol/more bs=64k count=4 2>&1; echo rc=$?")
        case("b2. at the limit a further write is refused too",
             {"ok": "Disk quota exceeded" in out, "output": out.strip()[-300:], "host_total_bytes": tree(data)[0]})

        # c. delete frees bytes
        rc, out, err = node.sh(sb2, "rm -f /vol/big /vol/more; sync; dd if=/dev/zero of=/vol/again bs=64k count=64 conv=fsync 2>&1; echo rc=$?; ls -l /vol/again | awk '{print $5}'")
        case("c. deleting frees byte quota: after rm, a 4 MiB write succeeds",
             {"ok": "rc=0" in out and last(out) == str(4 * MIB) and tree(data)[0] == 5 * MIB,
              "output": out.strip()[-300:], "host_total_bytes": tree(data)[0]})
        node.sh(sb2, "rm -f /vol/again")

        # d. inode quota
        rc, out, err = node.sh(sb2, "n=0; i=0; while [ $i -lt 60 ]; do if touch /vol/f$i 2>/tmp/e; then n=$((n+1)); else cat /tmp/e; break; fi; i=$((i+1)); done; echo created=$n")
        hb, entries = tree(data)
        created = int(last(out).split("=")[1])
        case(f"d. creates stop at the inode quota ({QUOTA_INODES} entries) with EDQUOT",
             {"ok": "Disk quota exceeded" in out and entries == QUOTA_INODES and created == QUOTA_INODES - 1,
              "output": out.strip()[-300:], "created_after_small": created, "host_entries": entries, "limit": QUOTA_INODES})
        rc, out, err = node.sh(sb2, "mkdir /vol/d 2>&1; echo rc=$?; ln -s x /vol/l 2>&1; echo rc=$?")
        case("d2. mkdir and symlink are refused at the inode quota too",
             {"ok": out.count("Disk quota exceeded") == 2, "output": out.strip()[-300:]})

        # e. delete frees inodes
        rc, out, err = node.sh(sb2, "rm -f /vol/f0 /vol/f1 /vol/f2 /vol/f3 /vol/f4; sync; for i in a b c d e; do touch /vol/new$i 2>&1 || echo FAILED $i; done; touch /vol/new_f 2>&1; echo done")
        hb, entries = tree(data)
        case("e. deleting frees inodes: after rm of 5 entries, 5 creates succeed and the 6th is refused",
             {"ok": "FAILED" not in out and out.count("Disk quota exceeded") == 1 and entries == QUOTA_INODES,
              "output": out.strip()[-300:], "host_entries": entries})
        node.delete(sb2)
        node.stop()

        # ---------------- guard / noguard ----------------
        for label, extra, fill_expect_guarded in [("guard", [], True), ("noguard", ["--volume-reserve-percent", "0"], False)]:
            mp = args.base / f"tmpfs-{label}"
            shutil.rmtree(mp, ignore_errors=True)
            mp.mkdir(parents=True)
            subprocess.run(["mount", "-t", "tmpfs", "-o", "size=64m", "tmpfs", str(mp)], check=True)
            mounts.append(mp)
            vdir = mp / "vols"
            node = Node(args, work, label, ["--volume-dir", str(vdir)] + extra)
            nodes.append(node)
            node.ready()
            node.volume("g")
            data = glob.glob(str(vdir / "*" / "data"))[0]
            # the filler sits outside the volume so only the host's free space shrinks
            def free():
                s = os.statvfs(mp)
                return s.f_bavail * s.f_frsize, s.f_bfree * s.f_frsize, s.f_bavail, s.f_files and s.f_ffree
            total = os.statvfs(mp).f_blocks * os.statvfs(mp).f_frsize
            filler = mp / "filler"
            with filler.open("wb") as f:
                target = total - 9 * MIB - (total - free()[0])
                f.write(b"\0" * int(target))
            start_free = free()[0]
            reserve = min(int(total * 0.05), 2 * 1024 * MIB)
            sb = node.create("g")
            rc, out, err = node.sh(sb, "dd if=/dev/zero of=/vol/fill bs=64k count=512 conv=fsync 2>&1; echo rc=$?; ls -l /vol/fill | awk '{print $5}'")
            end_free = free()[0]
            if fill_expect_guarded:
                ok = ("No space left" in out) and "rc=0" not in out and end_free >= reserve * 0.9
            else:
                ok = ("No space left" in out) and "rc=0" not in out and end_free < reserve // 2
            case(f"{'f' if fill_expect_guarded else 'h'}. {label}: guest writes 32 MiB onto a tmpfs with ~9 MiB free; "
                 + ("refused with ENOSPC while the host keeps its reserve" if fill_expect_guarded else "reserve off: the guest runs the filesystem to ~0 free"),
                 {"ok": ok, "output": out.strip()[-300:], "exit_code": rc, "tmpfs_total": total, "reserve_bytes": reserve if fill_expect_guarded else 0,
                  "host_free_before": start_free, "host_free_after": end_free, "guest_file_size": last(out)})
            if fill_expect_guarded:
                rc, out, err = node.sh(sb, "rm -f /vol/fill; sync; dd if=/dev/zero of=/vol/ok bs=64k count=32 conv=fsync 2>&1; echo rc=$?")
                time.sleep(0)
                case("g. guard: after deleting the guest's file, a 2 MiB write succeeds again",
                     {"ok": "rc=0" in out, "output": out.strip()[-300:], "host_free": free()[0]})
            node.delete(sb)
            node.stop()
            subprocess.run(["umount", str(mp)], check=True)
            mounts.remove(mp)
    except Exception as e:  # noqa
        import traceback
        report["error"] = traceback.format_exc()
        failures.append("exception")
    finally:
        for n in nodes:
            n.stop()
        for mp in mounts:
            subprocess.run(["umount", str(mp)])
    kvm = tcg = 0
    for log in glob.glob(str(work / "daemon-*.log")):
        text = Path(log).read_text(errors="replace")
        kvm += text.count("platform: Kvm") + text.count("platform: KVM")
        tcg += text.count("platform: Tcg")
    report["platform_lines"] = {"Kvm": kvm, "Tcg": tcg}
    report["success"] = not failures and tcg == 0 and kvm > 0
    report["failures"] = failures
    (work / "report.json").write_text(json.dumps(report, indent=2))
    print("success:", report["success"], "kvm", kvm, "tcg", tcg, "failures", failures)


main()
