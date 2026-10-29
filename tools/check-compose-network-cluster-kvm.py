#!/usr/bin/env python3
"""Compose services reach each other by service name, on KVM.

Runs an owned Redis, one hv2-sandboxd node with `--network` and a guest kit (it
pulls `alpine:3.20` itself; no Docker anywhere), the hv2-control-plane over
verified TLS and node mTLS, and hv2-native-gateway. A red and a blue key carry
a `principal_id`. Then the shipped `hm sandbox vm compose` is driven against
the control plane with a two-service file (`server` listens, `client` idles):

1. `up` joins both sandboxes to the private network named after the project
   (the API shows the tag on each).
2. In `client`, `nslookup server` (no domain typed; the search line expands it)
   answers with a private-network address.
3. In `client`, a TCP connection to `server:8080` gets its bytes echoed back.
4. The name does not resolve for another principal's sandbox in a project of
   the same name (a blue `up` of the same file, project `other`, cannot look
   `server.web` up), and `down` removes both sandboxes.
5. No API key or cluster token is in anything `hm` printed.

Two nodes, UDP and a name that is also a sandbox ID are not exercised.

Writes report.json and each service's log into --output. Needs root, Redis,
openssl and curl on the host.
"""
import argparse, hashlib, http.client, json, os, re, socket, ssl, subprocess
import tempfile, time, uuid
from pathlib import Path

SERVICE = """\
services:
  server:
    image: alpine:3.20
    command: ["sh", "-c", "nc -ll -p 8080 -e /bin/cat"]
  client:
    image: alpine:3.20
    command: ["sleep", "3600"]
    depends_on: [server]
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

    work = Path(tempfile.mkdtemp(prefix="hm-compose-network-", dir="/var/tmp"))
    state["work"] = work
    try:
        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)

        openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-network-ca",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(work / "ca.key"), "-out", str(work / "ca.pem")])
        for name, usage, san in [("node", "serverAuth,clientAuth", "DNS:tcp-node.test,DNS:hv2-node,IP:127.0.0.1"),
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
        namespace = "cnet-" + uuid.uuid4().hex
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
                       "--network", "--guest-kit", str(args.kit), "--volume-dir", str(work / "volumes"),
                       "--snapshot-store", str(work / "snapshots"),
                       "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "cnet-node",
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

        def guest(sandbox, command, seconds=30, key="red"):
            _, out = api("POST", f"/sandboxes/{sandbox}/exec", key=key, body={"cmd": command, "timeout_secs": seconds})
            return out

        def up_joins():
            done = hm("red", "compose", "-f", str(compose), "-p", "web", "up")
            require(done.returncode == 0, (done.returncode, done.stdout[-400:], done.stderr[-800:]))
            services = {s["service"]: s["sandboxID"] for s in json.loads(done.stdout)["services"]}
            require(sorted(services) == ["client", "server"], services)
            found.update(services)
            tags = {}
            for service, sandbox in services.items():
                _, row = api("GET", f"/sandboxes/{sandbox}/private-networks")
                require(row["tags"] == ["web"], (service, row))
                tags[service] = row["tags"]
            return {"tags": tags}
        case("up joins both services to the private network named after the project", up_joins)

        def resolves():
            def look():
                out = guest(found["client"], "nslookup server.web.hv2.internal; echo rc=$?")
                text = out.get("stdout", "") + out.get("stderr", "")
                if not re.search(r"Address[^\n]*198\.18\.\d+\.\d+", text):
                    by_id = guest(found["client"], f"nslookup {found['server']}.web.hv2.internal; echo rc=$?")
                    raise AssertionError(text[-300:] + " | by sandbox id: " + by_id.get("stdout", "")[-300:])
                return text
            return {"answer": eventually(look, 60).strip().splitlines()[-4:]}
        case("the full service name resolves to a private-network address", resolves)

        def short_name_connects():
            def talk():
                out = guest(found["client"], "echo hello-from-client | nc -w 3 server 8080; echo rc=$?")
                text = out.get("stdout", "")
                if "hello-from-client" not in text:
                    more = guest(found["client"],
                                 "cat /etc/resolv.conf; ls -l /etc/resolv.conf; "
                                 "echo full-name:; echo hi | nc -w 3 server.web.hv2.internal 8080; echo rc=$?; "
                                 "echo short-lookup:; nslookup server 2>&1; echo rc=$?")
                    raise AssertionError((text[-200:], out.get("stderr", "")[-200:], more.get("stdout", "")[-900:],
                                          more.get("stderr", "")[-300:]))
                return text
            eventually(talk, 60)
            return {"bytes": len("hello-from-client")}
        case("a TCP connection to the bare name `server` (the search line expands it) is echoed back",
             short_name_connects)

        def other_project():
            done = hm("blue", "compose", "-f", str(compose), "-p", "other", "up")
            require(done.returncode == 0, (done.returncode, done.stderr[-800:]))
            blue = {s["service"]: s["sandboxID"] for s in json.loads(done.stdout)["services"]}
            out = guest(blue["client"], "nslookup server.web.hv2.internal; echo rc=$?", key="blue")
            text = out.get("stdout", "") + out.get("stderr", "")
            require(not re.search(r"Address[^\n]*198\.18\.", text), text[-400:])
            hm("blue", "compose", "-f", str(compose), "-p", "other", "down")
            return {"answer": text.strip().splitlines()[-3:]}
        case("a different principal's project cannot resolve the first project's service", other_project)

        def down_removes():
            done = hm("red", "compose", "-f", str(compose), "-p", "web", "down")
            require(done.returncode == 0, (done.returncode, done.stderr[-600:]))
            _, rows = api("GET", "/sandboxes", key="admin")
            require(rows == [], rows)
        case("down removes both sandboxes", down_removes)

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
