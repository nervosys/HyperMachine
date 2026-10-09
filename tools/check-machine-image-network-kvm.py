#!/usr/bin/env python3
"""A machine booted from a stock cloud image, with a network, on real KVM.

Runs an owned hv2-sandboxd with --firmware, an --image-dir holding the image
and a --machine-dir, and an HTTP server on this host's own address standing in
for a service a machine may reach (the operator grants it with
--tenant-reserved-cidr). Then checks, through the API and the guest's console:

1. A machine created from the image with a network boots, and its guest
   configures itself by DHCP: the address, default route and resolver are the
   ones its gateway gives.
2. The guest reaches the address its allowOut names and is refused one its
   denyOut names, and the gateway's decision log says both.
3. After a stop and a start it configures itself again and reaches the
   service again.

The image is CirrOS, used as downloaded: its own kernel, its own DHCP client.
Writes report.json, the daemon's log and the console into --output.
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
              "purpose": "functional verification of a networked image machine on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "firmware", "image"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-imgnet-check-", dir="/var/tmp"))
    images = work / "images"
    images.mkdir()
    shutil.copyfile(args.image, images / "cirros.raw")
    machines = work / "machines"
    # This host's own address: private, so reserved unless the operator grants it.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(("192.0.2.1", 9))
        private = probe.getsockname()[0]
    www = work / "www"
    www.mkdir()
    (www / "marker").write_text("SERVICE-REACHED\n")
    with socket.socket() as sock:
        sock.bind((private, 0))
        web_port = sock.getsockname()[1]
    web = subprocess.Popen(["python3", "-m", "http.server", str(web_port), "--bind", private],
                           cwd=www, stdin=subprocess.DEVNULL,
                           stdout=(args.output / "service.log").open("wb"), stderr=subprocess.STDOUT)
    service = f"http://{private}:{web_port}/marker"
    deadline = time.monotonic() + 20
    while True:
        try:
            assert urllib.request.urlopen(service, timeout=2).read().strip() == b"SERVICE-REACHED"
            break
        except OSError:
            assert web.poll() is None, "the stand-in service exited"
            assert time.monotonic() < deadline, "the stand-in service never listened"
            time.sleep(0.1)
    api = port()
    base = f"http://127.0.0.1:{api}"
    # A kernel and initrd are the daemon's own requirement; no machine here
    # uses them, and --no-template keeps it from booting one.
    env = dict(os.environ, RUST_LOG="info")
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(machines), "--firmware", str(args.firmware), "--image-dir", str(images),
         "--tenant-reserved-cidr", f"{private}/32",
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
        # A long command makes the guest pad its console with NULs: strip them.
        text = text if isinstance(text, str) else json.dumps(text)
        return re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", text.replace(chr(0), ""))

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
        return wait_for(marker, 30, tail=20000)

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

        def decisions():
            status, value = request("GET", "/machines/cloud-01/network/decisions")
            assert status == 200, (status, value)
            return [f'{d["verdict"]} {d["destination"]} ({d["reason"]})' for d in value["decisions"]]

        def configured():
            # Each line carries a marker the guest's shell computes, so it is
            # the command's output that is read and not its echo.
            address = shell("echo addr-$((1+1)) $(ip -4 -o addr show dev eth0 | awk '{print $4}')", "addr-2 ")
            route = shell("echo route-$((1+1)) $(ip route | awk '/^default/ {print $3}')", "route-2 ")
            resolver = shell("echo dns-$((1+1)) $(awk '/^nameserver/ {print $2}' /etc/resolv.conf | head -1)",
                             "dns-2 ")
            found = {
                "address": re.findall(r"addr-2 (\S+)", address)[-1],
                "default_route": re.findall(r"route-2 (\S+)", route)[-1],
                "resolver": re.findall(r"dns-2 (\S+)", resolver)[-1],
            }
            assert found == {"address": "10.0.2.15/24", "default_route": "10.0.2.2", "resolver": "10.0.2.3"}, found
            return found

        def by_dhcp():
            status, created = request("POST", "/machines", {
                "name": "cloud-01", "image": "cirros.raw",
                "network": {"allowOut": [f"{private}/32"], "denyOut": ["192.0.2.0/24"]}})
            assert status == 201, (status, created)
            assert created["boot"] == "firmware" and created["network"]["allowOut"] == [f"{private}/32"], created
            log_in()
            return configured()
        case("the guest of a networked image machine configures itself by DHCP", by_dhcp)

        def reaches_what_it_may():
            seen = shell(f"echo got-$((1+1)) $(curl -s -m 8 {service})", "got-2 SERVICE-REACHED")
            shell("curl -s -m 5 -o /dev/null http://192.0.2.7/ ; echo refused-$((1+1))", "refused-2")
            log = decisions()
            assert f"allow {private}:{web_port} (allowOut address)" in log, log
            assert any(d.startswith("deny 192.0.2.7:80 (") for d in log), log
            return {"saw": [line for line in seen.replace("\r", "\n").split("\n") if "got-2" in line][-1:],
                    "decisions": [d for d in log if str(web_port) in d or "192.0.2.7" in d]}
        case("it reaches the address its allowOut names and is refused one its denyOut names",
             reaches_what_it_may)

        def again_after_a_restart():
            assert request("POST", "/machines/cloud-01/stop")[0] == 200
            assert request("POST", "/machines/cloud-01/start")[0] == 200
            log_in()
            found = configured()
            shell(f"echo again-$((1+1)) $(curl -s -m 8 {service})", "again-2 SERVICE-REACHED")
            return found
        case("after a stop and a start it configures itself and reaches the service again",
             again_after_a_restart)
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
        web.terminate()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
