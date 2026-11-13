#!/usr/bin/env python3
"""Mounting a block disk in a guest through the agent's mount_block, on real KVM.

Runs two owned hv2-sandboxd daemons from one binary and one guest image, alike
except that one has `--disk-slot`, and checks through the API the two node paths
that mount a disk in the guest:

a. No slot: a sandbox created with a disk cold-boots with it behind /dev/vda and
   the node mounts it (`disks::mount`). It is mounted ext4 at /data, a 4 MiB
   random file written there reads back by SHA-256, and a second sandbox that
   attaches the same disk reads the same bytes.
b. With the slot: the sandbox is restored from the template and the real disk is
   swapped in behind its placeholder (`disks::mount_swapped`: wait for the guest
   to see the new size, drop caches, mount). The guest's disk is the real size
   (not the 1 MiB placeholder), and the same write / re-attach round trip holds.

--agent-ops says which code path the image must take: `new` (the agent has
mount_block: the daemon log must show "using agent op" for every mount and never
"falling back to shell") or `old` (an agent from before it: the log must show the
fallback for every mount and never "using agent op").

--shell says how the guest is driven. `yes`: the image has /bin/sh and the check
runs shell lines. `no`: there is no shell and no /bin/busybox; the image carries
busybox applets installed under the names cat, dd, sha256sum, grep and ls (build.sh
--extra), and the check runs them by argv (exec's `argv`, no shell anywhere). The
old fallback script needs /bin/busybox and a shell, so on this image only the agent
op can succeed.

Writes report.json and daemon-log.txt (both daemons' logs) into --output, with the
number of `Kvm` and `Tcg` platform lines and of each code path's log lines.
"""
import argparse, hashlib, json, os, shutil, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

