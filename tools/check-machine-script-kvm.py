#!/usr/bin/env python3
"""Run a script as root in a throwaway Ubuntu machine, on real KVM.

For a destructive verification script that must not run on the host: it
installs units, wipes state and flushes the firewall. The machine is made
from a distribution's cloud image with the guest agent and a NIC behind the
node's egress gateway, the files go in through the machine's `/files` route,
and the script runs through `exec`. The machine is deleted afterwards.

1. A machine from the image boots and `exec` answers as root, with systemd as
   its first process.
2. A tar.gz of `--bundle` (a directory, kept with its relative paths) and of
   `--bins` (put at `--bins-dest`) is written into the guest and unpacked.
3. `--apt` packages are installed from the distribution's archive. This is the
   only thing that reaches the Internet.
4. `--script` (a path inside the bundle) runs with `--args`; its standard output,
   standard error and exit code are written to --output and are the result.
5. The machine is stopped and deleted.

The script's own verdict is not judged here: the report says what it printed
and the exit code it ended with. Writes report.json, script-stdout.txt,
script-stderr.txt and the node's log into --output.
"""
import argparse, hashlib, io, json, os, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

NAME = "script-01"
SKIP = {".git", "target", "node_modules"}


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def pack(entries):
    """A tar.gz of [(path on disk, name in the archive)], directories walked."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for source, name in entries:
            source = Path(source)
            if source.is_dir():
                for path in sorted(source.rglob("*")):
                    if SKIP & set(path.relative_to(source).parts):
                        continue
                    tar.add(path, arcname=f"{name}/{path.relative_to(source).as_posix()}", recursive=False)
            else:
                tar.add(source, arcname=name, recursive=False)
    return buffer.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "firmware", "image", "guest-agent", "bundle", "output", "kernel", "initrd"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--bins", type=Path, help="a directory of binaries, put at --bins-dest")
    parser.add_argument("--bins-dest", default="/opt/bins")
    parser.add_argument("--bundle-dest", default="/root/bundle")
    parser.add_argument("--script", required=True, help="path of the script inside the bundle")
    parser.add_argument("--args", default="", help="arguments for the script")
    parser.add_argument("--apt", default="", help="packages to install first")
    parser.add_argument("--cpus", type=int, default=2)
    parser.add_argument("--disk-gib", type=int, default=6)
    parser.add_argument("--script-seconds", type=int, default=1200)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "a destructive script run in a throwaway Ubuntu machine on KVM; the script's verdict is its own",
              "input_sha256": {k: digest(getattr(args, k.replace("-", "_")))
                               for k in ["daemon", "firmware", "image", "guest-agent"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    report["apt_packages"] = args.apt
    if args.bins:
        report["bins_sha256"] = {p.name: digest(p) for p in sorted(args.bins.iterdir()) if p.is_file()}
    work = Path(tempfile.mkdtemp(prefix="hm-machine-script-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(images / "distro.raw")], check=True)
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4", "--guest-agent", str(args.guest_agent)],
        env=dict(os.environ, RUST_LOG="info", HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd)),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, raw=None, timeout=120):
        data = raw if raw is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(base + path, method=method, data=data,
                                     headers={"content-type": "application/json"})
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            text = response.read()
            try:
                return response.status, json.loads(text) if text else None
            except ValueError:
                return response.status, text.decode(errors="replace")

    def run(command, seconds=60):
        return request("POST", f"/machines/{NAME}/exec", {"cmd": command, "timeout_secs": seconds},
                       timeout=seconds + 60)

    def upload(guest_path, content):
        query = urllib.parse.urlencode({"path": guest_path})
        status, value = request("PUT", f"/machines/{NAME}/files?{query}", raw=content, timeout=300)
        assert status == 204, (status, value)

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
            time.sleep(0.1)

        def boots():
            body = {"name": NAME, "image": "distro.raw", "cpuCount": args.cpus, "memoryMB": 2048,
                    "diskGiB": args.disk_gib, "guestAgent": True, "network": {"allowInternetAccess": True}}
            status, created = request("POST", "/machines", body)
            assert status == 201, (status, created)
            started = time.monotonic()
            refusals = 0
            while True:
                status, value = run("true", 30)
                if status == 200 and value["exit_code"] == 0:
                    break
                assert status == 409 and "no guest agent answering" in value["message"], (status, value)
                refusals += 1
                assert time.monotonic() - started < 600, f"exec still refused: {value}"
                time.sleep(3)
            found = {"seconds_until_exec_answered": round(time.monotonic() - started, 1), "refused_before_that": refusals}
            for key, cmd in [("whoami", "id -un"), ("pid_1", "cat /proc/1/comm"), ("kernel", "uname -r"),
                             ("os", ". /etc/os-release; echo $PRETTY_NAME")]:
                found[key] = run(cmd)[1]["stdout"].strip()
            assert found["whoami"] == "root" and found["pid_1"] == "systemd", found
            return found
        case("a machine from the image boots; exec answers as root with systemd as the first process", boots)

        def puts_files_in():
            entries = [(args.bundle, "bundle")]
            archive = pack(entries)
            upload("/root/bundle.tar.gz", archive)
            found = {"bundle_tar_bytes": len(archive)}
            status, value = run(f"mkdir -p {args.bundle_dest} && tar -xzf /root/bundle.tar.gz -C /root && "
                                f"test -f {args.bundle_dest}/{args.script} && echo there")
            assert status == 200 and value["stdout"].strip() == "there", (status, value)
            if args.bins:
                bins = pack([(p, p.name) for p in sorted(args.bins.iterdir()) if p.is_file()])
                upload("/root/bins.tar.gz", bins)
                found["bins_tar_bytes"] = len(bins)
                status, value = run(f"mkdir -p {args.bins_dest} && tar -xzf /root/bins.tar.gz -C {args.bins_dest} "
                                    f"&& chmod +x {args.bins_dest}/* && ls {args.bins_dest}")
                assert status == 200 and value["exit_code"] == 0, (status, value)
                found["bins"] = value["stdout"].split()
            return found
        case("the bundle and the binaries are written into the guest and unpacked", puts_files_in)

        def installs():
            if not args.apt:
                return {"skipped": "no packages asked for"}
            status, value = run(f"DEBIAN_FRONTEND=noninteractive apt-get update -qq && "
                                f"DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {args.apt}", 900)
            assert status == 200 and value["exit_code"] == 0, (status, value)
            status, value = run("dpkg-query -W -f='${Package} ${Version}\\n' " + args.apt)
            return {"installed": value["stdout"].split("\n")[:-1], "reached": "the distribution's apt archive"}
        case("the packages are installed from the distribution's archive", installs)

        def runs_script():
            script = f"{args.bundle_dest}/{args.script}"
            began = time.monotonic()
            status, value = run(f"cd {args.bundle_dest} && bash {script} {args.args}", args.script_seconds)
            took = round(time.monotonic() - began, 1)
            assert status == 200, (status, value)
            (args.output / "script-stdout.txt").write_text(value["stdout"])
            (args.output / "script-stderr.txt").write_text(value["stderr"])
            lines = value["stdout"].splitlines()
            return {"exit_code": value["exit_code"], "seconds": took,
                    "pass_lines": sum(1 for l in lines if l.lstrip().startswith("PASS")),
                    "fail_lines": sum(1 for l in lines if l.lstrip().startswith("FAIL")),
                    "last_line": lines[-1] if lines else ""}
        case("the script ran to an exit; what it printed is in script-stdout.txt", runs_script)

        def deletes():
            status, value = request("POST", f"/machines/{NAME}/stop")
            assert status in (200, 202, 204), (status, value)
            deadline = time.monotonic() + 120
            while True:
                status, value = request("GET", f"/machines/{NAME}")
                if status == 200 and value["state"] != "running":
                    break
                assert time.monotonic() < deadline, f"machine still {value}"
                time.sleep(2)
            status, value = request("DELETE", f"/machines/{NAME}")
            assert status in (200, 202, 204), (status, value)
            status, _ = request("GET", f"/machines/{NAME}")
            assert status == 404, status
            return {"deleted": True}
        case("the machine is stopped and deleted", deletes)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
