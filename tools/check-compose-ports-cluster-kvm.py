#!/usr/bin/env python3
"""Compose `ports:` through a real control plane and native gateway, on KVM.

Runs an owned Redis, one hv2-sandboxd node (with a guest kit, so it pulls
`alpine:3.20` itself; no Docker anywhere), the hv2-control-plane over verified
TLS and node mTLS with `--native-port-range`, and hv2-native-gateway. A red
key and a blue key carry a `principal_id`; the control plane's own
administrator key does not. Then the shipped `hm sandbox vm compose` is driven
against the control plane:

1. `up` of a service with `ports: ["9000:8080"]` reserves container port 8080
   (the host side is dropped; the gateway picks the public number), prints the
   public port, and the reservation is the one the API and `public-ports list`
   show.
2. A TCP client of that public port on the gateway reaches the service's own
   listener in the guest and gets its bytes echoed back.
3. Another principal's key cannot see that sandbox's reservation.
4. `down` removes the sandbox and its reservation, and the public port stops
   accepting connections.
5. `up` with a key that has no principal (so it may not reserve) fails, says
   which port it was publishing, and leaves no sandbox behind.
6. No API key or cluster token is in anything `hm` printed.

UDP and `protocol: both` through compose are not exercised.

Writes report.json and each service's log into --output. Needs root, Redis,
openssl and curl on the host.
"""
import argparse, hashlib, http.client, json, os, re, socket, ssl, subprocess
import tempfile, time, uuid
from pathlib import Path

