#!/usr/bin/env python3
"""Compose `healthcheck:` and `depends_on: condition: service_healthy`, on KVM.

Runs an owned Redis, one hv2-sandboxd node with `--network` and a guest kit (it
pulls `alpine:3.20` itself; no Docker anywhere), the hv2-control-plane over
verified TLS and node mTLS, and hv2-native-gateway. Then the shipped
`hm sandbox vm compose` is driven with these files:

1. `db` becomes ready ~20 s after it starts (`test -f /tmp/ready`); `app` waits
   for it with `condition: service_healthy`. `up` returns only after that
   (it took at least 18 s), and the file exists in `db` when `up` returns.
2. The same files with a plain `depends_on: [db]` return well before that
   (control: the wait is the condition's doing, not start-up cost).
3. A `db` whose check always fails (`false`, 2 s, 2 retries) makes `up` fail
   saying the service is unhealthy, and no sandbox is left behind.
4. `service_healthy` on a service with no `healthcheck` is refused before
   anything is created.
5. No API key or cluster token is in anything `hm` printed.

`restart:` is not supported and is not exercised.

Writes report.json and each service's log into --output. Needs root, Redis,
openssl and curl on the host.
"""
import argparse, hashlib, http.client, json, os, re, socket, ssl, subprocess
import tempfile, time, uuid
from pathlib import Path

def compose_text(delay, dep, test):
    return f"""services:
  db:
    image: alpine:3.20
    command: ["sh", "-c", "sleep {delay}; touch /tmp/ready; sleep 3600"]
    healthcheck:
      test: {test}
      interval: 2s
      retries: 30
  app:
    image: alpine:3.20
    command: ["sleep", "3600"]
    depends_on: {dep}
"""


NOCHECK = """\
services:
  db: {image: alpine:3.20}
  app:
    image: alpine:3.20
    depends_on:
      db:
        condition: service_healthy
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
              "purpose": "functional verification of compose health checks; the timings are bounds, not measurements",
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

    work = Path(tempfile.mkdtemp(prefix="hm-compose-health-", dir="/var/tmp"))
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
        namespace = "chealth-" + uuid.uuid4().hex
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
                       "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "chealth-node",
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

        found = {}

        def guest(sandbox, command, seconds=30, key="red"):
            _, out = api("POST", f"/sandboxes/{sandbox}/exec", key=key, body={"cmd": command, "timeout_secs": seconds})
            return out

        def write(name, text):
            path = work / name
            path.write_text(text)
            return path

        healthy = write("healthy.yaml", compose_text(20, "{db: {condition: service_healthy}}", '["CMD-SHELL", "test -f /tmp/ready"]'))

        def waits_for_health():
            began = time.monotonic()
            done = hm("red", "compose", "-f", str(healthy), "-p", "wait", "up")
            took = time.monotonic() - began
            require(done.returncode == 0, (done.returncode, done.stdout[-400:], done.stderr[-800:]))
            services = {s["service"]: s["sandboxID"] for s in json.loads(done.stdout)["services"]}
            require(sorted(services) == ["app", "db"], services)
            require(took >= 18, f"up returned after {took:.1f}s, before db could be healthy")
            out = guest(services["db"], "test -f /tmp/ready; echo rc=$?")
            require("rc=0" in out.get("stdout", ""), out)
            return {"up_seconds": round(took, 1)}
        case("up waits until the dependency's health check passes before starting the dependent", waits_for_health)

        def down(project, file):
            done = hm("red", "compose", "-f", str(file), "-p", project, "down")
            require(done.returncode == 0, (done.returncode, done.stderr[-600:]))
            _, rows = api("GET", "/sandboxes", key="admin")
            require(rows == [], rows)

        case("down removes both sandboxes", lambda: down("wait", healthy))

        plain = write("plain.yaml", compose_text(20, "[db]", '["CMD-SHELL", "test -f /tmp/ready"]'))

        def plain_does_not_wait():
            began = time.monotonic()
            done = hm("red", "compose", "-f", str(plain), "-p", "plain", "up")
            took = time.monotonic() - began
            require(done.returncode == 0, (done.returncode, done.stderr[-800:]))
            require(took < 15, f"plain depends_on took {took:.1f}s")
            down("plain", plain)
            return {"up_seconds": round(took, 1)}
        case("the same files with a plain depends_on do not wait (control)", plain_does_not_wait)

        broken = write("broken.yaml", compose_text(1, "{db: {condition: service_healthy}}", '["CMD-SHELL", "false"]').replace("retries: 30", "retries: 2"))

        def unhealthy_fails():
            done = hm("red", "compose", "-f", str(broken), "-p", "bad", "up")
            require(done.returncode != 0, done.stdout[-300:])
            require("unhealthy" in done.stderr, done.stderr[-600:])
            _, rows = api("GET", "/sandboxes", key="admin")
            require(rows == [], rows)
            return {"stderr": done.stderr[-300:]}
        case("a dependency that never turns healthy fails `up` and leaves no sandbox", unhealthy_fails)

        nocheck = write("nocheck.yaml", NOCHECK)

        def refused():
            done = hm("red", "compose", "-f", str(nocheck), "-p", "none", "up")
            require(done.returncode != 0 and "no healthcheck" in done.stderr, (done.returncode, done.stderr[-400:]))
            _, rows = api("GET", "/sandboxes", key="admin")
            require(rows == [], rows)
        case("service_healthy on a service with no healthcheck is refused before anything is created", refused)

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
