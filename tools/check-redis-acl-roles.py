#!/usr/bin/env python3
"""Per-role Redis ACL (security finding O12), against the real control plane and node.

Without roles, a node and the control planes share one Redis user, so a node with Redis write
access can DEL a managed API key's sealed revocation record (the tombstone) and write the
key's old record back, which revives a revoked key. tools/redis-acl.py gives each role its own
Redis user and takes the key and tombstone keyspace away from nodes. This starts an owned
Redis 7+ with that generated ACL (default user off), one hv2-sandboxd node as `hv2-node` and
hv2-control-plane as `hv2-control-plane` over verified TLS and node mTLS, and checks:

  * the ACL works for real: a managed key is made and used, the node registers and is listed
    live by the control plane, a managed-key sandbox boots on KVM and runs a command, and the
    key is revoked (401); neither service logs a NOPERM;
  * as the node user, every attack on the key/tombstone keyspace is refused with NOPERM (HDEL
    of the tombstone, DEL of the tombstone hash, HSET of the old key record, a Lua script that
    does either, and even a read), and the revoked key is still 401;
  * the gateway user can read what it routes by and write nothing;
  * CONTROL: on a second managed key, a legacy shared user (the old setup, one user with the
    whole namespace) does the same HDEL and HSET and the revoked key answers 200 again.

Writes report.json and each service's log into --output. Success only if every case passes.
"""
import argparse, hashlib, http.client, json, os, socket, ssl, subprocess, sys, tempfile, time, uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent


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
    parser.add_argument("--redis", type=Path, default=Path("redis-server"), help="redis-server binary (7+)")
    parser.add_argument("--redis-cli", type=Path, default=Path("redis-cli"), help="redis-cli binary")
    parser.add_argument("--redis-libs", type=Path, default=None, help="extra LD_LIBRARY_PATH for the redis binaries")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {name: getattr(args, name) for name in ["daemon", "control", "kernel", "initrd"]}
    inputs["driver"] = Path(__file__)
    inputs["acl_generator"] = HERE / "redis-acl.py"
    if args.redis.is_file():
        inputs["redis"] = args.redis
    report = {"success": False,
              "purpose": "functional verification of per-role Redis ACLs on a real KVM node; no performance comparison",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    token = uuid.uuid4().hex
    legacy_admin = uuid.uuid4().hex
    passwords = {name: os.urandom(24).hex() for name in ["control_plane", "node", "gateway", "admin", "shared"]}
    secrets = [token, legacy_admin] + list(passwords.values())
    managed = {}
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "info"}
    processes, handles = [], []
    state = {}

    def redact(text):
        for secret in secrets + list(managed.values()):
            text = text.replace(secret, "<secret>")
        return text

    def call(secret, method, path, body=None):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": secret, "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
        finally:
            conn.close()
        try:
            return response.status, (json.loads(data) if data else None)
        except ValueError:
            return response.status, data[:200].decode("utf-8", "replace")

    def check(name, ok, detail):
        report["cases"].append({"name": name, "success": bool(ok), "detail": detail})
        print(("ok: " if ok else "FAILED: ") + name, flush=True)

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    def cli(user, *argv):
        """One redis-cli command as `user`; returns the output with the exit code."""
        cli_env = dict(env)
        who = []
        if user != "default":  # the default user is off: an unauthenticated client
            cli_env["REDISCLI_AUTH"] = passwords[user.replace("hv2-", "").replace("-", "_")]
            who = ["--user", user]
        if args.redis_libs:
            cli_env["LD_LIBRARY_PATH"] = str(args.redis_libs)
        done = subprocess.run([str(args.redis_cli), "-p", str(state["redis_port"]), *who, "--no-auth-warning",
                               *argv], env=cli_env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, timeout=20)
        return done.stdout.decode("utf-8", "replace").strip()

    with tempfile.TemporaryDirectory(prefix="hm-redis-acl-", dir="/var/tmp") as temporary:
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
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-redisacl-ca",
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
            state["api_port"], state["redis_port"] = api_port, redis_port
            namespace = "redisacl-" + uuid.uuid4().hex[:12]
            prefix = f"hv2:{namespace}:"

            # The ACL under test, from the generator; plus the harness's own admin, and the
            # legacy shared user (the old setup) for the control. The default user is off.
            generated = subprocess.run(
                [sys.executable, str(HERE / "redis-acl.py"), "--namespace", namespace],
                env={**env, "HV2_ACL_CONTROL_PLANE_PASSWORD": passwords["control_plane"],
                     "HV2_ACL_NODE_PASSWORD": passwords["node"], "HV2_ACL_GATEWAY_PASSWORD": passwords["gateway"]},
                check=True, stdout=subprocess.PIPE).stdout.decode()
            sha = lambda value: hashlib.sha256(value.encode()).hexdigest()
            aclfile = (generated
                       + f"user hv2-admin on #{sha(passwords['admin'])} ~* &* +@all\n"
                       + f"user hv2-shared on #{sha(passwords['shared'])} ~{prefix}* +@all\n")
            (directory / "users.acl").write_text(aclfile)
            (args.output / "users.acl").write_text(aclfile)  # hashes only, no password

            redis_env = dict(env)
            if args.redis_libs:
                redis_env["LD_LIBRARY_PATH"] = str(args.redis_libs)
            start("redis", [str(args.redis), "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory), "--aclfile", str(directory / "users.acl")], redis_env)
            eventually(lambda: socket.create_connection(("127.0.0.1", redis_port), timeout=0.2).close(), 10)
            version = cli("hv2-admin", "INFO", "server")
            report["redis_version"] = next((l.split(":", 1)[1] for l in version.splitlines() if l.startswith("redis_version")), "?")
            report["acl_list"] = [l for l in cli("hv2-admin", "ACL", "LIST").splitlines()]
            check("the default user is off: an unauthenticated client can do nothing",
                  "NOAUTH" in cli("default", "PING") or "WRONGPASS" in cli("default", "PING") or "disabled" in cli("default", "PING").lower()
                  or "NOPERM" in cli("default", "PING"), {"ping_as_default": cli("default", "PING")[:120]})

            def url(role, name):
                return f"redis://{role}:{passwords[name]}@127.0.0.1:{redis_port}"
            secret_file = directory / "managed-key-secret"
            secret_file.write_text(os.urandom(48).hex())
            secret_file.chmod(0o600)
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "8",
                           "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots"),
                           "--cluster-store", url("hv2-node", "node"), "--cluster-namespace", namespace,
                           "--node-id", "redisacl-kvm-node",
                           "--advertise-api", f"https://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                           "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                           "--mtls-key", str(directory / "node.key")],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": token})
            start("control", [str(args.control), "--store", url("hv2-control-plane", "control_plane"), "--namespace", namespace,
                              "--port", str(api_port), "--proxy-port", str(proxy_port),
                              "--managed-key-secret-file", str(secret_file),
                              "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                              "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                              "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                              "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "tcp-node.test"],
                  {**env, "HV2_API_KEY": legacy_admin, "HV2_CLUSTER_TOKEN": token})

            def prepared():
                assert all(child.poll() is None for child in processes), "an owned service exited"
                status, templates = call(legacy_admin, "GET", "/templates")
                assert status == 200 and any(r.get("templateID") == "base" and r.get("snapshot") for r in templates), "template not ready"
            eventually(prepared, 300)

            # --- a: the roles suffice: the control plane makes and revokes keys, the node serves them ---
            def make(label):
                status, data = call(legacy_admin, "POST", "/keys", {"label": label, "expiresInSeconds": 1800, "preset": "developer",
                                                                  "teamId": "red", "principalId": "p-" + label})
                assert status == 201, f"create key {label}: {status} {data}"
                managed[label] = data["key"]
                return data["id"], data["key"]
            id1, key1 = make("acl")
            status, nodes = call(legacy_admin, "GET", "/cluster/nodes")
            check("the node (as hv2-node) joined and the control plane (as hv2-control-plane) lists it live",
                  status == 200 and any(n.get("id") == "redisacl-kvm-node" or n.get("nodeId") == "redisacl-kvm-node"
                                        or "redisacl-kvm-node" in json.dumps(n) for n in (nodes if isinstance(nodes, list) else nodes.get("nodes", []))),
                  {"status": status, "nodes": redact(json.dumps(nodes))[:300]})
            status, created = call(key1, "POST", "/v2/sandboxes", {"templateID": "base", "timeout": 900})
            check("a managed key creates a sandbox through the control plane; it boots on the node", status == 201, {"status": status})
            assert status == 201, f"sandbox create: {status} {created}"
            vm = created["sandboxID"]
            status, out = call(key1, "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo redisacl", "timeout_secs": 30})
            check("a command runs in it through the control plane",
                  status == 200 and out.get("exit_code") == 0 and out["stdout"].strip() == "redisacl", {"status": status})
            status, _ = call(key1, "DELETE", f"/sandboxes/{vm}")
            check("the sandbox is deleted through the control plane (the node removes its records)", status == 204, {"status": status})
            status, _ = call(key1, "GET", "/sandboxes")
            check("the managed key works before revocation", status == 200, {"status": status})
            sha1 = sha(key1)
            old_record = cli("hv2-admin", "HGET", prefix + "managed-keys", sha1)
            check("(harness) the key's sealed record is in Redis, as read by the admin", old_record.startswith("{"), {"record_bytes": len(old_record)})
            status, _ = call(legacy_admin, "DELETE", f"/keys/{id1}")
            check("the control plane revokes the key (DELETE /keys/{id})", status == 200, {"status": status})
            status, _ = call(key1, "GET", "/sandboxes")
            check("the revoked key is 401", status == 401, {"status": status})
            tomb = cli("hv2-admin", "HEXISTS", prefix + "managed-key-tombstones", id1)
            check("(harness) the revocation record exists in Redis", tomb == "1", {"hexists": tomb})

            # --- b: the node user is refused every attack on the key keyspace ---
            tombs, keysh = prefix + "managed-key-tombstones", prefix + "managed-keys"
            attacks = [
                ("HDEL tombstone", ["HDEL", tombs, id1]),
                ("DEL tombstone hash", ["DEL", tombs]),
                ("HSET old key record back", ["HSET", keysh, sha1, old_record]),
                ("HGET key record (even a read)", ["HGET", keysh, sha1]),
                ("HGETALL tombstones (even a read)", ["HGETALL", tombs]),
                ("Lua HDEL tombstone", ["EVAL", "return redis.call('HDEL',KEYS[1],ARGV[1])", "1", tombs, id1]),
                ("Lua HSET old key record", ["EVAL", "return redis.call('HSET',KEYS[1],ARGV[1],ARGV[2])", "1", keysh, sha1, old_record]),
                ("FLUSHALL", ["FLUSHALL"]),
                ("KEYS *", ["KEYS", "*"]),
            ]
            for label, argv in attacks:
                out = cli("hv2-node", *argv)
                check(f"as hv2-node, {label} is refused", out.startswith("NOPERM") or "NOPERM" in out, {"reply": redact(out)[:200]})
            for label, argv in attacks[:3]:
                out = cli("hv2-gateway", *argv)
                check(f"as hv2-gateway, {label} is refused", "NOPERM" in out, {"reply": redact(out)[:200]})
            still = cli("hv2-admin", "HEXISTS", tombs, id1)
            check("(harness) the tombstone is still there afterwards", still == "1", {"hexists": still})
            status, _ = call(key1, "GET", "/sandboxes")
            check("the revoked key is still 401 after the node-user attacks", status == 401, {"status": status})

            # the node's own families still work, and the gateway reads what it routes by
            node_write = cli("hv2-node", "HSET", prefix + "environments", "probe", "{}")
            check("as hv2-node, writing its own families (environments) is allowed", node_write in ("1", "0"), {"reply": node_write})
            cli("hv2-admin", "HDEL", prefix + "environments", "probe")
            gw_read = cli("hv2-gateway", "SMEMBERS", prefix + "nodes")
            check("as hv2-gateway, reading nodes is allowed", "NOPERM" not in gw_read and "redisacl-kvm-node" in gw_read, {"reply": gw_read[:120]})
            gw_write = cli("hv2-gateway", "SADD", prefix + "nodes", "forged")
            check("as hv2-gateway, writing nodes is refused", "NOPERM" in gw_write, {"reply": gw_write[:200]})
            gw_other = cli("hv2-gateway", "HGETALL", prefix + "environments")
            check("as hv2-gateway, a family it does not use is refused", "NOPERM" in gw_other, {"reply": gw_other[:200]})
            gw_eval = cli("hv2-gateway", "EVAL", "return redis.call('SADD',KEYS[1],ARGV[1])", "1", prefix + "nodes", "forged")
            check("as hv2-gateway, a Lua script that writes is refused", "NOPERM" in gw_eval or "ERR" in gw_eval, {"reply": gw_eval[:200]})

            # --- c: CONTROL: the old shared user does the same and the key comes back ---
            id2, key2 = make("shared")
            sha2 = sha(key2)
            old2 = cli("hv2-admin", "HGET", keysh, sha2)
            status, _ = call(legacy_admin, "DELETE", f"/keys/{id2}")
            status2, _ = call(key2, "GET", "/sandboxes")
            check("(control) a second key is revoked and is 401", status == 200 and status2 == 401, {"revoke": status, "after": status2})
            deleted = cli("hv2-shared", "HDEL", tombs, id2)
            written = cli("hv2-shared", "HSET", keysh, sha2, old2)
            check("(control) as the legacy shared user the same HDEL and HSET succeed", deleted == "1" and written == "1",
                  {"hdel_reply": deleted, "hset_reply": written})
            status, _ = call(key2, "GET", "/sandboxes")
            check("(control) the revoked key answers 200 again: the residual was real without role ACLs", status == 200, {"status": status})

            # --- neither service was refused by Redis while serving all of the above ---
            for name in ["node", "control"]:
                text = (args.output / (name + ".log")).read_text(errors="replace")
                check(f"the {name} log has no NOPERM", "NOPERM" not in text, {"noperm_lines": text.count("NOPERM")})
            logs = {p.name: p.read_text(errors="replace") for p in args.output.glob("*.log")}
            leaked = sorted({n for n, t in logs.items() for s in secrets + list(managed.values()) if s in t})
            check("no password or key appears in any service log", not leaked, {"leaked_in": leaked})
            report["_all_ran"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            text = (args.output / "node.log").read_text(errors="replace") if (args.output / "node.log").exists() else ""
            report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                        "Tcg": text.count("Detected hypervisor platform: Tcg")}
            report["cases"].insert(0, {"name": "the sandbox booted on KVM (no Tcg)",
                                       "success": report["platform_lines"]["Kvm"] >= 1 and report["platform_lines"]["Tcg"] == 0,
                                       "detail": report["platform_lines"]})
            report["success"] = bool(report.pop("_all_ran", False)) and all(c["success"] for c in report["cases"])
            (args.output / "report.json").write_text(redact(json.dumps(report, indent=2)) + "\n")
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
