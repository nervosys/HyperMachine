#!/usr/bin/env python3
"""A machine's reverse port through the control plane, on one real KVM cluster.

Runs an owned Redis, one hv2-sandboxd node with a --machine-dir and the
hv2-control-plane over verified TLS and node mTLS, with two teams' keys.
Then, through the control plane's API only:

1. Red creates a machine, and opens `GET /machines/web-01/ports/7000/reverse`
   as an upgrade. A program in the machine connects to its own port 7000
   with `nc`: the caller receives `+`, then the program's line, and what the
   caller sends back reaches the program.
2. Refusals, none of which reach the machine: a wrong key (401), a key
   without the machines scope (403), port 0 (400), a plain GET that is not an
   upgrade (400), and another team's machine by name and by ID, and a machine
   that is not there (404).
3. The machine stopped: the node's 409 is the answer.

Writes report.json and each service's log into --output.
"""
import argparse, hashlib, http.client, json, socket, ssl, subprocess, tempfile, threading, time, uuid
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

    with tempfile.TemporaryDirectory(prefix="hm-machine-reverse-cluster-", dir="/var/tmp") as temporary:
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

            def made():
                created = api("POST", "/machines", {"name": "web-01", "diskGiB": 1}, status=201)
                assert created["state"] == "running" and created["teamID"] == "red", created
                state["red"] = created["machineID"]
                assert run("web-01", "echo up") == "up"
                return {"machineID": created["machineID"]}
            case("a team's machine is placed on a node", made)

            def upgrade(path, key="red_a", protocol="hv2-reverse/1", asked=True):
                raw = socket.create_connection(("127.0.0.1", state["api_port"]), timeout=30)
                sock = state["context"].wrap_socket(raw, server_hostname="localhost")
                lines = [f"GET {path} HTTP/1.1", "Host: localhost", f"x-api-key: {keys[key]}"]
                if asked:
                    lines += ["Connection: Upgrade", f"Upgrade: {protocol}"]
                sock.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
                head = b""
                while b"\r\n\r\n" not in head:
                    chunk = sock.recv(1)
                    assert chunk, head
                    head += chunk
                return sock, int(head.split(b" ", 2)[1]), head.decode(errors="replace")

            def carried():
                sock, status, head = upgrade("/machines/web-01/ports/7000/reverse")
                assert status == 101 and "hv2-reverse/1" in head, head
                result = {}
                program = threading.Thread(target=lambda: result.update(
                    api("POST", "/machines/web-01/exec",
                        {"cmd": "printf 'hello from the machine\\n' | nc -w 5 127.0.0.1 7000", "timeout_secs": 30})))
                began = time.monotonic()
                program.start()
                sock.settimeout(30)
                assert sock.recv(1) == b"+"
                got = b""
                while b"\n" not in got:
                    got += sock.recv(4096)
                took = round(time.monotonic() - began, 3)
                assert got == b"hello from the machine\n", got
                sock.sendall(b"hello from the caller\n")
                sock.close()
                program.join(timeout=30)
                assert "hello from the caller" in result.get("stdout", ""), result
                return {"seconds_to_first_bytes": took}
            case("through the control plane: a machine program's connection reaches the caller, both ways", carried)

            def refused():
                seen = {}
                for label, path, kwargs, want in [
                    ("wrong_key", "/machines/web-01/ports/7000/reverse", {"key": "token"}, 401),
                    ("without_scope", "/machines/web-01/ports/7000/reverse", {"key": "unscoped"}, 403),
                    ("port_zero", "/machines/web-01/ports/0/reverse", {}, 400),
                    ("not_an_upgrade", "/machines/web-01/ports/7000/reverse", {"asked": False}, 400),
                    ("other_team_by_name", "/machines/web-01/ports/7000/reverse", {"key": "blue"}, 404),
                    ("other_team_by_id", f"/machines/{state['red']}/ports/7000/reverse", {"key": "blue"}, 404),
                    ("no_such_machine", "/machines/nothing/ports/7000/reverse", {}, 404)]:
                    sock, status, head = upgrade(path, **kwargs)
                    sock.close()
                    assert status == want, (label, status, want, head)
                    seen[label] = status
                return seen
            case("refusals: key, scope, port 0, not an upgrade, another team's machine, no machine", refused)

            def stopped():
                assert api("POST", "/machines/web-01/stop")["state"] == "stopped"
                sock, status, head = upgrade("/machines/web-01/ports/7000/reverse")
                sock.close()
                assert status == 409, (status, head)
                return {"status": status}
            case("a stopped machine answers as its node does (409)", stopped)
            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
