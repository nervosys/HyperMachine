#!/usr/bin/env python3
"""Machine networking on real KVM: one NIC behind the node's egress gateway.

Runs an owned hv2-sandboxd with --network and a --machine-dir, and an HTTP
server on this host's own address standing in for a service a machine may
reach (the operator grants it with --tenant-reserved-cidr). Then checks,
through the API:

1. A machine created with a network has a configured NIC, reaches the
   address its allowOut names, and is refused the cloud metadata address;
   the gateway's decision log says both.
2. A machine created without a network has no NIC.
3. A machine whose network does not allow that address is refused it.
4. The network comes back after stop and start and after a guest reboot,
   and the egress CA is in the guest's trust bundle exactly once however
   many times it has booted.
5. A network that cannot be decided is refused at creation, leaving nothing.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path


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
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of machine networking on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-net-check-", dir="/var/tmp"))
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
    machines = work / "machines"
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    state = {"daemon": None, "base": None}

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def start_daemon(label):
        api, proxy = port(), port()
        log = (args.output / f"{label}.log").open("wb")
        daemon = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy), "--no-template",
             "--network", "--tenant-reserved-cidr", f"{private}/32",
             "--machine-dir", str(machines), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8"],
            env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        state["daemon"], state["base"] = daemon, f"http://127.0.0.1:{api}"
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, f"{label} exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, f"{label} readiness"
            time.sleep(0.1)

    def request(method, path, body=None, timeout=180):
        req = urllib.request.Request(state["base"] + path, method=method,
                                     headers={"content-type": "application/json"},
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

    def run(name, cmd):
        status, value = request("POST", f"/machines/{name}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, f"{name}: {cmd!r}: {status} {value}"
        return value["stdout"].strip()

    def attempt(name, cmd):
        status, value = request("POST", f"/machines/{name}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200, f"{name}: {cmd!r}: {status} {value}"
        return value["exit_code"], (value["stdout"] + value["stderr"]).strip()

    def decisions(name):
        status, value = request("GET", f"/machines/{name}/network/decisions")
        assert status == 200, (status, value)
        return [f'{d["verdict"]} {d["destination"]} ({d["reason"]})' for d in value["decisions"]]

    fetch = f"busybox wget -q -T 8 -O - {service}"
    bundle = "grep -c 'BEGIN CERTIFICATE' /etc/ssl/certs/ca-certificates.crt"

    def eventually(action, seconds=120):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.5)

    try:
        start_daemon("daemon")

        def reaches_what_it_may():
            status, created = request("POST", "/machines", {
                "name": "net-01", "diskGiB": 1, "network": {"allowOut": [f"{private}/32"]}})
            assert status == 201 and created["state"] == "running", (status, created)
            assert created["network"]["allowOut"] == [f"{private}/32"], created
            address = run("net-01", "busybox ip -4 -o addr show dev eth0")
            assert " inet " in address, address
            code, output = attempt("net-01", fetch)
            assert output == "SERVICE-REACHED", (output, decisions("net-01"))
            code, output = attempt("net-01", "busybox wget -q -T 5 -O - http://169.254.169.254/")
            assert code != 0, output
            log = decisions("net-01")
            assert f"allow {private}:{web_port} (allowOut address)" in log, log
            assert "deny 169.254.169.254:80 (reserved address)" in log, log
            return {"guest_address": address.split()[3], "decisions": log}
        case("a machine with a network reaches what it may and not the metadata address",
             reaches_what_it_may)

        def no_network_no_nic():
            status, created = request("POST", "/machines", {"name": "bare-01", "diskGiB": 1})
            assert status == 201 and created["network"] is None, (status, created)
            links = run("bare-01", "ls /sys/class/net")
            assert "eth0" not in links.split(), links
            status, value = request("GET", "/machines/bare-01/network/decisions")
            assert status == 400, (status, value)
            assert request("POST", "/machines/bare-01/stop")[0] == 200
            assert request("DELETE", "/machines/bare-01")[0] == 204
            return {"links": links.split()}
        case("a machine without a network has no NIC", no_network_no_nic)

        def refused_what_it_may_not():
            status, created = request("POST", "/machines", {
                "name": "shut-01", "diskGiB": 1, "network": {"allowInternetAccess": False}})
            assert status == 201, (status, created)
            code, output = attempt("shut-01", fetch)
            assert code != 0 and "SERVICE-REACHED" not in output, output
            log = decisions("shut-01")
            assert any(d.startswith(f"deny {private}:{web_port} (") for d in log), log
            assert request("POST", "/machines/shut-01/stop")[0] == 200
            assert request("DELETE", "/machines/shut-01")[0] == 204
            return {"decisions": log}
        case("a machine whose network does not allow an address is refused it",
             refused_what_it_may_not)

        def network_returns():
            first = int(run("net-01", bundle))
            assert first == 1, first
            assert request("POST", "/machines/net-01/stop")[0] == 200
            assert request("POST", "/machines/net-01/start")[0] == 200
            assert run("net-01", fetch) == "SERVICE-REACHED"
            after_restart = int(run("net-01", bundle))
            run("net-01", "touch /tmp/before-reboot")
            request("POST", "/machines/net-01/exec", {"cmd": "busybox reboot -f", "timeout_secs": 5})

            def fresh():
                code, _ = attempt("net-01", "test ! -e /tmp/before-reboot")
                assert code == 0
            eventually(fresh)
            assert eventually(lambda: run("net-01", fetch)) == "SERVICE-REACHED"
            after_reboot = int(run("net-01", bundle))
            assert after_restart == 1 and after_reboot == 1, (after_restart, after_reboot)
            assert run("net-01", "readlink /etc/resolv.conf") == "/proc/net/pnp"
            return {"ca_certificates_in_bundle_after_three_boots": after_reboot}
        case("the network returns after stop/start and a guest reboot, the CA trusted once",
             network_returns)

        def bad_network_is_refused():
            status, value = request("POST", "/machines", {
                "name": "bad-01", "diskGiB": 1, "network": {"allowOut": ["not a host or cidr/99"]}})
            assert status == 400, (status, value)
            assert request("GET", "/machines/bad-01")[0] == 404
            return {"status": status, "reply": value}
        case("a network that cannot be decided is refused at creation", bad_network_is_refused)
        report["success"] = True
    finally:
        if state["daemon"] and state["daemon"].poll() is None:
            state["daemon"].terminate()
            try:
                state["daemon"].wait(timeout=20)
            except subprocess.TimeoutExpired:
                state["daemon"].kill()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        web.terminate()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
