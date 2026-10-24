#!/usr/bin/env python3
"""A machine's lifecycle events reach the webhooks that ask for them, on one real KVM cluster.

Runs an owned Redis, one hv2-sandboxd node (--machine-dir, --allow-private-webhooks,
so a receiver on loopback may be called) and the hv2-control-plane over verified
TLS and node mTLS, with a red key, a blue key and the legacy administrator, and
an HTTP receiver of the script's own. Through the control plane's API only:

1. Red registers a webhook for the four machine types, with a secret. Red creates
   a machine, stops and starts it, stops it, and deletes it. The receiver gets
   `machine.lifecycle.created`, `started`, `stopped`, `started`, `stopped` and
   `deleted`, each naming the machine, and each `hv2-signature` verifies against
   the secret (HMAC-SHA256 over `<timestamp>.<body>`) as does the E2B-style
   `e2b-signature`.
2. Blue's webhook for the same types and red's with no list of types heard none
   of red's machine's events; the delivery records of red's webhook show six
   successes.
3. Blue's machine of the same name then reaches blue's webhook, and not red's.
4. Red's webhook with no list of types still gets a sandbox's events (a sandbox
   made and deleted), and the sandbox event list (`GET /events/sandboxes`) shows
   no machine event.
5. A webhook naming a type that is neither `sandbox.lifecycle.*` nor
   `machine.lifecycle.*` (`machine.other`, `volume.lifecycle.created`) is
   refused with 400.

Writes report.json and each service's log into --output. Needs root, Redis and
openssl on the host.
"""
import argparse, base64, hashlib, hmac, http.client, json, socket, ssl, subprocess, tempfile, threading, time, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

