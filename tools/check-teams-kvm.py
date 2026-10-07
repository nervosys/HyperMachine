#!/usr/bin/env python3
"""Two teams on one real KVM cluster: what each can reach, and what it cannot.

Runs an owned Redis, one hv2-sandboxd node and the hv2-control-plane over
verified TLS and node mTLS, with an API key policy of two red keys (different
principals), one blue key and the legacy administrator. Then, through the
control plane's API only:

1. Sandboxes: red's second member lists, reads and runs commands in red's
   first member's sandbox; blue sees none of it (403 on every route tried,
   an empty list), and the administrator sees both teams'.
2. Volumes: red and blue each create a volume named `data` and get two
   volumes; a file a red guest writes through its mount is not in blue's
   `data`; blue cannot read red's volume by ID (404).
3. Snapshots: red snapshots its sandbox; red starts a new sandbox from it
   and finds the file it wrote; blue cannot list, start from or delete it,
   and red deletes it.
4. Forks of a red sandbox stay red.
5. Events and webhooks: blue's event list holds none of red's sandboxes,
   and blue cannot see red's webhook.

Writes report.json and each service's log into --output.
"""
import argparse, hashlib, http.client, json, socket, ssl, subprocess, tempfile, time, uuid
from pathlib import Path


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
    report = {"success": False, "purpose": "local KVM functional verification of team isolation; no performance comparison",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red_a", "red_b", "blue", "admin", "token"]}
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn"}
    processes, handles = [], []
    state = {}

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def api(method, path, body=None, key="red_a", status=200):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
            assert response.status == status, f"{key} {method} {path}: {response.status}, expected {status}: {data[:300]!r}"
            return json.loads(data) if data else None
        finally:
            conn.close()

    def run(guest, cmd, key="red_a"):
        value = api("POST", f"/sandboxes/{guest}/exec", {"cmd": cmd, "timeout_secs": 30}, key=key)
        assert value.get("exit_code") == 0, f"guest command failed: {value}"
        return value["stdout"].strip()

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    def create(key, **body):
        created = api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600, **body}, key=key, status=201)
        return created["sandboxID"]

    def ids(rows, field="sandboxID"):
        return sorted(row[field] for row in rows)

    with tempfile.TemporaryDirectory(prefix="hm-teams-kvm-", dir="/var/tmp") as temporary:
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
            namespace = "teams-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5)
            expiry = int(time.time()) + 1800
            policy = [
                {"sha256": hashlib.sha256(keys["red_a"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "volumes", "templates", "events"],
                 "principal_id": "red-a", "team_id": "red"},
                {"sha256": hashlib.sha256(keys["red_b"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "volumes", "templates", "events"],
                 "principal_id": "red-b", "team_id": "red"},
                {"sha256": hashlib.sha256(keys["blue"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "volumes", "templates", "events"],
                 "principal_id": "blue-a", "team_id": "blue"},
            ]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
                           "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots"),
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "teams-kvm-node",
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

            red = create("red_a")
            blue = create("blue")

            def sandboxes():
                assert ids(api("GET", "/sandboxes", key="red_b")) == [red]
                assert run(red, "echo from-red-b", key="red_b") == "from-red-b"
                assert api("GET", f"/sandboxes/{red}", key="red_b")["sandboxID"] == red
                for method, suffix in [("GET", ""), ("DELETE", ""), ("POST", "/exec"), ("POST", "/pause"),
                                       ("POST", "/fork"), ("GET", "/checkpoints"), ("GET", "/logs")]:
                    api(method, f"/sandboxes/{red}{suffix}", {"cmd": "true"} if suffix == "/exec" else None,
                        key="blue", status=403)
                assert ids(api("GET", "/sandboxes", key="blue")) == [blue]
                assert ids(api("GET", "/sandboxes", key="admin")) == sorted([red, blue])
                return {"red": red, "blue": blue}
            case("a team reaches its whole team's sandboxes and nothing else", sandboxes)

            def volumes():
                red_volume = api("POST", "/volumes", {"name": "data"}, status=201)
                blue_volume = api("POST", "/volumes", {"name": "data"}, key="blue", status=201)
                assert red_volume["volumeID"] != blue_volume["volumeID"], "one name, two teams, one volume"
                assert ids(api("GET", "/volumes"), "volumeID") == [red_volume["volumeID"]]
                api("GET", f"/volumes/{red_volume['volumeID']}", key="blue", status=404)
                red_mounted = create("red_a", volumeMounts=[{"name": "data", "path": "/mnt/data"}])
                blue_mounted = create("blue", volumeMounts=[{"name": "data", "path": "/mnt/data"}])
                run(red_mounted, "echo red-secret > /mnt/data/note && sync")
                listing = run(blue_mounted, "ls -A /mnt/data", key="blue")
                assert listing == "", f"blue's data holds {listing!r}"
                assert run(red_mounted, "cat /mnt/data/note") == "red-secret"
                return {"red": red_volume["volumeID"], "blue": blue_volume["volumeID"]}
            case("volumes of one name are each team's own", volumes)

            def snapshots():
                run(red, "echo kept-in-snapshot > /root/marker")
                api("POST", f"/sandboxes/{red}/snapshots", {"name": "red-snap"}, status=201)
                def offered():
                    assert any(r["templateID"] == "red-snap" for r in api("GET", "/templates"))
                eventually(offered, 30)
                assert not any(r["templateID"] == "red-snap" for r in api("GET", "/templates", key="blue"))
                assert not any(r["snapshotID"].startswith("red-snap") for r in api("GET", "/snapshots", key="blue"))
                api("POST", "/v2/sandboxes", {"templateID": "red-snap", "timeout": 600}, key="blue", status=404)
                api("DELETE", "/templates/red-snap", key="blue", status=404)
                restored = create("red_b", templateID="red-snap")
                assert run(restored, "cat /root/marker", key="red_b") == "kept-in-snapshot"
                api("DELETE", "/templates/red-snap", status=204)
                return {"restored": restored}
            case("a team's snapshot is its own to start from and delete", snapshots)

            def forks():
                forked = api("POST", f"/sandboxes/{red}/fork", {"count": 1, "timeout": 600}, key="red_b", status=201)
                child = forked[0]["sandbox"]["sandboxID"]
                assert child in ids(api("GET", "/sandboxes", key="red_a"))
                api("GET", f"/sandboxes/{child}", key="blue", status=403)
                return {"fork": child}
            case("a fork stays in its team", forks)

            def events():
                hook = api("POST", "/events/webhooks", {"name": "red-hook", "url": "https://example.com/hook",
                                                        "signatureSecret": "0123456789abcdef"}, status=201)
                assert [h["id"] for h in api("GET", "/events/webhooks", key="blue")] == []
                api("GET", f"/events/webhooks/{hook['id']}", key="blue", status=404)
                blue_events = api("GET", "/events/sandboxes?limit=1000", key="blue")
                assert blue_events, "blue has events of its own"
                assert all(e.get("sandboxId") != red for e in blue_events), "blue sees red's events"
                red_events = api("GET", "/events/sandboxes?limit=1000")
                assert any(e.get("sandboxId") == red for e in red_events)
                return {"blue_events": len(blue_events), "red_events": len(red_events)}
            case("events and webhooks are each team's own", events)
            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