SECTORS = (64 << 20) // 512
PLACEHOLDER_SECTORS = (1 << 20) // 512
MOUNT_LINE = "mounting the disk at"
SWAPPED_LINE = "mounting the swapped disk at"
AGENT_LOG = "using agent op"
SHELL_LOG = "falling back to shell"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Node:
    def __init__(self, args, work, label, extra):
        self.label = label
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        rust_log = "info,hv2_sandboxd::disks=debug"
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG=rust_log)
        self.log_path = work / f"{label}.log"
        self.log = self.log_path.open("wb")
        self.process = subprocess.Popen(
            [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()), "--memory-mb", "512",
             "--cpu-cores", "1", "--capacity", "8", "--disk-dir", str(work / f"disks-{label}")] + extra,
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

    def disk(self, name, mib=64):
        status, value = self.request("POST", "/disks", {"name": name, "sizeMiB": mib})
        assert status == 201, (status, value)
        return value["diskID"]

    def create(self, disk, path="/data"):
        status, value = self.request("POST", "/sandboxes", {"timeout": 900, "diskMount": {"name": disk, "path": path}})
        assert status == 201, f"create with a disk answered {status}: {value}"
        return value["sandboxID"]

    def run(self, sandbox, shell_line, argv):
        """Run one command in the guest: a shell line, or an argv with no shell."""
        body = {"timeout_secs": 60}
        body.update({"cmd": shell_line} if argv is None else {"argv": argv})
        status, value = self.request("POST", f"/sandboxes/{sandbox}/exec", body)
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

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
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--agent-ops", choices=["new", "old"], required=True)
    parser.add_argument("--shell", choices=["yes", "no"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    has_shell = args.shell == "yes"
    report = {"success": False, "label": args.label, "agent_ops": args.agent_ops, "shell": has_shell,
              "purpose": "functional verification of mounting a disk through the agent's mount_block, on KVM; "
                         "no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-disk-mount-check-", dir="/var/tmp"))
    plain = Node(args, work, "no-slot", [])
    slot = Node(args, work, "with-slot", ["--disk-slot"])
    nodes = [plain, slot]

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(node, sandbox, line, argv):
        return node.run(sandbox, line if has_shell else None, None if has_shell else argv)

    def round_trip(node, disk_name):
        """Mount, write a file, end, mount again from a new sandbox and read it back."""
        first = node.create(disk_name)
        mounts = sh(node, first, "grep ' /data ' /proc/mounts", ["/bin/grep", " /data ", "/proc/mounts"])
        assert mounts.startswith("/dev/vda /data ext4"), mounts
        sectors = int(sh(node, first, "cat /sys/block/vda/size", ["/bin/cat", "/sys/block/vda/size"]))
        sh(node, first, "dd if=/dev/urandom of=/data/payload bs=1M count=4 conv=fsync 2>/dev/null",
           ["/bin/dd", "if=/dev/urandom", "of=/data/payload", "bs=1M", "count=4", "conv=fsync"])
        written = sh(node, first, "sha256sum /data/payload | cut -d' ' -f1",
                     ["/bin/sha256sum", "/data/payload"]).split()[0]
        evidence = {"mount": mounts, "disk_sectors": sectors, "sha256": written}
        if has_shell:
            evidence["kernel_resize_lines"] = int(node.run(
                first, "dmesg | grep -i 'vda' | grep -ic 'new size' || true", None))
        node.delete(first)
        second = node.create(disk_name)
        read = sh(node, second, "sha256sum /data/payload | cut -d' ' -f1",
                  ["/bin/sha256sum", "/data/payload"]).split()[0]
        assert read == written, f"the second sandbox read {read}, the first wrote {written}"
        node.delete(second)
        evidence["second_sandbox_sha256"] = read
        return evidence

    try:
        for node in nodes:
            node.ready()
        plain.disk("data")
        slot.disk("data")

        def cold():
            evidence = round_trip(plain, "data")
            assert evidence["disk_sectors"] == SECTORS, evidence
            if has_shell:
                assert evidence["kernel_resize_lines"] == 0, "a cold boot saw a resize: " + str(evidence)
            return evidence
        case("a. cold boot with a disk: mounted ext4, written file read back by a second sandbox "
             "(disks::mount)", cold)

        def swapped():
            evidence = round_trip(slot, "data")
            assert evidence["disk_sectors"] == SECTORS, evidence
            assert SECTORS != PLACEHOLDER_SECTORS
            if has_shell:
                assert evidence["kernel_resize_lines"] >= 1, \
                    "its kernel never saw the disk change size: it booted with the disk: " + str(evidence)
            return evidence
        case("b. restored from the template and given the disk: mounted ext4 at the real size, written file "
             "read back by a second sandbox (disks::mount_swapped)", swapped)
        report["success"] = True
    finally:
        for node in nodes:
            node.stop()
        text = ""
        for node in nodes:
            text += f"===== {node.label} daemon log =====\n" + node.log_path.read_text(errors="replace")
        (args.output / "daemon-log.txt").write_text(text)
        lines = text.splitlines()
        count = lambda *needles: sum(all(n in l for n in needles) for l in lines)
        counts = {
            "mount_agent": count(MOUNT_LINE, AGENT_LOG), "mount_shell": count(MOUNT_LINE, SHELL_LOG),
            "mount_swapped_agent": count(SWAPPED_LINE, AGENT_LOG),
            "mount_swapped_shell": count(SWAPPED_LINE, SHELL_LOG),
        }
        # "mounting the disk at" is not a substring of "mounting the swapped disk at".
        report["log_counts"] = counts
        report["platform_lines"] = {"Kvm": count("Detected hypervisor platform: Kvm"),
                                    "Tcg": count("Detected hypervisor platform: Tcg")}
        if report["success"]:
            expected = 2  # each node mounts twice: the writer and the second sandbox
            if args.agent_ops == "new":
                ok = counts == {"mount_agent": expected, "mount_shell": 0,
                                "mount_swapped_agent": expected, "mount_swapped_shell": 0}
            else:
                ok = counts == {"mount_agent": 0, "mount_shell": expected,
                                "mount_swapped_agent": 0, "mount_swapped_shell": expected}
            report["code_path_as_expected"] = ok
            report["success"] = ok and report["platform_lines"]["Tcg"] == 0
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("PASS" if report["success"] else "FAIL", json.dumps(report["log_counts"]), flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
