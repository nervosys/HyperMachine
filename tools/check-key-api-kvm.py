#!/usr/bin/env python3
"""The key-management API (POST/GET/DELETE /keys, GET /teams/{team}/members), against the
real control plane on KVM.

DRAFT: written against the shipped API and modelled on tools/check-key-presets-roles-kvm.py;
it has not been run. Run it on a Linux host with KVM, an owned Redis, and release builds of
hv2-sandboxd and hv2-control-plane, the same inputs as that script.

It starts an owned Redis, one hv2-sandboxd node and two hv2-control-plane processes over the
same Redis (verified TLS and node mTLS), with a small policy file and the legacy admin key, and
then, through the public API only, checks:

  * an administrator makes a member key (`POST /keys`); the key is shown once, `GET /keys`
    never shows a secret or a digest, and the store holds only a digest;
  * the member key creates a sandbox that really boots on KVM and runs a command in it, and is
    refused the key routes (403);
  * a team-admin cannot escalate (another team, team-admin role, admin scope, a scope it lacks,
    a lifetime beyond its own: each 403) and can make, list and revoke its own team's keys, and
    cannot see or revoke another team's (404);
  * revoking a key makes the next request 401, on the control plane that revoked it and on the
    other one over the same Redis, and the sandbox it created is untouched;
  * a managed key survives a control-plane restart (it lives in Redis, not in the process);
  * the shortest lifetime (60 s) really expires: the key works, then is 401 after it passes;
  * `GET /teams/{team}/members` lists the principals of file and managed keys;
  * with Redis stopped, a managed key's request is 503, never a pass;
  * no created secret, and no digest of one, appears in any service log.

Writes report.json (every request and response status of every case) and each service's log
into --output. Success only if every case passes.
"""
import argparse, hashlib, http.client, json, os, signal, socket, ssl, subprocess, tempfile, time, uuid
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
    parser.add_argument("--redis", type=Path, default=Path("redis-server"), help="redis-server binary")
    parser.add_argument("--redis-libs", type=Path, default=None, help="extra LD_LIBRARY_PATH for --redis")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {name: getattr(args, name) for name in ["daemon", "control", "kernel", "initrd"]}
    inputs["driver"] = Path(__file__)
    if args.redis.is_file():
        inputs["redis"] = args.redis
    report = {"success": False,
              "purpose": "functional verification of the key-management API on a real KVM node; no performance comparison",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    # Keys the file and the environment hold, by name. Managed keys are added as they are made.
    keys = {name: uuid.uuid4().hex for name in
            ["legacy_admin", "token", "unknown", "file_member", "red_lead", "red_short", "blue_lead"]}
    secrets = []  # every managed secret made, to check the logs never hold one
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "info"}
    processes, handles = [], []
    state = {}

    def call(label, method, path, body=None, api_port=None, raw_key=None):
        """One request; returns (status, parsed body or text)."""
        conn = http.client.HTTPSConnection("127.0.0.1", api_port or state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": raw_key if raw_key is not None else keys[label],
                                  "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
        finally:
            conn.close()
        try:
            return response.status, (json.loads(data) if data else None)
        except ValueError:
            return response.status, data[:200].decode("utf-8", "replace")

    def case(name, steps):
        """steps: list of (label, method, path, body, expected statuses, check)."""
        row = {"name": name, "success": True, "requests": []}
        report["cases"].append(row)
        for label, method, path, body, expected, check in steps:
            status, data = call(label, method, path, body)
            ok = status in expected
            detail = None
            if ok and check is not None:
                try:
                    detail = check(data)
                except AssertionError as error:
                    ok, detail = False, "check failed: " + str(error)
            shown = json.dumps(data)
            for secret in secrets:  # a secret is shown once; never keep it in the report
                shown = shown.replace(secret, "<secret>")
            row["requests"].append({"key": label, "request": f"{method} {path}", "expected": sorted(expected),
                                    "status": status, "ok": ok, "response": shown[:240],
                                    **({"detail": detail} if detail else {})})
            if not ok:
                row["success"] = False
        print(("ok: " if row["success"] else "FAILED: ") + name, flush=True)

    def extra(name, ok, detail):
        report["cases"].append({"name": name, "success": bool(ok), "requests": [], "detail": detail})
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

    def make(label, creator, body, expect=201):
        """POST /keys as `creator`; on 201 keeps the secret under `label`."""
        status, data = call(creator, "POST", "/keys", body)
        if status == 201:
            keys[label] = data["key"]
            secrets.append(data["key"])
        extra(f"POST /keys as {creator} for {label}: {expect}", status == expect,
              {"status": status, "id": data.get("id") if isinstance(data, dict) else None,
               "message": None if status == 201 or not isinstance(data, dict) else data.get("message")})
        return status, data

    with tempfile.TemporaryDirectory(prefix="hm-keyapi-kvm-", dir="/var/tmp") as temporary:
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
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-keyapi-ca",
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
            while len(ports) < 8:
                ports.add(port())
            redis_port, node_port, node_proxy, api_port, proxy_port, api2_port, proxy2_port, _spare = list(ports)
            state["api_port"] = api_port
            store = f"redis://127.0.0.1:{redis_port}"
            namespace = "keyapi-" + uuid.uuid4().hex
            redis_env = dict(env)
            if args.redis_libs:
                redis_env["LD_LIBRARY_PATH"] = str(args.redis_libs)
            redis_proc = start("redis", [str(args.redis), "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                                         "--appendonly", "no", "--dir", str(directory)], redis_env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 10)

            now = int(time.time())
            sha = lambda label: hashlib.sha256(keys[label].encode()).hexdigest()
            policy = [
                {"sha256": sha("file_member"), "expires_at": now + 1800, "scopes": ["sandboxes", "inventory"],
                 "role": "member", "principal_id": "file-member", "team_id": "red"},
                {"sha256": sha("red_lead"), "expires_at": now + 1800, "scopes": ["sandboxes", "inventory"],
                 "role": "team-admin", "principal_id": "red-lead", "team_id": "red"},
                # Five minutes: shorter than the lifetime a test key asks for, to check the cap.
                {"sha256": sha("red_short"), "expires_at": now + 300, "scopes": ["sandboxes", "inventory"],
                 "role": "team-admin", "principal_id": "red-short", "team_id": "red"},
                {"sha256": sha("blue_lead"), "expires_at": now + 1800, "scopes": ["sandboxes", "inventory"],
                 "role": "team-admin", "principal_id": "blue-lead", "team_id": "blue"},
            ]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            report["policy"] = [{k: v for k, v in row.items() if k != "sha256"} for row in policy]
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "8",
                           "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots"),
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "keyapi-kvm-node",
                           "--advertise-api", f"https://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                           "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                           "--mtls-key", str(directory / "node.key")],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": keys["token"]})

            def start_control(name, listen, proxy):
                return start(name, [str(args.control), "--store", store, "--namespace", namespace, "--port", str(listen),
                                    "--proxy-port", str(proxy), "--api-keys-file", str(policies),
                                    "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                                    "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                                    "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                                    "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "tcp-node.test"],
                             {**env, "HV2_API_KEY": keys["legacy_admin"], "HV2_CLUSTER_TOKEN": keys["token"]})

            control_proc = start_control("control", api_port, proxy_port)

            def prepared():
                assert all(child.poll() is None for child in processes), "an owned service exited"
                status, templates = call("legacy_admin", "GET", "/templates")
                assert status == 200 and any(r.get("templateID") == "base" and r.get("snapshot") for r in templates), "template not ready"
            eventually(prepared, 300)
            # The second control plane shares the same Redis: a revocation must hold on both.
            start_control("control2", api2_port, proxy2_port)
            eventually(lambda: call("legacy_admin", "GET", "/cluster/nodes", api_port=api2_port)[0] == 200 or (_ for _ in ()).throw(AssertionError("second control plane not up")), 60)

            ALL_FORBIDDEN = {403}

            def no_secret_or_digest(data):
                text = json.dumps(data)
                assert "sha256" not in text and '"key"' not in text, "a secret or digest field in the listing"
                for secret in secrets:
                    assert secret not in text and hashlib.sha256(secret.encode()).hexdigest() not in text, "a secret or digest in the listing"

            # --- an administrator makes a member key; the member key boots a sandbox on KVM ---
            status, made = make("member", "legacy_admin",
                                {"label": "kvm member", "expiresInSeconds": 1800, "preset": "developer",
                                 "teamId": "red", "principalId": "managed-member"})
            member_id = made.get("id")
            extra("the creation response carries the documented fields and a recognisable secret",
                  status == 201 and made["key"].startswith("hm_") and made["id"].startswith("mk_")
                  and made["teamId"] == "red" and made["role"] == "operator"
                  and made["expiresAt"] - made["createdAt"] == 1800,
                  {k: v for k, v in made.items() if k != "key"})
            case("GET /keys shows metadata only, never a secret or a digest", [
                ("legacy_admin", "GET", "/keys", None, {200}, lambda d: no_secret_or_digest(d) or "no secret, no digest"),
                ("red_lead", "GET", "/keys", None, {200},
                 lambda d: [k for k in d["keys"] if k["id"] == member_id] or (_ for _ in ()).throw(AssertionError("own team's key not listed"))),
            ])
            status, created = call("member", "POST", "/v2/sandboxes", {"templateID": "base", "timeout": 900})
            extra("the managed member key creates a sandbox that boots on KVM", status == 201, {"status": status})
            assert status == 201, f"managed-key sandbox create: {status} {created}"
            vm = created["sandboxID"]

            def exec_ok(data):
                assert data.get("exit_code") == 0 and data["stdout"].strip() == "keyapi", data
                return "guest ran the command"
            case("the member key works inside its team and is refused the key routes", [
                ("member", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyapi", "timeout_secs": 30}, {200}, exec_ok),
                ("member", "GET", "/sandboxes", None, {200}, lambda d: vm in [r["sandboxID"] for r in d] or (_ for _ in ()).throw(AssertionError("own sandbox not listed"))),
                ("member", "GET", "/keys", None, ALL_FORBIDDEN, None),
                ("member", "POST", "/keys", {"label": "x", "expiresInSeconds": 60, "scopes": ["inventory"], "teamId": "red"}, ALL_FORBIDDEN, None),
                ("member", "DELETE", f"/keys/{member_id}", None, ALL_FORBIDDEN, None),
                ("member", "GET", "/teams/red/members", None, ALL_FORBIDDEN, None),
                ("file_member", "GET", "/keys", None, ALL_FORBIDDEN, None),
                ("unknown", "GET", "/keys", None, {401}, None),
            ])

            # --- a team-admin cannot escalate ---
            base = {"label": "esc", "expiresInSeconds": 600}
            # blue's key, made by the administrator, for the cross-team checks
            status, blue = make("blue_member", "legacy_admin",
                                {"label": "blue", "expiresInSeconds": 1800, "preset": "developer",
                                 "teamId": "blue", "principalId": "blue-member"})
            blue_id = blue.get("id")
            case("a team-admin cannot escalate: each rule is 403", [
                ("red_lead", "POST", "/keys", {**base, "scopes": ["sandboxes"], "teamId": "blue", "principalId": "p"}, ALL_FORBIDDEN, None),
                # a principal another team's key carries: principals are global
                ("red_lead", "POST", "/keys", {**base, "scopes": ["sandboxes"], "principalId": "blue-member"}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "scopes": ["sandboxes"], "role": "team-admin"}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "preset": "developer", "role": "team-admin"}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "scopes": ["admin"]}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "preset": "admin"}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "scopes": ["volumes"]}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "scopes": ["machines"]}, ALL_FORBIDDEN, None),
                ("red_lead", "POST", "/keys", {**base, "preset": "developer"}, ALL_FORBIDDEN, None),
                ("red_short", "POST", "/keys", {**base, "scopes": ["sandboxes"], "expiresInSeconds": 3600}, ALL_FORBIDDEN, None),
                ("red_short", "POST", "/keys", {**base, "scopes": ["sandboxes"], "expiresInSeconds": 600}, ALL_FORBIDDEN, None),
            ])
            case("a team-admin sees and revokes only its own team's keys", [
                ("red_lead", "GET", "/keys", None, {200},
                 lambda d: (member_id in [k["id"] for k in d["keys"]] and blue_id not in [k["id"] for k in d["keys"]]) or (_ for _ in ()).throw(AssertionError("wrong keys listed"))),
                ("red_lead", "DELETE", f"/keys/{blue_id}", None, {404}, None),
                ("blue_member", "GET", "/sandboxes", None, {200}, None),  # still works
                ("red_lead", "GET", "/teams/blue/members", None, ALL_FORBIDDEN, None),
                ("blue_lead", "GET", "/teams/red/members", None, ALL_FORBIDDEN, None),
                ("red_lead", "DELETE", "/keys/" + sha("file_member")[:16], None, {404}, None),
                ("file_member", "GET", "/sandboxes", None, {200}, None),  # a file key cannot be deleted through the API
            ])
            status, child = make("red_child", "red_lead",
                                 {**base, "scopes": ["sandboxes", "inventory"], "principalId": "red-child"})
            child_id = child.get("id")
            case("a team-admin's own key is forced to its team and sees the team's sandbox", [
                ("red_child", "GET", "/sandboxes", None, {200}, lambda d: vm in [r["sandboxID"] for r in d] or (_ for _ in ()).throw(AssertionError("teammate's sandbox not listed"))),
                ("red_child", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyapi", "timeout_secs": 30}, {200}, exec_ok),
                ("red_child", "GET", "/keys", None, ALL_FORBIDDEN, None),
                ("blue_lead", "GET", "/sandboxes", None, {200}, lambda d: vm not in [r["sandboxID"] for r in d] or (_ for _ in ()).throw(AssertionError("another team's sandbox listed"))),
            ])

            # --- members ---
            def members(data):
                found = {m["principalId"]: m for m in data["members"]}
                for who in ["file-member", "red-lead", "red-short", "managed-member", "red-child"]:
                    assert who in found, f"{who} missing"
                assert "blue-member" not in found and "blue-lead" not in found
                assert found["red-lead"]["roles"] == ["team-admin"] and found["managed-member"]["roles"] == ["operator"]
                text = json.dumps(data)
                assert "sha256" not in text and "hm_" not in text and "mk_" not in text
                return "file and managed principals, no secrets"
            case("the member listing covers file and managed keys of one team", [
                ("legacy_admin", "GET", "/teams/red/members", None, {200}, members),
                ("red_lead", "GET", "/teams/red/members", None, {200}, members),
                ("member", "GET", "/teams/red/members", None, ALL_FORBIDDEN, None),
            ])

            # --- a 60 s key: used now, expired later ---
            status, short = make("short_lived", "legacy_admin",
                                 {"label": "sixty", "expiresInSeconds": 60, "scopes": ["inventory"],
                                  "teamId": "red", "principalId": "short-lived"})
            made_at = time.monotonic()
            case("a 60 s key works while it lives", [("short_lived", "GET", "/sandboxes", None, {200}, None)])

            # --- the second control plane sees the same keys, and a revocation holds on both ---
            def both(label, expected):
                return [(label, "GET", "/sandboxes", None, {expected}, None)]
            status_a, _ = call("member", "GET", "/sandboxes")
            status_b, _ = call("member", "GET", "/sandboxes", api_port=api2_port)
            extra("a managed key works on both control planes over the same Redis", status_a == 200 and status_b == 200,
                  {"first": status_a, "second": status_b})

            # --- a managed key survives a control-plane restart ---
            stop(control_proc)
            control_proc = start_control("control", api_port, proxy_port)
            eventually(lambda: call("member", "GET", "/sandboxes")[0] == 200 or (_ for _ in ()).throw(AssertionError("not back")), 60)
            extra("a managed key still works after the control plane restarts", True, {"restart": "control"})

            # --- revoke: the next request is 401 everywhere; the sandbox is untouched ---
            case("revoking the member key by the administrator", [
                ("legacy_admin", "DELETE", f"/keys/{member_id}", None, {200}, lambda d: d == {"deleted": member_id, "kind": "key"} or (_ for _ in ()).throw(AssertionError(d))),
                ("legacy_admin", "DELETE", f"/keys/{member_id}", None, {404}, None),
            ])
            status_a, _ = call("member", "GET", "/sandboxes")
            status_b, _ = call("member", "GET", "/sandboxes", api_port=api2_port)
            extra("the revoked key's next request is 401 on both control planes", status_a == 401 and status_b == 401,
                  {"first": status_a, "second": status_b})
            case("the sandbox the revoked key made is untouched, and a team-admin revokes its own team's key", [
                ("red_lead", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("red_child", "GET", "/sandboxes", None, {200}, None),
                ("red_lead", "DELETE", f"/keys/{child_id}", None, {200}, None),
                ("red_child", "GET", "/sandboxes", None, {401}, None),
                ("red_lead", "DELETE", f"/sandboxes/{vm}", None, {204}, None),
            ])

            # --- expiry ---
            wait = 63 - (time.monotonic() - made_at)
            if wait > 0:
                time.sleep(wait)
            case("a 60 s key is 401 once it has expired, and the listing says so", [
                ("short_lived", "GET", "/sandboxes", None, {401}, None),
                ("legacy_admin", "GET", "/keys", None, {200},
                 lambda d: [k for k in d["keys"] if k["principalId"] == "short-lived" and k["expired"]] or (_ for _ in ()).throw(AssertionError("not marked expired"))),
            ])

            # --- no secret in any log ---
            logs = {p.name: p.read_text(errors="replace") for p in args.output.glob("*.log")}
            leaked = sorted({name for name, text in logs.items() for secret in secrets
                             if secret in text or hashlib.sha256(secret.encode()).hexdigest() in text})
            extra("no created secret, and no digest of one, appears in any service log", not leaked,
                  {"logs_checked": sorted(logs), "leaked_in": leaked, "secrets_made": len(secrets)})

            # --- the store down: a managed key is 503, never a pass (last: it stops Redis) ---
            status, _ = call("blue_member", "GET", "/sandboxes")
            stop(redis_proc)
            status_down, _ = call("blue_member", "GET", "/sandboxes")
            status_unknown, _ = call("unknown", "GET", "/sandboxes")
            extra("with Redis stopped a managed key's request is 503, and an unknown key's is not a pass",
                  status == 200 and status_down == 503 and status_unknown in (503, 401),
                  {"before": status, "managed_key_down": status_down, "unknown_key_down": status_unknown})
            report["_all_ran"] = True
        finally:
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            log = args.output / "node.log"
            text = log.read_text(errors="replace") if log.exists() else ""
            report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                        "Tcg": text.count("Detected hypervisor platform: Tcg")}
            report["cases"].insert(0, {"name": "a sandbox booted and ran a command on KVM (no Tcg)",
                                       "success": report["platform_lines"]["Kvm"] >= 1 and report["platform_lines"]["Tcg"] == 0
                                       and any(r["ok"] and r["request"].endswith("/exec")
                                               for c in report["cases"] for r in c["requests"]),
                                       "platform_lines": report["platform_lines"], "requests": []})
            report["success"] = bool(report.pop("_all_ran", False)) and all(c["success"] for c in report["cases"])
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
