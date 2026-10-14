#!/usr/bin/env python3
"""Today's sandbox routes through the control plane, on one real KVM cluster.

Runs an owned Redis, one hv2-sandboxd node and the hv2-control-plane over
verified TLS and node mTLS, with a red key and a blue key on two teams. Then,
through the control plane's API only, as red:

1. A sandbox is resized, and forked at another size; each runs at its own
   (POST .../resize, POST .../fork with cpuCount and memoryMB).
2. Its guest's processes are listed (GET .../processes), init among them.
3. Its commands are kept and read again (GET .../commands, .../commands/{n}).
4. A checkpoint of another sandbox is listed and read from outside
   (GET .../checkpoints/{name}/files and .../file).
5. A program in its guest reaches a port of the caller's, upgraded through
   the control plane (GET .../ports/{port}/reverse, hv2-reverse/1).
6. Blue reaches none of it: every one of those routes on red's sandbox is
   refused.

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
    report = {"success": False,
              "purpose": "local KVM functional verification of sandbox routes through the control plane; no timing claim",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "admin", "token"]}
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

    def api(method, path, body=None, key="red", status=200):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=180)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(4 << 20)
            wanted = status if isinstance(status, tuple) else (status,)
            assert response.status in wanted, f"{key} {method} {path}: {response.status}, expected {status}: {data[:300]!r}"
            try:
                return response.status, (json.loads(data) if data else None)
            except ValueError:
                return response.status, data
        finally:
            conn.close()

    def run(sandbox, cmd, key="red"):
        _, value = api("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30}, key=key)
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

    def create(key="red", **body):
        _, created = api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 900, **body}, key=key, status=201)
        return created["sandboxID"]

    def upgrade(path, key="red", protocol="hv2-reverse/1"):
        raw = socket.create_connection(("127.0.0.1", state["api_port"]), timeout=60)
        sock = state["context"].wrap_socket(raw, server_hostname="127.0.0.1")
        sock.sendall((f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: Upgrade\r\n"
                      f"Upgrade: {protocol}\r\nx-api-key: {keys[key]}\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = sock.recv(1)
            assert chunk, head
            head += chunk
        return sock, int(head.split(b" ", 2)[1]), head.decode(errors="replace")

    with tempfile.TemporaryDirectory(prefix="hm-cluster-routes-", dir="/var/tmp") as temporary:
        directory = Path(temporary)

        def start(name, argv, environment):
            handle = (args.output / (name + ".log")).open("wb")
            handles.append(handle)
            child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle,
                                     stderr=subprocess.STDOUT)
            processes.append(child)
            return child

        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)

        try:
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=cluster-routes-ca",
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
            namespace = "routes-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5)
            expiry = int(time.time()) + 3600
            policy = [{"sha256": hashlib.sha256(keys[k].encode()).hexdigest(), "expires_at": expiry,
                       "scopes": ["sandboxes", "inventory", "templates"], "principal_id": f"{k}-a", "team_id": k}
                      for k in ["red", "blue"]]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "8",
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "routes-node",
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
                _, templates = api("GET", "/templates", key="admin")
                assert any(row.get("templateID") == "base" and row.get("snapshot") for row in templates), "template not ready"
            eventually(prepared, 300)

            def sizes():
                sandbox = create()
                state["a"] = sandbox
                _, resized = api("POST", f"/sandboxes/{sandbox}/resize", {"cpuCount": 2, "memoryMB": 512})
                assert resized["resized"] is True, resized
                assert run(sandbox, "grep -c ^processor /proc/cpuinfo") == "2"
                _, forks = api("POST", f"/sandboxes/{sandbox}/fork", {"count": 1, "cpuCount": 1, "memoryMB": 256,
                                                                     "timeout": 600}, status=201)
                fork = forks[0]["sandbox"]["sandboxID"]
                state["fork"] = fork
                assert run(fork, "grep -c ^processor /proc/cpuinfo") == "1"
                return {"resized": resized, "fork": fork}
            case("a sandbox is resized and forked at another size through the control plane", sizes)

            def processes_listed():
                _, listed = api("GET", f"/sandboxes/{state['a']}/processes")
                pids = [p["pid"] for p in listed["processes"]]
                assert 1 in pids, pids
                return {"count": len(pids)}
            case("its guest's processes are listed through the control plane", processes_listed)

            def history():
                run(state["a"], "echo kept-by-history")
                _, listed = api("GET", f"/sandboxes/{state['a']}/commands")
                kept = [c for c in listed if c["cmd"] == "echo kept-by-history"]
                assert kept, listed
                _, one = api("GET", f"/sandboxes/{state['a']}/commands/{kept[0]['n']}")
                assert one["stdout"]["text"] == "kept-by-history\n", one
                return {"commands": len(listed)}
            case("its commands are kept and read again through the control plane", history)

            def checkpoint_files():
                # A sandbox at its template's size: one of another size has
                # no checkpoint.
                sandbox = create()
                state["b"] = sandbox
                run(sandbox, "mkdir -p /root/notes && echo as-taken > /root/notes/a.txt")
                api("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "cp"}, status=(200, 201, 204))
                run(sandbox, "echo changed > /root/notes/a.txt")
                _, listed = api("GET", f"/sandboxes/{sandbox}/checkpoints/cp/files?path=/root/notes")
                assert [e["name"] for e in listed["entries"]] == ["a.txt"], listed
                _, body = api("GET", f"/sandboxes/{sandbox}/checkpoints/cp/file?path=/root/notes/a.txt")
                body = body if isinstance(body, bytes) else json.dumps(body).encode()
                assert body == b"as-taken\n", body
                return {"read": body.decode()}
            case("a checkpoint is listed and read from outside through the control plane", checkpoint_files)

            def reverse():
                sock, status, head = upgrade(f"/sandboxes/{state['b']}/ports/7000/reverse")
                assert status == 101 and "hv2-reverse/1" in head, head
                result = {}
                program = threading.Thread(target=lambda: result.update(stdout=run(
                    state["b"], "printf 'hello through the plane\\n' | nc -w 5 127.0.0.1 7000")))
                program.start()
                sock.settimeout(60)
                assert sock.recv(1) == b"+"
                got = b""
                while b"\n" not in got:
                    got += sock.recv(4096)
                assert got == b"hello through the plane\n", got
                sock.sendall(b"hello back\n")
                sock.close()
                program.join(timeout=60)
                assert "hello back" in result.get("stdout", ""), result
                return {"both_ways": True}
            case("a guest program reaches the caller's port, upgraded through the control plane", reverse)

            def blue_refused():
                refused = {}
                for label, method, path, body in [
                    ("resize", "POST", f"/sandboxes/{state['a']}/resize", {"cpuCount": 1}),
                    ("fork", "POST", f"/sandboxes/{state['a']}/fork", {"count": 1}),
                    ("processes", "GET", f"/sandboxes/{state['a']}/processes", None),
                    ("commands", "GET", f"/sandboxes/{state['a']}/commands", None),
                    ("checkpoint file", "GET", f"/sandboxes/{state['b']}/checkpoints/cp/file?path=/root/notes/a.txt", None),
                ]:
                    status, _ = api(method, path, body, key="blue", status=(403, 404))
                    refused[label] = status
                _, status, head = upgrade(f"/sandboxes/{state['b']}/ports/7001/reverse", key="blue")
                assert status in (403, 404), head
                refused["reverse"] = status
                return {"refused": refused}
            case("another team reaches none of it", blue_refused)

            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
