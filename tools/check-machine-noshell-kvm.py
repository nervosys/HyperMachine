#!/usr/bin/env python3
"""A machine whose image has no shell, on real KVM: boot, CA, and the shell-only routes.

Runs an owned hv2-sandboxd with --network and a --machine-dir, its node
initrd being an image that holds only the guest agent as init (no /bin/sh,
no busybox). Then checks, through the API:

a. A machine created with a network boots from that image: the machine-boot
   network setup (configure_network) did not fail on the missing shell.
b. The guest's CA bundle holds the node's egress CA exactly once, read back
   through the files API. The image has no bundle, so the agent's file
   writes made it.
c. POST /machines/{name}/exec, which runs its command through /bin/sh,
   answers 400 naming the missing /bin/sh and the agent's error.
d. POST /machines/{name}/runs answers 400 likewise.
e. A direct (argv) exec form: the machine API has none, so this case is
   recorded as not supported rather than run.
f. After stop/start and after restart the machine boots again and the bundle
   still holds the CA exactly once.

Writes report.json and the daemon's log into --output, with the number of
`Kvm` and `Tcg` platform lines the log holds.
"""
import argparse, base64, hashlib, json, os, shutil, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

BUNDLE = "/etc/ssl/certs/ca-certificates.crt"
NAME = "bare-01"


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
    report = {"success": False,
              "purpose": "functional verification of machines on an image with no shell, on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-noshell-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    state = {"daemon": None, "base": None}
    log_path = args.output / "daemon-log.txt"

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, raw_body=None, timeout=180):
        data = raw_body if raw_body is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(state["base"] + path, method=method,
                                     headers={"content-type": "application/json"}, data=data)
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw

    def bundle_certs():
        status, value = request("GET", f"/machines/{NAME}/files?path={BUNDLE}")
        assert status == 200, (status, value)
        text = value if isinstance(value, bytes) else value.encode() if isinstance(value, str) else json.dumps(value).encode()
        # The certificate held must be the node's egress CA, not something else.
        for pem in text.split(b"-----BEGIN CERTIFICATE-----")[1:]:
            body = b"".join(pem.split(b"-----END CERTIFICATE-----")[0].split())
            assert b"HyperMachine sandbox egress CA" in base64.b64decode(body), "a certificate in the bundle is not the node's egress CA"
        return text.count(b"BEGIN CERTIFICATE"), len(text)

    def eventually(action, seconds=120):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.5)

    def refused_for_no_shell(path, body):
        status, value = request("POST", path, body)
        assert status == 400, (status, value)
        message = value["message"]
        assert "no /bin/sh" in message and "No such file or directory" in message, message
        return {"status": status, "message": message}

    api = port()
    state["base"] = f"http://127.0.0.1:{api}"
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--network", "--machine-dir", str(work / "machines"),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log_path.open("wb"), stderr=subprocess.STDOUT)
    state["daemon"] = daemon
    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)

        def boots():
            status, created = request("POST", "/machines", {
                "name": NAME, "diskGiB": 1, "network": {"allowInternetAccess": False}})
            assert status == 201 and created["state"] == "running", (status, created)
            assert created["network"] is not None, created
            return {"state": created["state"]}
        case("a no-shell machine with a network boots (configure_network did not fail)", boots)

        def ca_once():
            count, size = bundle_certs()
            assert count == 1, (count, size)
            return {"certificates_in_bundle": count, "bundle_bytes": size}
        case("the guest's CA bundle holds the egress CA exactly once", ca_once)

        case("exec answers 400 naming the missing /bin/sh",
             lambda: refused_for_no_shell(f"/machines/{NAME}/exec", {"cmd": "true", "timeout_secs": 10}))
        case("runs answers 400 naming the missing /bin/sh",
             lambda: refused_for_no_shell(f"/machines/{NAME}/runs", {"cmd": "true"}))

        def argv_form():
            report["cases"].append({
                "name": "a direct argv exec runs /bin/hv2-urltrace",
                "success": None, "supported": False,
                "detail": "not supported: POST /machines/{name}/exec takes only cmd (run through "
                          "/bin/sh -c) and /runs likewise; the argv form exists only on "
                          "/sandboxes/{id}/exec, covered by check-urltrace-kvm.py"})
            print("not supported: argv exec on machines", flush=True)
        argv_form()

        def boots_again():
            assert request("POST", f"/machines/{NAME}/stop")[0] == 200
            status, started = request("POST", f"/machines/{NAME}/start")
            assert status == 200, (status, started)
            first = eventually(bundle_certs)
            status, restarted = request("POST", f"/machines/{NAME}/restart")
            assert status == 200, (status, restarted)
            second = eventually(bundle_certs)
            assert first[0] == 1 and second[0] == 1, (first, second)
            # the shell-only routes still say the same after the reboots
            refused_for_no_shell(f"/machines/{NAME}/exec", {"cmd": "true", "timeout_secs": 10})
            return {"certificates_after_start": first[0], "certificates_after_restart": second[0]}
        case("after stop/start and restart it boots and the CA is still there once", boots_again)
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
        text = log_path.read_text(errors="replace")
        report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                    "Tcg": text.count("Detected hypervisor platform: Tcg")}
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
