#!/usr/bin/env python3
"""A machine from a distribution's cloud image, on real KVM.

`check-machine-firmware-kvm.py` boots CirrOS, which is small and has no
systemd. This boots a full distribution's image, the thing a machine from a
disk image is for, and asks the guest what it is running on:

1. A machine created from the image with four processors and 2 GiB boots by
   firmware to the distribution's login prompt.
2. Logged in on the console, the guest counts four processors; its first
   process is systemd; and its kernel took the ACPI tables without an error.
3. systemd finished starting: no job is left queued, and the units that
   failed, if any, are named in the report rather than hidden.
4. `systemctl poweroff` stops the machine, and it is left stopped.
5. Started again, it comes back with what was written to its disk.

The image must already let `--user` log in on the serial console with
`--password`: a cloud image has no account until something gives it one, and
this node offers it no cloud-init datasource. How the image used for a
recorded run was prepared is in that run's README.

Writes report.json, the node's log and the console into --output.
"""
import argparse, hashlib, json, os, re, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "distro-01"


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "firmware", "image", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--user", default="root")
    parser.add_argument("--password", required=True)
    parser.add_argument("--prompt", default="# ", help="what the shell prompt ends with")
    parser.add_argument("--cpus", type=int, default=4)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a distribution image as a machine on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "firmware", "image"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-distro-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(images / "distro.raw")], check=True)
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, RUST_LOG="info"), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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

    def console(tail=8000):
        status, text = request("GET", f"/machines/{NAME}/console?tail={tail}")
        assert status == 200, (status, text)
        return re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text if isinstance(text, str) else json.dumps(text))

    def wait_for(text, seconds, tail=8000):
        deadline = time.monotonic() + seconds
        while True:
            try:
                seen = console(tail)
            except (OSError, AssertionError):
                seen = ""
            if text in seen:
                return seen
            assert time.monotonic() < deadline, f"no {text!r} on the console; it ends: {seen[-600:]!r}"
            time.sleep(1)

    def type_line(text):
        status, value = request("POST", f"/machines/{NAME}/console", raw=(text + "\n").encode())
        assert status == 204, (status, value)

    def log_in():
        wait_for("login:", 300, tail=600)
        # systemd goes on printing after the prompt; let it finish its line.
        time.sleep(3)
        type_line(args.user)
        wait_for("Password:", 30, tail=300)
        type_line(args.password)
        wait_for(args.prompt, 60, tail=300)

    asked = [0]

    def ask(command, key):
        """What the guest's shell answers to `command`, between two markers it computes."""
        # The sums are done by the guest's shell, so the markers appear only
        # in its output and never in the command as the terminal echoed it.
        # Each question has a number of its own, so an answer still on the
        # console from the question before is not taken for this one.
        asked[0] += 1
        tag = f"{key}{asked[0]}"
        type_line(f"echo {tag}-$((20+1))-[$({command})]-$((30+3))-{tag}")
        seen = wait_for(f"]-33-{tag}", 60, tail=2000)
        return re.findall(re.escape(tag) + r"-21-\[(.*?)\]-33-" + re.escape(tag), seen, re.S)[-1].strip()

    def state():
        status, value = request("GET", f"/machines/{NAME}")
        assert status == 200, (status, value)
        return value

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
            status, created = request("POST", "/machines", {
                "name": NAME, "image": "distro.raw", "cpuCount": args.cpus, "memoryMB": 2048})
            assert status == 201, (status, created)
            assert created["state"] == "running" and created["boot"] == "firmware", created
            assert created["cpuCount"] == args.cpus, created
            started = time.monotonic()
            seen = wait_for("login:", 300)
            return {"cpuCount": created["cpuCount"], "memoryMB": created["memoryMB"],
                    "diskGiB": created["diskGiB"], "seconds_to_login_prompt": round(time.monotonic() - started, 1),
                    "prompt": [line for line in seen.replace("\r", "\n").split("\n") if line.strip()][-1]}
        case("a machine from a distribution's image boots by firmware to its login prompt", boots)

        def what_it_runs_on():
            log_in()
            found = {
                "os": ask(". /etc/os-release; echo $PRETTY_NAME", "os"),
                "kernel": ask("uname -r", "kernel"),
                "nproc": int(ask("nproc", "nproc")),
                "processors_in_cpuinfo": int(ask("grep -c ^processor /proc/cpuinfo", "cpuinfo")),
                "pid_1": ask("cat /proc/1/comm", "init"),
                "systemd": ask("systemctl --version | head -n1", "sdver"),
                "acpi_tables_in_kernel_log": int(ask("dmesg | grep -c 'ACPI: \\(FACP\\|APIC\\|DSDT\\) '", "tables")),
                "acpi_errors_in_kernel_log": int(ask("dmesg | grep -ci 'ACPI.*\\(Error\\|AE_\\)'", "faults")),
                "virtio_on_the_io_apic": int(ask("grep -c 'IO-APIC.*virtio' /proc/interrupts", "ioapic")),
                "memory_mb": int(ask("awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo", "mem")),
            }
            assert found["nproc"] == args.cpus == found["processors_in_cpuinfo"], found
            assert found["pid_1"] == "systemd", found
            assert found["acpi_tables_in_kernel_log"] == 3 and found["acpi_errors_in_kernel_log"] == 0, found
            assert found["virtio_on_the_io_apic"] >= 1, found
            assert found["memory_mb"] > 1800, found
            return found
        case("the guest counts its processors, runs systemd as its first process, and took the ACPI tables",
             what_it_runs_on)

        def finished_starting():
            # Up to three minutes for the last jobs; a state that is neither
            # running nor degraded by then is a boot that did not finish.
            deadline = time.monotonic() + 180
            while True:
                running = ask("systemctl is-system-running 2>/dev/null || true", "sysstate")
                if running in ("running", "degraded") or time.monotonic() > deadline:
                    break
                time.sleep(5)
            jobs = int(ask("systemctl list-jobs --no-legend 2>/dev/null | wc -l", "jobs"))
            failed = ask("systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | tr '\\n' ' '",
                         "failed").split()
            assert running in ("running", "degraded"), (running, jobs, failed)
            assert jobs == 0, (running, jobs)
            return {"is_system_running": running, "queued_jobs": jobs, "failed_units": failed}
        case("systemd finished starting, and what failed is named", finished_starting)

        def turns_itself_off():
            ask("echo kept-$((6*7)) > /root/note; sync; cat /root/note", "note")
            before = state()
            type_line("systemctl poweroff")
            deadline = time.monotonic() + 120
            while state()["state"] != "stopped":
                assert time.monotonic() < deadline, "still running after systemctl poweroff"
                time.sleep(1)
            time.sleep(8)
            after = state()
            assert after["state"] == "stopped", after
            return {"restartPolicy": before.get("restartPolicy"), "seconds_left_off": 8}
        case("`systemctl poweroff` stops the machine, and it is left stopped", turns_itself_off)

        def comes_back():
            status, started = request("POST", f"/machines/{NAME}/start")
            assert status == 200 and started["state"] == "running", (status, started)
            log_in()
            note = ask("cat /root/note", "back")
            assert note == "kept-42", note
            again = int(ask("nproc", "again"))
            assert again == args.cpus, again
            return {"note": note, "nproc": again}
        case("started again, it has what was written to its disk and the same processors", comes_back)

        (args.output / "console.txt").write_text(
            "\n".join(line.rstrip() for line in console(1 << 20).replace("\r", "\n").split("\n") if line.strip()) + "\n")
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
