#!/usr/bin/env python3
"""A machine's port by name through the node's proxy, over TLS, on real KVM.

Runs an owned hv2-sandboxd with a self-signed certificate for `*.hm.test`,
so its sandbox proxy speaks TLS, and one machine from the base template
serving a directory over HTTP on its port 8080. Then checks:

1. A port published with `proxy` is reached over HTTPS at
   `8080-{machineID}.hm.test`, by SNI and Host, with the certificate
   verified; and by E2B's `e2b-sandbox-id` / `e2b-sandbox-port` headers.
2. A port published without `proxy`, and a port not published, are not
   reached through the proxy.
3. A machine that is stopped answers nothing through it; started again, it
   does.
4. A port taken back is no longer reached.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, http.client, json, os, shutil, signal, socket, ssl, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "proxied-01"
DOMAIN = "hm.test"


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
              "purpose": "functional verification of a machine port through the node's TLS proxy on KVM; "
                         "no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-proxy-check-", dir="/var/tmp"))
    cert, key = work / "cert.pem", work / "key.pem"
    subprocess.run(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
                    "-nodes", "-keyout", str(key), "-out", str(cert), "-days", "2", "-subj", f"/CN=*.{DOMAIN}",
                    "-addext", f"subjectAltName=DNS:*.{DOMAIN},DNS:localhost,IP:127.0.0.1"],
                   check=True, capture_output=True)
    trust = ssl.create_default_context(cafile=str(cert))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api, proxy = port(), port()
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy), "--no-template",
         "--tls-cert", str(cert), "--tls-key", str(key),
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    scheme = {"api": None}

    def request(method, path, body=None, timeout=180):
        for candidate in ([scheme["api"]] if scheme["api"] else ["https", "http"]):
            req = urllib.request.Request(f"{candidate}://127.0.0.1:{api}{path}", method=method,
                                         headers={"content-type": "application/json"},
                                         data=None if body is None else json.dumps(body).encode())
            try:
                response = urllib.request.urlopen(req, timeout=timeout, context=trust if candidate == "https" else None)
            except urllib.error.HTTPError as error:
                response = error
            except (urllib.error.URLError, ConnectionError, ssl.SSLError):
                if scheme["api"]:
                    raise
                continue
            scheme["api"] = candidate
            with response:
                raw = response.read()
                try:
                    return response.status, json.loads(raw) if raw else None
                except ValueError:
                    return response.status, raw.decode(errors="replace")
        raise OSError("the API answers neither https nor http")

    def through_proxy(label, path="/small.txt", headers=None):
        """GET through the proxy over TLS: the status and body, or the error."""
        conn = http.client.HTTPSConnection("127.0.0.1", proxy, timeout=60, context=trust)
        # SNI and Host carry the name; the certificate is checked against it.
        conn._server_hostname = f"{label}.{DOMAIN}" if label else None  # noqa: SLF001
        host = f"{label}.{DOMAIN}" if label else f"localhost.{DOMAIN}"
        sock = socket.create_connection(("127.0.0.1", proxy), timeout=60)
        conn.sock = trust.wrap_socket(sock, server_hostname=host)
        try:
            conn.request("GET", path, headers=dict({"Host": host}, **(headers or {})))
            response = conn.getresponse()
            return response.status, response.read()
        except (ConnectionError, http.client.HTTPException) as error:
            return None, repr(error).encode()
        finally:
            conn.close()

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, (cmd, status, value)
        return value["stdout"]

    def serve():
        status, value = request("POST", f"/machines/{NAME}/runs", {"cmd": "exec httpd -f -p 8080 -h /root/www"})
        assert status == 201, (status, value)
        deadline = time.monotonic() + 20
        while "8080" not in guest("netstat -ltn"):
            assert time.monotonic() < deadline
            time.sleep(0.2)

    def power(action, want):
        status, machine = request("POST", f"/machines/{NAME}/{action}")
        assert status == 200 and machine["state"] == want, (action, status, machine)

    def reached(label, **more):
        status, body = through_proxy(label, **more)
        return status == 200 and body == b"small\n"

    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        report["api_scheme"] = scheme["api"]
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1})
        assert status == 201 and created["state"] == "running", (status, created)
        machine_id = created["machineID"]
        guest("mkdir -p /root/www && echo small > /root/www/small.txt && sync")
        serve()
        state = {}

        def by_name_over_tls():
            status, published = request("POST", f"/machines/{NAME}/ports", {"guestPort": 8080, "proxy": True})
            assert status == 201, (status, published)
            assert published["proxy"] is True and published["proxyLabel"] == f"8080-{machine_id}", published
            state["host_port"] = published["hostPort"]
            assert reached(f"8080-{machine_id}"), through_proxy(f"8080-{machine_id}")
            # By E2B's headers, with a name that routes nowhere itself.
            assert reached(None, headers={"e2b-sandbox-id": machine_id, "e2b-sandbox-port": "8080"})
            return {"label": published["proxyLabel"]}
        case("a port published with proxy is reached over HTTPS by name, and by E2B's headers", by_name_over_tls)

        def only_what_was_asked():
            status, published = request("POST", f"/machines/{NAME}/ports", {"guestPort": 9000})
            assert status == 201 and published["proxy"] is False and published["proxyLabel"] is None, published
            results = {}
            for label in [f"9000-{machine_id}", f"22-{machine_id}", f"8080-vm-0000000000000000", f"8080-{NAME}"]:
                status, body = through_proxy(label)
                assert status != 200, (label, status, body)
                results[label] = status
            return results
        case("a port published without proxy, or not at all, is not reached through it", only_what_was_asked)

        def stopped_and_started():
            power("stop", "stopped")
            status, body = through_proxy(f"8080-{machine_id}")
            assert status != 200, (status, body)
            power("start", "running")
            serve()
            assert reached(f"8080-{machine_id}")
            return {"while_stopped": status}
        case("a stopped machine answers nothing through it; started again, it does", stopped_and_started)

        def taken_back():
            status, value = request("DELETE", f"/machines/{NAME}/ports/{state['host_port']}")
            assert status == 204, (status, value)
            status, body = through_proxy(f"8080-{machine_id}")
            assert status != 200, (status, body)
            return {"after": status}
        case("a port taken back is no longer reached", taken_back)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=30)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