TYPES = ["machine.lifecycle.created", "machine.lifecycle.started",
         "machine.lifecycle.stopped", "machine.lifecycle.deleted"]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {name: getattr(args, name) for name in ["daemon", "control", "kernel", "initrd"]}
    inputs["driver"] = Path(__file__)
    report = {"success": False, "purpose": "local KVM functional verification of machine webhooks; no timing claim",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "admin", "token"]}
    secrets = {"red": "red-secret-" + uuid.uuid4().hex, "blue": "blue-secret-" + uuid.uuid4().hex,
               "redall": "redall-secret-" + uuid.uuid4().hex}
    env = {"PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "RUST_LOG": "warn"}
    processes, handles = [], []
    state = {}
    received = []  # (hook path, headers, parsed body, raw body)
    lock = threading.Lock()

    class Receiver(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers.get("content-length", "0")))
            with lock:
                received.append((self.path.strip("/"), {k.lower(): v for k, v in self.headers.items()},
                                 json.loads(raw), raw))
            self.send_response(200)
            self.send_header("Content-Length", "0")
            self.end_headers()

    receiver = ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
    receiver.daemon_threads = True
    threading.Thread(target=receiver.serve_forever, daemon=True).start()
    receiver_url = f"http://127.0.0.1:{receiver.server_port}"

    def heard(hook):
        with lock:
            rows = [row for row in received if row[0] == hook]
        return sorted(rows, key=lambda row: (row[2]["timestamp"], row[2]["id"]))

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def api(method, path, body=None, key="red", status=200):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
            assert response.status in ((status,) if isinstance(status, int) else status), \
                f"{key} {method} {path}: {response.status}, expected {status}: {data[:300]!r}"
            return json.loads(data) if data else None
        finally:
            conn.close()

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    def verifies(row, secret):
        _, headers, _, raw = row
        stamp = headers["hv2-signature-timestamp"]
        want = hmac.new(secret.encode(), f"{stamp}.{raw.decode()}".encode(), hashlib.sha256).hexdigest()
        modern = any(v == "v1=" + want for v in headers["hv2-signature"].split(" "))
        old = base64.b64encode(hashlib.sha256(secret.encode() + raw).digest()).decode().rstrip("=")
        return modern and headers["e2b-signature"] == old

    def register(key, name, events, secret):
        body = {"name": name, "url": f"{receiver_url}/{name}", "signatureSecret": secret}
        if events is not None:
            body["events"] = events
        return api("POST", "/events/webhooks", body, key=key, status=(200, 201))

    with tempfile.TemporaryDirectory(prefix="hm-machine-webhooks-", dir="/var/tmp") as temporary:
        directory = Path(temporary)

        def start(name, argv, environment):
            handle = (args.output / (name + ".log")).open("wb")
            handles.append(handle)
            child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle, stderr=subprocess.STDOUT)
            processes.append(child)
            return child

        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)

        try:
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-teams-ca",
                     "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                     "-keyout", str(directory / "ca.key"), "-out", str(directory / "ca.pem")])
            for name, usage, san in [("node", "serverAuth", "DNS:tcp-node.test,IP:127.0.0.1"),
                                     ("control", "clientAuth", "DNS:tcp-control.test"),
                                     ("api", "serverAuth", "IP:127.0.0.1,DNS:localhost")]:
                openssl(["req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                         "-keyout", str(directory / (name + ".key")), "-out", str(directory / (name + ".csr"))])
                ext = directory / (name + ".ext")
                ext.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                               f"extendedKeyUsage={usage}\nsubjectAltName={san}\n")
                openssl(["x509", "-req", "-in", str(directory / (name + ".csr")), "-CA", str(directory / "ca.pem"),
                         "-CAkey", str(directory / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(ext),
                         "-out", str(directory / (name + ".pem"))])
            state["context"] = ssl.create_default_context(cafile=str(directory / "ca.pem"))
            ports = set()
            while len(ports) < 5:
                ports.add(port())
            redis_port, node_port, node_proxy, api_port, proxy_port = list(ports)
            state["api_port"] = api_port
            store = f"redis://127.0.0.1:{redis_port}"
            namespace = "mwh-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5)
            expiry = int(time.time()) + 1800
            scopes = ["sandboxes", "inventory", "templates", "machines", "events"]
            policy = [
                {"sha256": hashlib.sha256(keys["red"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": scopes, "principal_id": "red-a", "team_id": "red"},
                {"sha256": hashlib.sha256(keys["blue"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": scopes, "principal_id": "blue-a", "team_id": "blue"},
            ]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
                           "--machine-dir", str(directory / "machines"), "--allow-private-webhooks",
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "mwh-node",
                           "--advertise-api", f"https://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                           "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                           "--mtls-key", str(directory / "node.key")],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": keys["token"]})
            start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                              "--proxy-port", str(proxy_port), "--api-keys-file", str(policies),
                              "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                              "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                              "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                              "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "tcp-node.test"],
                  {**env, "HV2_API_KEY": keys["admin"], "HV2_CLUSTER_TOKEN": keys["token"]})

            def prepared():
                assert all(child.poll() is None for child in processes), "an owned service exited"
                templates = api("GET", "/templates", key="admin")
                assert any(row.get("templateID") == "base" and row.get("snapshot") for row in templates), "template not ready"
            eventually(prepared, 300)

            hooks = {}

            def red_machine():
                hooks["red"] = register("red", "red", TYPES, secrets["red"])["id"]
                hooks["blue"] = register("blue", "blue", TYPES, secrets["blue"])["id"]
                hooks["redall"] = register("red", "redall", None, secrets["redall"])["id"]
                created = api("POST", "/machines", {"name": "hook-01", "diskGiB": 1}, status=201)
                assert created["state"] == "running", created
                api("POST", "/machines/hook-01/stop")
                api("POST", "/machines/hook-01/start")
                api("POST", "/machines/hook-01/stop")
                api("DELETE", "/machines/hook-01", status=204)
                want = [t.split(".")[-1] for t in ["machine.lifecycle.created", "machine.lifecycle.started",
                        "machine.lifecycle.stopped", "machine.lifecycle.started", "machine.lifecycle.stopped",
                        "machine.lifecycle.deleted"]]

                def all_six():
                    rows = heard("red")
                    assert len(rows) >= 6, [r[2]["type"] for r in rows]
                    return rows
                rows = eventually(all_six, 60)
                time.sleep(1)
                rows = heard("red")
                kinds = [r[2]["type"].split(".")[-1] for r in rows]
                assert kinds == want, kinds
                for row in rows:
                    body = row[2]
                    assert body["machine_name"] == "hook-01" and body["event_category"] == "lifecycle", body
                    assert "sandbox_id" not in body, body
                    assert verifies(row, secrets["red"]), "signature does not verify"
                return {"types": kinds}
            case("red's webhook hears its machine's lifecycle, in order, signed", red_machine)

            def others_heard_none():
                time.sleep(1)
                assert heard("blue") == [] and heard("redall") == [], (len(heard("blue")), len(heard("redall")))
                groups = api("GET", f"/events/webhooks/{hooks['red']}/deliveries")
                assert len(groups) == 6, groups
                for group in groups:
                    assert group["eventType"].startswith("machine.lifecycle."), group
                    assert group["sandboxId"] == "hook-01", group
                    assert [a["status"] for a in group["attempts"]] == ["success"], group
                return {"events_delivered": len(groups)}
            case("blue's webhook and red's catch-all heard none of it, and red's delivery records show success",
                 others_heard_none)

            def blues_own():
                api("POST", "/machines", {"name": "hook-01", "diskGiB": 1}, key="blue", status=201)
                rows = eventually(lambda: heard("blue") if len(heard("blue")) >= 2 else (_ for _ in ()).throw(AssertionError("blue")), 60)
                kinds = [r[2]["type"].split(".")[-1] for r in rows]
                assert kinds[:2] == ["created", "started"], kinds
                assert all(verifies(r, secrets["blue"]) for r in rows)
                time.sleep(1)
                assert len(heard("red")) == 6 and heard("redall") == [], (len(heard("red")), len(heard("redall")))
                return {"blue_types": kinds}
            case("blue's machine of the same name reaches blue's webhook and not red's", blues_own)

            def sandboxes_unchanged():
                made = api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600}, status=201)["sandboxID"]
                api("DELETE", f"/sandboxes/{made}", status=(200, 204))
                rows = eventually(lambda: heard("redall") if len(heard("redall")) >= 2 else (_ for _ in ()).throw(AssertionError("redall")), 60)
                kinds = sorted(r[2]["type"] for r in rows)
                assert kinds == ["sandbox.lifecycle.created", "sandbox.lifecycle.killed"], kinds
                assert all(verifies(r, secrets["redall"]) for r in rows)
                listed = api("GET", "/events/sandboxes")
                assert listed and all(e["type"].startswith("sandbox.") for e in listed), listed
                return {"redall": kinds, "listed": len(listed)}
            case("a webhook with no list of types still gets sandbox events only, and the sandbox event list has none of a machine's",
                 sandboxes_unchanged)

            def refused():
                for bad in ["machine.other", "volume.lifecycle.created"]:
                    api("POST", "/events/webhooks", {"name": "bad", "url": f"{receiver_url}/bad", "events": [bad],
                                                     "signatureSecret": "bad-secret-0123456789"}, status=400)
                return {"refused": 2}
            case("a type that is neither sandbox.lifecycle.* nor machine.lifecycle.* is refused", refused)
            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            receiver.shutdown()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
