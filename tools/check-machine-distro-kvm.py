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

`--user` must be able to log in on the serial console with `--password`. A
cloud image has no account until cloud-init gives it one, so there are two
ways to run this:

- With `--user-data FILE`, on the distribution's image as published. The
  file is given to the machine as its cloud-init user data and must set that
  password. Two more cases then check that the guest read it: the machine's
  name is its hostname, its ID is cloud-init's instance ID, and cloud-init
  finished.
- Without it, on an image that was given the account by hand.

`--sudo` is for a user that is not root: it is put in front of what needs to
be.

With `--network` the machine is created with a NIC behind the node's egress
gateway and the Internet allowed, and three more cases run: the guest
configured itself by DHCP and resolves a name; `apt-get` installs Docker from
the distribution's archive; and Docker pulls an image and runs a container.
Those reach the Internet, so they are for a run by hand and say what they
reached. `--disk-gib` grows the root disk, which an install needs.

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
    parser.add_argument("--user-data", type=Path, help="cloud-init user data to create the machine with")
    parser.add_argument("--sudo", action="store_true", help="the user is not root; use sudo where root is needed")
    parser.add_argument("--network", action="store_true",
                        help="give the machine a NIC with the Internet allowed, and install and run Docker")
    parser.add_argument("--disk-gib", type=int, help="the root disk's size, when larger than the image")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a distribution image as a machine on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "firmware", "image"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    sudo = "sudo " if args.sudo else ""
    user_data = args.user_data.read_text() if args.user_data else None
    report["cloud_init_user_data_given"] = user_data is not None
    report["network_given"] = args.network
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
        # The prompt can be there before the account is: cloud-init sets the
        # password while the system finishes starting. A refused login is
        # tried again, for up to four minutes, and each try is counted.
        deadline = time.monotonic() + 240
        tries = 0
        while True:
            # systemd goes on printing after the prompt; let it finish its line.
            time.sleep(3)
            tries += 1
            type_line(args.user)
            wait_for("Password:", 30, tail=300)
            type_line(args.password)
            answered = time.monotonic() + 30
            while time.monotonic() < answered:
                seen = console(300)
                # After the password: the shell's prompt, or a refusal and
                # the login prompt again.
                after = seen.rsplit("Password:", 1)[-1]
                if "incorrect" in after or after.rstrip().endswith(args.prompt.rstrip()):
                    break
                time.sleep(1)
            if "incorrect" not in after and after.rstrip().endswith(args.prompt.rstrip()):
                return tries
            assert time.monotonic() < deadline, f"could not log in after {tries} tries; console ends: {seen[-300:]!r}"
            time.sleep(10)
            wait_for("login:", 60, tail=200)

    asked = [0]

    def ask(command, key, seconds=60):
        """What the guest's shell answers to `command`, between two markers it computes."""
        # The sums are done by the guest's shell, so the markers appear only
        # in its output and never in the command as the terminal echoed it.
        # Each question has a number of its own, so an answer still on the
        # console from the question before is not taken for this one.
        asked[0] += 1
        tag = f"{key}{asked[0]}"
        type_line(f"echo {tag}-$((20+1))-[$({command})]-$((30+3))-{tag}")
        seen = wait_for(f"]-33-{tag}", seconds, tail=4000)
        return re.findall(re.escape(tag) + r"-21-\[(.*?)\]-33-" + re.escape(tag), seen, re.S)[-1].strip()

    machine = {}

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
            body = {"name": NAME, "image": "distro.raw", "cpuCount": args.cpus, "memoryMB": 2048}
            if user_data is not None:
                body["userData"] = user_data
            if args.network:
                body["network"] = {"allowInternetAccess": True}
            if args.disk_gib:
                body["diskGiB"] = args.disk_gib
            status, created = request("POST", "/machines", body)
            assert status == 201, (status, created)
            assert created["cloudInit"] is (user_data is not None), created
            # What was given is not handed back with the machine's record.
            assert "userData" not in created and (user_data is None or user_data not in json.dumps(created))
            machine["id"] = created["machineID"]
            assert created["state"] == "running" and created["boot"] == "firmware", created
            assert created["cpuCount"] == args.cpus, created
            started = time.monotonic()
            seen = wait_for("login:", 300)
            return {"cpuCount": created["cpuCount"], "memoryMB": created["memoryMB"],
                    "diskGiB": created["diskGiB"], "seconds_to_login_prompt": round(time.monotonic() - started, 1),
                    "prompt": [line for line in seen.replace("\r", "\n").split("\n") if line.strip()][-1]}
        case("a machine from a distribution's image boots by firmware to its login prompt", boots)

        def what_it_runs_on():
            tries = log_in()
            found = {
                "logins_tried": tries,
                "os": ask(". /etc/os-release; echo $PRETTY_NAME", "os"),
                "kernel": ask("uname -r", "kernel"),
                "nproc": int(ask("nproc", "nproc")),
                "processors_in_cpuinfo": int(ask("grep -c ^processor /proc/cpuinfo", "cpuinfo")),
                "pid_1": ask("cat /proc/1/comm", "init"),
                "systemd": ask("systemctl --version | head -n1", "sdver"),
                "acpi_tables_in_kernel_log": int(ask(sudo + "dmesg | grep -c 'ACPI: \\(FACP\\|APIC\\|DSDT\\) '", "tables")),
                "acpi_errors_in_kernel_log": int(ask(sudo + "dmesg | grep -ci 'ACPI.*\\(Error\\|AE_\\)'", "faults")),
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

        def read_its_seed():
            # cloud-init goes on after the login prompt; wait for it to say
            # it is done, and take "error" as an answer too rather than wait.
            deadline = time.monotonic() + 240
            while True:
                status_line = ask("cloud-init status 2>/dev/null | head -n1", "cistatus")
                if "done" in status_line or "error" in status_line or time.monotonic() > deadline:
                    break
                time.sleep(5)
            found = {
                "cloud_init_status": status_line,
                "hostname": ask("hostname", "hostname"),
                "instance_id": ask("cat /var/lib/cloud/data/instance-id", "iid"),
                "datasource": ask("cloud-id 2>/dev/null || true", "cloudid"),
                "seed_disk_label": ask("lsblk -ndo LABEL /dev/vdb 2>/dev/null || true", "label"),
                "seed_disk_read_only": ask("cat /sys/block/vdb/ro 2>/dev/null || true", "ro"),
                "disks": ask("lsblk -ndo NAME,SIZE | tr '\\n' ' '", "disks"),
            }
            assert "done" in found["cloud_init_status"], found
            assert found["hostname"] == NAME, found
            assert found["instance_id"] == machine["id"], (found, machine)
            assert found["datasource"] == "nocloud", found
            assert found["seed_disk_label"].lower() == "cidata" and found["seed_disk_read_only"] == "1", found
            return found
        if user_data is not None:
            case("cloud-init read the machine's seed: its name, its ID, and the user data that let this log in",
                 read_its_seed)

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

        def has_a_network():
            found = {
                "address": ask("ip -4 -o addr show scope global | awk '{print $4}' | head -n1", "addr"),
                "default_route": ask("ip route show default | awk '{print $3}' | head -n1", "route"),
                "resolves_archive_ubuntu_com": ask(
                    "getent ahostsv4 archive.ubuntu.com | awk '{print $1}' | head -n1", "dns"),
                "root_filesystem": ask("df -h --output=size,avail / | tail -n1 | tr -s ' '", "df"),
            }
            assert found["address"] and found["default_route"], found
            assert found["resolves_archive_ubuntu_com"], found
            return found
        if args.network:
            case("with a NIC the guest configured itself by DHCP and resolves a name", has_a_network)

        def installs_docker():
            started = time.monotonic()
            updated = ask(sudo + "apt-get update -qq >/tmp/apt-update.log 2>&1; echo $?", "aptupdate", 600)
            assert updated == "0", (updated, ask("tail -n 5 /tmp/apt-update.log | tr '\\n' '|'", "aptlog"))
            installed = ask(
                sudo + "env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io "
                ">/tmp/apt-install.log 2>&1; echo $?", "aptinstall", 1500)
            assert installed == "0", (installed, ask("tail -n 8 /tmp/apt-install.log | tr '\\n' '|'", "aptlog2"))
            found = {
                "seconds": round(time.monotonic() - started),
                "package": ask("dpkg-query -W -f '${Package} ${Version}' docker.io", "dpkg"),
                "docker_service": ask("systemctl is-active docker", "dockeractive", 120),
            }
            assert found["docker_service"] == "active", found
            return found
        if args.network:
            case("apt-get installs Docker from the distribution's archive, and its service starts",
                 installs_docker)

        def runs_a_container():
            started = time.monotonic()
            info = ask(sudo + "docker info --format '{{.ServerVersion}} {{.Driver}} cgroup-v{{.CgroupVersion}}'",
                       "dockerinfo", 120)
            hello = ask(sudo + "docker run --rm hello-world 2>/tmp/docker-run.log | grep -c 'Hello from Docker'",
                        "hello", 600)
            assert hello == "1", (hello, ask("tail -n 5 /tmp/docker-run.log | tr '\\n' '|'", "dockerlog"))
            # A container that is not a single static binary: a shell in a
            # distribution's image, with a namespace of its own.
            inside = ask(sudo + "docker run --rm alpine:3 sh -c 'echo $(cat /etc/alpine-release | cut -d. -f1)-pid-$$'",
                         "alpine", 600)
            assert inside.endswith("-pid-1"), inside
            return {"docker": info, "hello_world": "Hello from Docker", "alpine_said": inside,
                    "seconds": round(time.monotonic() - started)}
        if args.network:
            case("Docker pulls an image and runs a container", runs_a_container)

        def turns_itself_off():
            ask("echo kept-$((6*7)) > ~/note; sync; cat ~/note", "note")
            before = state()
            type_line(sudo + "systemctl poweroff")
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
            note = ask("cat ~/note", "back")
            assert note == "kept-42", note
            again = int(ask("nproc", "again"))
            assert again == args.cpus, again
            found = {"note": note, "nproc": again}
            if user_data is not None:
                # The same instance: cloud-init's once-per-instance steps did
                # not run a second time, so the same ID is still recorded.
                found["instance_id"] = ask("cat /var/lib/cloud/data/instance-id", "iid2")
                found["boots_recorded"] = ask(
                    sudo + "sh -c 'ls /var/lib/cloud/instances | wc -l'", "instances")
                assert found["instance_id"] == machine["id"] and found["boots_recorded"] == "1", found
            return found
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
