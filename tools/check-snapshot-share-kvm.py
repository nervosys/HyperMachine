#!/usr/bin/env python3
"""A team shares a snapshot with another team, through a real control plane, on KVM.

Runs an owned Redis, one hv2-sandboxd node (with a snapshot store) and the
hv2-control-plane over verified TLS and node mTLS, with a red key, a blue key,
a green key and the legacy administrator. Through the control plane's API only:

1. Red writes a file in a sandbox and snapshots it. Before any share, blue
   and green cannot list the snapshot or start from it (404).
2. Sharing without `acknowledgeContents` is refused (400) and shares nothing.
3. Red shares it with blue (`PUT /templates/{id}/shares/blue`). Blue lists it,
   starts a sandbox from it and finds red's file; that sandbox is blue's, and
   red cannot reach it. Green still can neither list nor start from it.
4. Blue cannot read the share list, share it on to green, unshare it, or
   delete the snapshot (404 each); a share with the owning team itself is
   refused (409), a malformed team label is 400, and an operator's template
   (`base`) cannot be shared (404, it is not a team's snapshot).
5. After the node restarts, the share is still there: blue starts from it.
6. Red revokes the share (204). Blue can no longer list or start from it, and
   the sandbox blue started earlier keeps running.

Writes report.json and each service's log into --output. Needs root, Redis and
openssl on the host.
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
            process.wait(timeout=15)
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
    report = {"success": False, "purpose": "local KVM functional verification of sharing a snapshot between teams; no timing claim",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "green", "admin", "token"]}
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

    def call(method, path, body=None, key="red"):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
            return response.status, (json.loads(data) if data else None)
        finally:
            conn.close()

    def api(method, path, body=None, key="red", status=200):
        got, value = call(method, path, body, key)
        assert got in ((status,) if isinstance(status, int) else status), \
            f"{key} {method} {path}: {got}, expected {status}: {value!r}"
        return value

    def sh(box, command, key="red"):
        value = api("POST", f"/sandboxes/{box}/exec", {"cmd": command, "timeout_secs": 30}, key=key)
        assert value["exit_code"] == 0, value
        return value["stdout"].strip()

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.3)

    def lists(key, name):
        snaps = api("GET", "/snapshots", key=key)
        templates = api("GET", "/templates", key=key)
        return (any(r["snapshotID"].startswith(name) for r in snaps),
                any(str(r.get("templateID", "")).startswith(name) for r in templates))

    def start_from(key, name, status):
        got, value = call("POST", "/v2/sandboxes", {"templateID": name, "timeout": 600}, key)
        assert got == status, (key, name, got, value)
        return value["sandboxID"] if status == 201 else None

    with tempfile.TemporaryDirectory(prefix="hm-snapshot-share-", dir="/var/tmp") as temporary:
        directory = Path(temporary)

        def start(name, argv, environment):
            handle = (args.output / (name + ".log")).open("ab")
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
            namespace = "share-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5)
            expiry = int(time.time()) + 3600
            scopes = ["sandboxes", "inventory", "templates", "events"]
            policy = [{"sha256": hashlib.sha256(keys[t].encode()).hexdigest(), "expires_at": expiry,
                       "scopes": scopes, "principal_id": t + "-a", "team_id": t} for t in ["red", "blue", "green"]]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))

            def start_node():
                return start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                                      "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
                                      "--snapshot-store", str(directory / "snapshots"),
                                      "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "share-node",
                                      "--advertise-api", f"https://127.0.0.1:{node_port}",
                                      "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                                      "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                                      "--mtls-key", str(directory / "node.key")],
                             {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd),
                              "HV2_CLUSTER_TOKEN": keys["token"]})
            node = start_node()
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

            def red_snapshot():
                red = start_from("red", "base", 201)
                sh(red, "echo red-state > /root/marker", key="red")
                api("POST", f"/sandboxes/{red}/snapshots", {"name": "red-snap"}, key="red", status=201)
                state["red_sandbox"] = red
                assert lists("red", "red-snap") == (True, True), lists("red", "red-snap")
                for key in ["blue", "green"]:
                    assert lists(key, "red-snap") == (False, False), (key, lists(key, "red-snap"))
                    start_from(key, "red-snap", 404)
                return {"before_share": "hidden from blue and green"}
            case("before a share, only red can list or start from its snapshot", red_snapshot)

            def needs_acknowledgement():
                for body in [None, {}, {"acknowledgeContents": False}]:
                    got, value = call("PUT", "/templates/red-snap/shares/blue", body, "red")
                    assert got == 400 and "environment" in json.dumps(value), (got, value)
                start_from("blue", "red-snap", 404)
                return {"refused": 3}
            case("a share without acknowledgeContents is refused and shares nothing", needs_acknowledgement)

            def shared_with_blue():
                shared = api("PUT", "/templates/red-snap/shares/blue", {"acknowledgeContents": True})
                assert shared["sharedWith"] == ["blue"] and shared["teamID"] == "red", shared
                assert api("GET", "/templates/red-snap/shares")["sharedWith"] == ["blue"]
                eventually(lambda: lists("blue", "red-snap") == (True, True) or (_ for _ in ()).throw(AssertionError("blue lists")), 30)
                blue = eventually(lambda: start_from("blue", "red-snap", 201), 30)
                state["blue_sandbox"] = blue
                assert sh(blue, "cat /root/marker", key="blue") == "red-state"
                got, _ = call("GET", f"/sandboxes/{blue}", key="red")
                assert got in (403, 404), got
                assert lists("green", "red-snap") == (False, False)
                start_from("green", "red-snap", 404)
                return {"blue_sandbox": blue}
            case("red shares it with blue: blue lists it, starts from it and finds red's file; green still cannot", shared_with_blue)

            def blue_cannot_manage():
                for method, path, body in [("GET", "/templates/red-snap/shares", None),
                                           ("PUT", "/templates/red-snap/shares/green", {"acknowledgeContents": True}),
                                           ("DELETE", "/templates/red-snap/shares/blue", None),
                                           ("DELETE", "/templates/red-snap", None)]:
                    got, value = call(method, path, body, "blue")
                    assert got == 404, (method, path, got, value)
                assert lists("green", "red-snap") == (False, False)
                got, _ = call("PUT", "/templates/red-snap/shares/red", {"acknowledgeContents": True}, "red")
                assert got == 409, got
                got, _ = call("PUT", "/templates/red-snap/shares/not%20a%20team", {"acknowledgeContents": True}, "red")
                assert got == 400, got
                got, _ = call("PUT", "/templates/base/shares/blue", {"acknowledgeContents": True}, "red")
                assert got == 404, got
                assert api("GET", "/templates/red-snap/shares")["sharedWith"] == ["blue"]
                return {"refused": "404 x4, 409, 400, 404"}
            case("blue cannot read, extend, revoke or delete the share; bad shares are refused", blue_cannot_manage)

            def survives_restart():
                stop(node)
                processes.remove(node)
                nonlocal_node = start_node()
                state["node"] = nonlocal_node
                eventually(prepared, 120)
                started = eventually(lambda: start_from("blue", "red-snap", 201), 60)
                assert sh(started, "cat /root/marker", key="blue") == "red-state"
                # The sandbox from before the restart is gone with the node's
                # memory; this one is what the revoke is checked against.
                state["blue_sandbox"] = started
                assert api("GET", "/templates/red-snap/shares")["sharedWith"] == ["blue"]
                return {"after_restart": "blue started from it"}
            case("the share is still there after the node restarts", survives_restart)

            def revoked():
                api("DELETE", "/templates/red-snap/shares/blue", status=204)
                eventually(lambda: lists("blue", "red-snap") == (False, False) or (_ for _ in ()).throw(AssertionError("still listed")), 30)
                start_from("blue", "red-snap", 404)
                assert api("GET", "/templates/red-snap/shares")["sharedWith"] == []
                assert sh(state["blue_sandbox"], "echo still-running", key="blue") == "still-running"
                assert start_from("red", "red-snap", 201)
                return {"blue_sandbox_still_runs": True}
            case("revoking removes blue's access, and the sandbox blue started keeps running", revoked)
            report["success"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
