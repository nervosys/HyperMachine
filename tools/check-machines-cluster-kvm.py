#!/usr/bin/env python3
"""Machines through the control plane, on one real KVM cluster with two teams.

Runs an owned Redis, one hv2-sandboxd node with a --machine-dir and the
hv2-control-plane over verified TLS and node mTLS, with an API key policy of
two red keys, one blue key, a key without the machines scope and the legacy
administrator. Then, through the control plane's API only:

1. Red creates a machine; the control plane places it on the node and says
   which. Red's second member lists it and runs a command in it.
2. Blue sees none of it: an empty list, and 404 on every route by name and by
   ID. Blue creates a machine of the same name and gets its own; the
   administrator sees both and reaches red's by its ID.
3. A key without the machines scope is refused (403); a second machine of a
   name in one team is refused (409).
4. Stop and start through the control plane keep the machine's disk.
5. Red deletes its machine once stopped; blue's of the same name remains.

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
    report = {"success": False, "purpose": "local KVM functional verification of machines through the control plane; no performance comparison",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red_a", "red_b", "blue", "unscoped", "admin", "token"]}
    env = {"PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "RUST_LOG": "warn"}
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
        value = api("POST", f"/machines/{guest}/exec", {"cmd": cmd, "timeout_secs": 30}, key=key)
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

    with tempfile.TemporaryDirectory(prefix="hm-machines-cluster-", dir="/var/tmp") as temporary:
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
            namespace = "machines-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5)
            expiry = int(time.time()) + 1800
            policy = [
                {"sha256": hashlib.sha256(keys["red_a"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "templates", "machines"],
                 "principal_id": "red-a", "team_id": "red"},
                {"sha256": hashlib.sha256(keys["red_b"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "templates", "machines"],
                 "principal_id": "red-b", "team_id": "red"},
                {"sha256": hashlib.sha256(keys["blue"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory", "templates", "machines"],
                 "principal_id": "blue-a", "team_id": "blue"},
                {"sha256": hashlib.sha256(keys["unscoped"].encode()).hexdigest(), "expires_at": expiry,
                 "scopes": ["sandboxes", "inventory"], "principal_id": "red-c", "team_id": "red"},
            ]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
                           "--machine-dir", str(directory / "machines"),
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "machines-kvm-node",
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

            def placed():
                created = api("POST", "/machines", {"name": "web-01", "diskGiB": 1}, status=201)
                assert created["state"] == "running" and created["teamID"] == "red", created
                state["red"] = created["machineID"]
                listed = api("GET", "/machines", key="red_b")
                assert [(m["name"], m["nodeID"]) for m in listed] == [("web-01", "machines-kvm-node")], listed
                assert run("web-01", "echo from-red-b", key="red_b") == "from-red-b"
                assert "root=/dev/vda" in run("web-01", "cat /proc/cmdline")
                return {"machineID": created["machineID"], "nodeID": listed[0]["nodeID"]}
            case("a team's machine is placed on a node and reached by the whole team", placed)

            def isolated():
                assert api("GET", "/machines", key="blue") == []
                for reference in ["web-01", state["red"]]:
                    for method, suffix, body in [("GET", "", None), ("DELETE", "", None),
                                                 ("POST", "/exec", {"cmd": "true"}), ("POST", "/stop", None),
                                                 ("GET", "/console", None), ("GET", "/network/decisions", None)]:
                        api(method, f"/machines/{reference}{suffix}", body, key="blue", status=404)
                created = api("POST", "/machines", {"name": "web-01", "diskGiB": 1}, key="blue", status=201)
                assert created["teamID"] == "blue" and created["machineID"] != state["red"], created
                state["blue"] = created["machineID"]
                run("web-01", "echo red-only > /root/note && sync")
                assert run("web-01", "ls /root", key="blue") == ""
                everything = api("GET", "/machines", key="admin")
                assert ids(everything, "machineID") == sorted([state["red"], state["blue"]]), everything
                assert run(state["red"], "cat /root/note", key="admin") == "red-only"
                api("GET", "/machines/web-01", key="admin", status=404)
                return {"red": state["red"], "blue": state["blue"]}
            case("another team sees and reaches none of it, and has its own machine of the name", isolated)

            def refused():
                api("GET", "/machines", key="unscoped", status=403)
                api("POST", "/machines/web-01/exec", {"cmd": "true"}, key="unscoped", status=403)
                api("POST", "/machines", {"name": "web-01", "diskGiB": 1}, key="red_b", status=409)
                api("POST", "/machines", {"name": "pinned", "diskGiB": 1, "nodeID": "no-such-node"}, status=503)
                return {"without_scope": 403, "duplicate": 409, "unknown_node": 503}
            case("a key without the scope, a taken name and an unknown node are refused", refused)

            def keeps_its_disk():
                assert api("POST", "/machines/web-01/stop")["state"] == "stopped"
                api("POST", "/machines/web-01/exec", {"cmd": "true"}, status=409)
                assert api("POST", "/machines/web-01/start", key="red_b")["state"] == "running"
                assert run("web-01", "cat /root/note") == "red-only"
                return {"note": "red-only"}
            case("stop and start through the control plane keep its disk", keeps_its_disk)

            def deleted():
                api("DELETE", "/machines/web-01", status=409)
                api("POST", "/machines/web-01/stop")
                api("DELETE", "/machines/web-01", status=204)
                api("GET", "/machines/web-01", status=404)
                assert api("GET", "/machines", key="red_b") == []
                assert ids(api("GET", "/machines", key="blue"), "machineID") == [state["blue"]]
                assert run("web-01", "echo still-here", key="blue") == "still-here"
                return {"remaining": state["blue"]}
            case("a team deletes its machine and the other team's remains", deleted)
            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
