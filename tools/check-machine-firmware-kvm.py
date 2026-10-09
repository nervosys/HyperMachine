#!/usr/bin/env python3
"""A machine booted by firmware from a stock cloud image, on real KVM.

Runs an owned hv2-sandboxd with --firmware, an --image-dir holding the image
and a --machine-dir, then checks, through its API:

1. A machine created from the image boots by firmware and reaches the
   image's login prompt on its serial console.
2. The console takes input: logging in and running commands works, a file
   is written, and exec answers that there is no guest agent.
3. The file survives stop and start.
4. The file survives the guest rebooting itself, which the daemon answers by
   booting the machine again.
5. What cannot work is refused at creation: a network, an image that is not
   there, a path instead of a file name, an image together with a template.

The image is CirrOS, used as downloaded, so the login is its documented
default. Writes report.json, the daemon's log and the console into --output.
"""
import argparse, hashlib, json, os, re, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

USER, PASSWORD = "cirros", "gocubsgo"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    hasher = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            hasher.update(block)
    return hasher.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "firmware", "image", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of firmware-booted machines on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "firmware", "image"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-fw-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    shutil.copyfile(args.image, images / "cirros.raw")
    machines = work / "machines"
    api = port()
    base = f"http://127.0.0.1:{api}"
    # A kernel and initrd are the daemon's own requirement; no machine here
    # uses them, and --no-template keeps it from booting one.
    env = dict(os.environ, RUST_LOG="info")
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(machines), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

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
            payload = response.read()
            try:
                return response.status, json.loads(payload) if payload else None
            except ValueError:
                return response.status, payload.decode(errors="replace")

    def console(tail=6000):
        status, text = request("GET", f"/machines/cloud-01/console?tail={tail}")
        assert status == 200, (status, text)
        return re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text if isinstance(text, str) else json.dumps(text))

    def wait_for(text, seconds, tail=6000):
        deadline = time.monotonic() + seconds
        while True:
            try:
                seen = console(tail)
            except (OSError, AssertionError):
                seen = ""
            if text in seen:
                return seen
            assert time.monotonic() < deadline, f"no {text!r} on the console; it ends: {seen[-500:]!r}"
            time.sleep(1)

    def type_line(text):
        status, value = request("POST", "/machines/cloud-01/console", raw=(text + "\n").encode())
        assert status == 204, (status, value)

    def log_in():
        wait_for("login:", 240, tail=400)
        type_line(USER)
        wait_for("Password:", 30, tail=200)
        type_line(PASSWORD)
        wait_for("$ ", 30, tail=200)

    def shell(command, marker):
        # The marker is computed by the guest's shell, so finding it on the
        # console means the command ran there, not that it was echoed.
        type_line(command)
        return wait_for(marker, 30, tail=600)

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
            status, created = request("POST", "/machines", {"name": "cloud-01", "image": "cirros.raw"})
            assert status == 201, (status, created)
            assert created["state"] == "running" and created["boot"] == "firmware", created
            assert created["image"] == "cirros.raw" and created["cpuCount"] == 1, created
            seen = wait_for("login:", 240)
            for line in ["Booting with PVH Boot Protocol", "Found EFI partition", "Executable loaded"]:
                assert line in console(1 << 20), line
            return {"machineID": created["machineID"], "diskGiB": created["diskGiB"],
                    "prompt": seen.replace("\r", "\n").strip().split("\n")[-1]}
        case("a machine created from the image boots by firmware to the image's login prompt", boots)

        def console_works():
            log_in()
            seen = shell("echo kept-$((6*7)) > ~/note; sync; echo wrote-$(cat ~/note)", "wrote-kept-42")
            status, value = request("POST", "/machines/cloud-01/exec", {"cmd": "true"})
            assert status == 409 and "no guest agent" in value["message"], (status, value)
            kernel = shell("echo kernel-$(uname -r)", "kernel-5")
            # The last match: the first is the command as the terminal echoed it.
            return {"kernel": re.findall(r"kernel-(\d\S+)", kernel)[-1], "exec": status,
                    "saw": [line for line in seen.replace("\r", "\n").split("\n") if line.strip()][-2:]}
        case("the console takes input: a login, commands, and a file written", console_works)

        def survives_stop_start():
            status, stopped = request("POST", "/machines/cloud-01/stop")
            assert status == 200 and stopped["state"] == "stopped", (status, stopped)
            assert request("GET", "/machines/cloud-01/console?tail=10")[0] == 409
            status, started = request("POST", "/machines/cloud-01/start")
            assert status == 200 and started["state"] == "running", (status, started)
            log_in()
            shell("echo again-$(cat ~/note)", "again-kept-42")
            return {"note": "kept-42"}
        case("the file survives stop and start", survives_stop_start)

        def survives_reboot():
            status, before = request("GET", "/machines/cloud-01")
            type_line("sudo reboot")

            def restarted():
                status, now = request("GET", "/machines/cloud-01")
                assert status == 200 and now["state"] == "running"
                assert now["startedAt"] > before["startedAt"], "not booted again yet"
            deadline = time.monotonic() + 120
            while True:
                try:
                    restarted()
                    break
                except AssertionError:
                    assert time.monotonic() < deadline, "the daemon did not boot it again"
                    time.sleep(1)
            log_in()
            shell("echo third-$(cat ~/note)", "third-kept-42")
            return {"note": "kept-42"}
        case("the file survives the guest rebooting itself", survives_reboot)

        def refusals():
            results = {}
            for label, body in [
                ("network", {"name": "bad-01", "image": "cirros.raw", "network": {}}),
                ("missing", {"name": "bad-02", "image": "absent.raw"}),
                ("path", {"name": "bad-03", "image": "../cirros.raw"}),
                ("both", {"name": "bad-04", "image": "cirros.raw", "templateID": "base"}),
                ("too small", {"name": "bad-05", "image": "cirros.raw", "diskGiB": 0}),
            ]:
                status, value = request("POST", "/machines", body)
                assert status == 400, (label, status, value)
                assert request("GET", f"/machines/{body['name']}")[0] == 404, label
                results[label] = value["message"]
            return results
        case("what cannot work is refused at creation", refusals)
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
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