SERVICE = """\
services:
  echo:
    image: alpine:3.20
    command: ["sh", "-c", "nc -ll -p 8080 -e /bin/cat"]
    ports: ["9000:8080"]
"""


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def window(width):
    """A first port whose `width` successors are all free on loopback."""
    for first in range(41000, 60000, 97):
        held = []
        try:
            for value in range(first, first + width):
                for kind in (socket.SOCK_STREAM, socket.SOCK_DGRAM):
                    probe = socket.socket(socket.AF_INET, kind)
                    held.append(probe)
                    probe.bind(("127.0.0.1", value))
            return first
        except OSError:
            continue
        finally:
            for probe in held:
                probe.close()
    raise AssertionError("no free port window")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "gateway", "hm", "kernel", "initrd", "kit", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    require(os.geteuid() == 0 and Path("/dev/kvm").exists(), "root and KVM required")
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "cases": [],
              "purpose": "functional verification of compose ports: through the control plane and native gateway; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in
                               ["daemon", "control", "gateway", "hm", "kernel", "initrd"]}}
    report["input_sha256"]["driver"] = digest(__file__)
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "admin", "token"]}
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "info"}
    processes, handles = [], []
    state = {}
    printed = []  # everything hm printed

    def api(method, path, key="red", status=(200, 201), body=None):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
            wanted = (status,) if isinstance(status, int) else status
            require(response.status in wanted, f"{key} {method} {path}: {response.status}: {data[:300]!r}")
            return response.status, (json.loads(data) if data else None)
        finally:
            conn.close()

    def hm(key, *argv, timeout=1500):
        full = [str(args.hm), "sandbox", "vm", "--endpoint", f"https://127.0.0.1:{state['api_port']}",
                "--api-ca-cert", str(state["work"] / "ca.pem"), "--request-timeout", "300", *argv]
        done = subprocess.run(full, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL,
                              env={"PATH": env["PATH"], "HOME": str(args.output), "HV2_API_KEY": keys[key]})
        printed.extend([done.stdout, done.stderr])
        return done

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        detail = action()
        row["seconds"] = round(time.monotonic() - began, 3)
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
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    work = Path(tempfile.mkdtemp(prefix="hm-compose-ports-", dir="/var/tmp"))
    state["work"] = work
    try:
        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)

        openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-ports-ca",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(work / "ca.key"), "-out", str(work / "ca.pem")])
        for name, usage, san in [("node", "serverAuth", "DNS:tcp-node.test,IP:127.0.0.1"),
                                 ("control", "clientAuth", "DNS:tcp-control.test"),
                                 ("api", "serverAuth", "IP:127.0.0.1,DNS:localhost")]:
            openssl(["req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                     "-keyout", str(work / (name + ".key")), "-out", str(work / (name + ".csr"))])
            ext = work / (name + ".ext")
            ext.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                           f"extendedKeyUsage={usage}\nsubjectAltName={san}\n")
            openssl(["x509", "-req", "-in", str(work / (name + ".csr")), "-CA", str(work / "ca.pem"),
                     "-CAkey", str(work / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(ext),
                     "-out", str(work / (name + ".pem"))])
        state["context"] = ssl.create_default_context(cafile=str(work / "ca.pem"))

        def start(name, argv, environment):
            handle = (args.output / (name + ".log")).open("wb")
            handles.append(handle)
            child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle,
                                     stderr=subprocess.STDOUT)
            processes.append(child)
            return child

        ports = set()
        while len(ports) < 5:
            ports.add(port())
        redis_port, node_port, node_proxy, api_port, proxy_port = list(ports)
        state["api_port"] = api_port
        first = window(4)
        store = f"redis://127.0.0.1:{redis_port}"
        namespace = "cports-" + uuid.uuid4().hex
        start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                        "--appendonly", "no", "--dir", str(work)], env)

        def redis_ready():
            with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                pass
        eventually(redis_ready, 5)
        expiry = int(time.time()) + 3600
        scopes = ["sandboxes", "inventory", "volumes", "templates", "events"]
        policy = [
            {"sha256": hashlib.sha256(keys["red"].encode()).hexdigest(), "expires_at": expiry,
             "scopes": scopes, "principal_id": "red-a"},
            {"sha256": hashlib.sha256(keys["blue"].encode()).hexdigest(), "expires_at": expiry,
             "scopes": scopes, "principal_id": "blue-a"},
        ]
        (work / "keys.json").write_text(json.dumps(policy))
        start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                       "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
                       "--guest-kit", str(args.kit), "--volume-dir", str(work / "volumes"),
                       "--snapshot-store", str(work / "snapshots"),
                       "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "cports-node",
                       "--advertise-api", f"https://127.0.0.1:{node_port}",
                       "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                       "--mtls-ca", str(work / "ca.pem"), "--mtls-cert", str(work / "node.pem"),
                       "--mtls-key", str(work / "node.key")],
              {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd),
               "HV2_CLUSTER_TOKEN": keys["token"]})
        start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                          "--proxy-port", str(proxy_port), "--api-keys-file", str(work / "keys.json"),
                          "--native-port-range", f"{first}-{first + 3}",
                          "--api-tls-cert", str(work / "api.pem"), "--api-tls-key", str(work / "api.key"),
                          "--tls-cert", str(work / "api.pem"), "--tls-key", str(work / "api.key"),
                          "--mtls-ca", str(work / "ca.pem"), "--mtls-cert", str(work / "control.pem"),
                          "--mtls-key", str(work / "control.key"), "--mtls-node-name", "tcp-node.test"],
              {**env, "HV2_API_KEY": keys["admin"], "HV2_CLUSTER_TOKEN": keys["token"]})
        start("gateway", [str(args.gateway), "--bind-ip", "127.0.0.1", "--namespace", namespace,
                          "--mtls-ca", str(work / "ca.pem"), "--mtls-cert", str(work / "control.pem"),
                          "--mtls-key", str(work / "control.key"), "--mtls-node-name", "tcp-node.test",
                          "--poll-ms", "100"],
              {**env, "HV2_STORE_URL": store, "HV2_CLUSTER_TOKEN": keys["token"]})

        def prepared():
            require(all(child.poll() is None for child in processes), "an owned service exited")
            _, templates = api("GET", "/templates", key="admin")
            require(any(row.get("templateID") == "base" and row.get("snapshot") for row in templates),
                    "the node has not registered its template")
        eventually(prepared, 300)

        compose = work / "compose.yaml"
        compose.write_text(SERVICE)
        found = {}

        def up_publishes():
            done = hm("red", "compose", "-f", str(compose), "-p", "web", "up")
            require(done.returncode == 0, (done.returncode, done.stdout[-400:], done.stderr[-800:]))
            services = json.loads(done.stdout)["services"]
            require([s["service"] for s in services] == ["echo"], services)
            found["id"] = services[0]["sandboxID"]
            said = re.search(r"container port 8080/tcp is public port (\d+)", done.stderr)
            require(said, done.stderr[-600:])
            found["public"] = int(said.group(1))
            require(first <= found["public"] <= first + 3, (found["public"], first))
            _, rows = api("GET", f"/sandboxes/{found['id']}/public-ports")
            require(rows == [{"machinePort": 8080, "publicPort": found["public"], "protocol": "tcp"}], rows)
            listed = hm("red", "public-ports", "list", found["id"])
            require(listed.returncode == 0 and json.loads(listed.stdout) == rows, (listed.stdout, listed.stderr))
            return {"public_port": found["public"], "reservation": rows}
        case("up reserves the container port, and the API and the CLI show the same reservation", up_publishes)

        def reaches_the_service():
            payload = b"hello from outside\n"

            def echo():
                with socket.create_connection(("127.0.0.1", found["public"]), timeout=3) as client:
                    client.settimeout(5)
                    client.sendall(payload)
                    got = b""
                    while len(got) < len(payload):
                        chunk = client.recv(len(payload) - len(got))
                        require(chunk, "closed before the echo")
                        got += chunk
                    require(got == payload, got)
            eventually(echo, 60)
            return {"bytes": len(payload)}
        case("a TCP client of the public port reaches the service's listener and gets its bytes back",
             reaches_the_service)

        def other_team():
            status, _ = api("GET", f"/sandboxes/{found['id']}/public-ports", key="blue", status=(401, 403, 404))
            return {"blue_status": status}
        case("another principal's key cannot see the reservation", other_team)

        def down_releases():
            done = hm("red", "compose", "-f", str(compose), "-p", "web", "down")
            require(done.returncode == 0, (done.returncode, done.stdout[-400:], done.stderr[-600:]))
            status, _ = api("GET", f"/sandboxes/{found['id']}/public-ports", status=(404, 410))

            def closed():
                try:
                    socket.create_connection(("127.0.0.1", found["public"]), timeout=1).close()
                except OSError:
                    return
                raise AssertionError("the public port still accepts connections")
            eventually(closed, 30)
            return {"after_down_status": status}
        case("down removes the sandbox and its reservation, and the public port closes", down_releases)

        def no_principal_refused():
            done = hm("admin", "compose", "-f", str(compose), "-p", "nokey", "up")
            require(done.returncode != 0, "up succeeded without a principal")
            require("publishing port 8080" in done.stderr, done.stderr[-600:])
            _, rows = api("GET", "/sandboxes", key="admin")
            require(rows == [], rows)
            return {"stderr": done.stderr.strip().splitlines()[-1][:200]}
        case("up with a key that has no principal fails naming the port and leaves no sandbox", no_principal_refused)

        def no_secrets():
            text = "\n".join(printed)
            for name, value in keys.items():
                require(value not in text, f"hm printed the {name} credential")
            return {"hm_output_bytes": len(text)}
        case("no API key or cluster token is in anything hm printed", no_secrets)

        report["success"] = all(row["success"] for row in report["cases"])
    except BaseException as error:
        report["error"] = repr(error)[:600]
        raise
    finally:
        for child in reversed(processes):
            stop(child)
        for handle in handles:
            handle.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    require(report["success"], "a case failed")


if __name__ == "__main__":
    main()
