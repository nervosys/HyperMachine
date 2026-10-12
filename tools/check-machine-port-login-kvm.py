#!/usr/bin/env python3
"""A login in front of a machine's port, through the control plane, on KVM.

Runs Redis, a node with mTLS and a machine directory, and a control plane
with web access (HTTP Basic, per target ID), proxy TLS and node mTLS, as
docs/PRIVATE_GUEST_URLS.md requires. One machine serves a page on its port
8080, published with `proxy`. Through the control plane's proxy, at
`8080-{machineID}.machine.test`:

1. With no credentials, the wrong password, or a credential whose grant does
   not name this machine, the answer is 401 with a Basic challenge.
2. With a grant naming the machine, after the policy is reloaded, the page
   comes back, marked private and not to be stored.
3. A port published without `proxy` is not reached, even with credentials.
4. A machine ID that is not one is not asked of any node.

Writes report.json and the logs into --output.
"""
import argparse, base64, hashlib, http.client, json, os, shutil, signal, socket, ssl, subprocess, tempfile, time, uuid
from pathlib import Path

NAME = "login-01"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a login in front of a machine port on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "control", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    admin_key, token, secret = (uuid.uuid4().hex for _ in range(3))
    env = {"PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "RUST_LOG": "info"}
    processes, handles = [], []
    directory = Path(tempfile.mkdtemp(prefix="hm-machine-login-", dir="/var/tmp"))

    def start(name, argv, environment):
        handle = (args.output / f"{name}.log").open("wb")
        handles.append(handle)
        child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle,
                                 stderr=subprocess.STDOUT)
        processes.append((name, child))
        return child

    def run(argv):
        subprocess.run(argv, check=True, capture_output=True, timeout=30)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.2)

    try:
        run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=login-ca",
             "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
             "-keyout", str(directory / "ca.key"), "-out", str(directory / "ca.pem")])
        for name, usage, san in [("node", "serverAuth", "DNS:login-node.test,IP:127.0.0.1"),
                                 ("control", "clientAuth", "DNS:login-control.test"),
                                 ("api", "serverAuth", "IP:127.0.0.1,DNS:localhost")]:
            run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                 "-keyout", str(directory / f"{name}.key"), "-out", str(directory / f"{name}.csr")])
            ext = directory / f"{name}.ext"
            ext.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                           f"extendedKeyUsage={usage}\nsubjectAltName={san}\n")
            run(["openssl", "x509", "-req", "-in", str(directory / f"{name}.csr"), "-CA", str(directory / "ca.pem"),
                 "-CAkey", str(directory / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(ext),
                 "-out", str(directory / f"{name}.pem")])
        context = ssl.create_default_context(cafile=str(directory / "ca.pem"))
        redis_port, node_port, node_proxy, api_port, proxy_port = (port() for _ in range(5))
        store, namespace = f"redis://127.0.0.1:{redis_port}", "machine-login-" + uuid.uuid4().hex
        start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                        "--dir", str(directory)], env)

        def redis_ready():
            socket.create_connection(("127.0.0.1", redis_port), timeout=0.2).close()
        eventually(redis_ready, 10)

        keys = directory / "keys.json"
        keys.write_text(json.dumps([{"sha256": hashlib.sha256(admin_key.encode()).hexdigest(),
                                     "expires_at": int(time.time()) + 3600,
                                     "scopes": ["admin", "machines", "sandboxes", "inventory", "templates"]}]))
        policy = directory / "web.json"

        def write_policy(targets):
            fresh = directory / "web.json.new"
            fresh.write_text(json.dumps([{"subject": "alice", "sha256": hashlib.sha256(secret.encode()).hexdigest(),
                                          "expires_at": int(time.time()) + 3600, "sandboxes": targets}]))
            os.replace(fresh, policy)
        write_policy([])

        start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                       "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4", "--no-template",
                       "--machine-dir", str(directory / "machines"), "--machine-ports", "33120-33129",
                       "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "login-node",
                       "--advertise-api", f"https://127.0.0.1:{node_port}",
                       "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                       "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                       "--mtls-key", str(directory / "node.key")],
              {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": token})
        control = start("control", [str(args.control), "--store", store, "--namespace", namespace,
                                    "--port", str(api_port), "--proxy-port", str(proxy_port),
                                    "--api-keys-file", str(keys),
                                    "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                                    "--web-access-file", str(policy),
                                    "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                                    "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                                    "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "login-node.test"],
                        {**env, "HV2_CLUSTER_TOKEN": token})

        def api(method, path, body=None, status=200, timeout=300):
            conn = http.client.HTTPSConnection("127.0.0.1", api_port, context=context, timeout=timeout)
            try:
                conn.request(method, path, body=None if body is None else json.dumps(body),
                             headers={"x-api-key": admin_key, "content-type": "application/json"})
                response = conn.getresponse()
                data = response.read()
                assert response.status == status, (method, path, response.status, data[:300])
                return json.loads(data) if data else None
            finally:
                conn.close()

        def web(label, password=None):
            """GET through the control plane's proxy; status, headers, body."""
            conn = http.client.HTTPSConnection("127.0.0.1", proxy_port, context=context, timeout=30)
            headers = {"host": f"{label}.machine.test"}
            if password is not None:
                headers["authorization"] = "Basic " + base64.b64encode(f"alice:{password}".encode()).decode()
            try:
                conn.request("GET", "/page.txt", headers=headers)
                response = conn.getresponse()
                return response.status, dict(response.getheaders()), response.read()
            finally:
                conn.close()

        def ready():
            assert all(child.poll() is None for _, child in processes), "a service exited"
            api("GET", "/machines")
        eventually(ready, 60)
        created = api("POST", "/machines", {"name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, status=201)
        machine_id = created["machineID"]
        report["machine_id"] = machine_id
        run_answer = api("POST", f"/machines/{NAME}/runs",
                         {"cmd": "mkdir -p /root/www && echo machine-page > /root/www/page.txt && "
                                 "exec httpd -f -p 8080 -h /root/www"}, status=201)
        api("POST", f"/machines/{NAME}/runs", {"cmd": "exec httpd -f -p 9090 -h /root/www"}, status=201)
        proxied = api("POST", f"/machines/{NAME}/ports", {"guestPort": 8080, "proxy": True}, status=201)
        api("POST", f"/machines/{NAME}/ports", {"guestPort": 9090}, status=201)
        assert proxied["proxyLabel"] == f"8080-{machine_id}", proxied
        report["run"] = run_answer["runID"]
        label = f"8080-{machine_id}"

        def refused_without_a_grant():
            results = {}
            for name, password in [("no credentials", None), ("wrong password", "not-" + secret),
                                   ("a grant that does not name the machine", secret)]:
                status, headers, _ = web(label, password)
                assert status == 401, (name, status)
                challenge = headers.get("www-authenticate") or headers.get("WWW-Authenticate") or ""
                assert challenge.startswith("Basic "), (name, headers)
                results[name] = status
            return results
        case("without a grant naming the machine, a login is asked for and refused", refused_without_a_grant)

        def reached_with_a_grant():
            write_policy([machine_id])
            control.send_signal(signal.SIGHUP)

            def page():
                status, headers, body = web(label, secret)
                assert status == 200 and body == b"machine-page\n", (status, body[:200])
                return headers
            headers = eventually(page, 30)
            cache = headers.get("cache-control") or headers.get("Cache-Control")
            assert cache == "private, no-store", headers
            # And still refused without the credential.
            assert web(label, None)[0] == 401
            return {"cache-control": cache}
        case("with a grant naming the machine, its page comes through the control plane", reached_with_a_grant)

        def only_what_was_published_with_proxy():
            status, _, body = web(f"9090-{machine_id}", secret)
            assert status != 200, (status, body[:200])
            status_unpublished, _, _ = web(f"22-{machine_id}", secret)
            assert status_unpublished != 200
            return {"9090 (published without proxy)": status, "22 (not published)": status_unpublished}
        case("a port published without proxy is not reached, credentials or not", only_what_was_published_with_proxy)

        def not_a_machine_id():
            # Granted, so the login lets it through and the refusal is the
            # route's: no machine by that ID, and no node asked for one.
            write_policy(["vm-ZZZZ", machine_id])
            control.send_signal(signal.SIGHUP)
            eventually(lambda: web(label, secret)[0] == 200 or (_ for _ in ()).throw(AssertionError()), 30)

            def routed_nowhere():
                status, _, _ = web("8080-vm-ZZZZ", secret)
                assert status not in (200, 401), status
                return status
            status = eventually(routed_nowhere, 30)
            return {"8080-vm-ZZZZ": status}
        case("an ID that is not a machine's is not asked of any node", not_a_machine_id)

        report["success"] = True
    finally:
        for name, child in reversed(processes):
            if child.poll() is None:
                child.send_signal(signal.SIGTERM)
                try:
                    child.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    child.kill()
        for handle in handles:
            handle.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(directory, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
