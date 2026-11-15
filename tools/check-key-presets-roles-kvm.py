#!/usr/bin/env python3
"""API key scopes, presets and key roles, against the real control plane on KVM.

What a key can do is set by its `scopes` (admin, inventory, sandboxes, templates,
volumes, events, machines, telemetry) and its `role` (operator, the default, or
observer), or by a named `preset` that expands to both (read-only, developer,
machines, admin). The first cases treat each scope as the nearest thing to a
preset; the "named preset" cases then use the real presets, and a policy file
with an unknown preset (at start and at SIGHUP reload). It runs an owned Redis,
one hv2-sandboxd node and hv2-control-plane over verified TLS and node mTLS,
issues one key per scope and per role, and then, through the control plane's
public API only, checks for each key one operation it must be allowed and
operations it must be refused (403), plus 401 for an unknown and an expired key.

The one sandbox is created with the `sandboxes` key and really boots on KVM; a
command runs in it. Refused operations need no VM.

Writes report.json (every request and response status of every case) and each
service's log into --output. Success only if every case passes.
"""
import argparse, hashlib, http.client, json, os, socket, ssl, subprocess, tempfile, time, uuid
from pathlib import Path

SCOPES = ["admin", "inventory", "sandboxes", "templates", "volumes", "events", "machines", "telemetry"]
TEAM_SCOPES = ["sandboxes", "inventory", "volumes", "templates", "events", "machines", "telemetry"]


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
              "purpose": "functional verification of API key scopes and roles on a real KVM node; no performance comparison",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {"legacy_admin": uuid.uuid4().hex, "token": uuid.uuid4().hex, "unknown": uuid.uuid4().hex,
            "expired": uuid.uuid4().hex, "operator": uuid.uuid4().hex, "observer": uuid.uuid4().hex,
            "observer_admin": uuid.uuid4().hex, "other_team": uuid.uuid4().hex,
            "viewer": uuid.uuid4().hex, "member": uuid.uuid4().hex, "team_admin": uuid.uuid4().hex,
            "team_admin_blue": uuid.uuid4().hex}
    PRESET_KEYS = ["read_only", "developer", "machines", "admin", "admin_observer"]
    for name in PRESET_KEYS:
        keys["preset_" + name] = uuid.uuid4().hex
    for scope in SCOPES:
        keys["scope_" + scope] = uuid.uuid4().hex
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "info"}
    processes, handles = [], []
    state = {}

    def call(label, method, path, body=None):
        """One request; returns (status, parsed body or text)."""
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[label], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
        finally:
            conn.close()
        try:
            return response.status, (json.loads(data) if data else None)
        except ValueError:
            return response.status, data[:200].decode("utf-8", "replace")

    def case(name, steps):
        """steps: list of (label, method, path, body, expected statuses, note)."""
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
            shown = json.dumps(data)[:240]
            row["requests"].append({"key": label, "request": f"{method} {path}", "expected": sorted(expected),
                                    "status": status, "ok": ok, "response": shown, **({"detail": detail} if detail else {})})
            if not ok:
                row["success"] = False
        print(("ok: " if row["success"] else "FAILED: ") + name, flush=True)

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    with tempfile.TemporaryDirectory(prefix="hm-keyroles-kvm-", dir="/var/tmp") as temporary:
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
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-keyroles-ca",
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
            namespace = "keyroles-" + uuid.uuid4().hex
            redis_env = dict(env)
            if args.redis_libs:
                redis_env["LD_LIBRARY_PATH"] = str(args.redis_libs)
            start("redis", [str(args.redis), "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], redis_env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 10)

            expiry = int(time.time()) + 1800
            sha = lambda label: hashlib.sha256(keys[label].encode()).hexdigest()
            policy = []
            for scope in SCOPES:
                row = {"sha256": sha("scope_" + scope), "expires_at": expiry, "scopes": [scope]}
                if scope != "admin":  # a team key needs a team and a principal; admin stays global
                    row.update(principal_id="k-" + scope, team_id="red")
                policy.append(row)
            policy += [
                {"sha256": sha("operator"), "expires_at": expiry, "scopes": ["sandboxes", "inventory"],
                 "principal_id": "role-operator", "team_id": "red"},
                {"sha256": sha("observer"), "expires_at": expiry, "scopes": ["inventory"], "role": "observer",
                 "principal_id": "role-observer", "team_id": "red"},
                {"sha256": sha("observer_admin"), "expires_at": expiry, "scopes": ["admin", "sandboxes", "inventory"],
                 "role": "observer", "principal_id": "role-observer-admin", "team_id": "red"},
                {"sha256": sha("other_team"), "expires_at": expiry, "scopes": ["sandboxes", "inventory"],
                 "principal_id": "blue-a", "team_id": "blue"},
                {"sha256": sha("viewer"), "expires_at": expiry, "scopes": ["sandboxes", "inventory"], "role": "viewer",
                 "principal_id": "team-viewer", "team_id": "red"},
                {"sha256": sha("member"), "expires_at": expiry, "scopes": ["sandboxes", "inventory"], "role": "member",
                 "principal_id": "team-member", "team_id": "red"},
                {"sha256": sha("team_admin"), "expires_at": expiry, "scopes": ["sandboxes", "inventory"],
                 "role": "team-admin", "principal_id": "red-lead", "team_id": "red"},
                {"sha256": sha("team_admin_blue"), "expires_at": expiry, "preset": "developer",
                 "role": "team-admin", "principal_id": "blue-lead", "team_id": "blue"},
                {"sha256": sha("expired"), "expires_at": 1, "scopes": ["sandboxes"],
                 "principal_id": "expired", "team_id": "red"},
                {"sha256": sha("preset_read_only"), "expires_at": expiry, "preset": "read-only",
                 "principal_id": "preset-read-only", "team_id": "red"},
                {"sha256": sha("preset_developer"), "expires_at": expiry, "preset": "developer",
                 "principal_id": "preset-developer", "team_id": "red"},
                {"sha256": sha("preset_machines"), "expires_at": expiry, "preset": "machines",
                 "principal_id": "preset-machines", "team_id": "red"},
                {"sha256": sha("preset_admin"), "expires_at": expiry, "preset": "admin"},
                {"sha256": sha("preset_admin_observer"), "expires_at": expiry, "preset": "admin",
                 "role": "observer", "principal_id": "preset-admin-observer", "team_id": "red"},
            ]
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            report["policy"] = [{k: v for k, v in row.items() if k != "sha256"} for row in policy]
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "8",
                           "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots"),
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "keyroles-kvm-node",
                           "--advertise-api", f"https://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                           "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                           "--mtls-key", str(directory / "node.key")],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": keys["token"]})
            control_proc = start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                              "--proxy-port", str(proxy_port), "--api-keys-file", str(policies),
                              "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                              "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                              "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                              "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "tcp-node.test"],
                  {**env, "HV2_API_KEY": keys["legacy_admin"], "HV2_CLUSTER_TOKEN": keys["token"]})

            def prepared():
                assert all(child.poll() is None for child in processes), "an owned service exited"
                status, templates = call("legacy_admin", "GET", "/templates")
                assert status == 200 and any(r.get("templateID") == "base" and r.get("snapshot") for r in templates), "template not ready"
            eventually(prepared, 300)

            # --- the one real sandbox, made by the `sandboxes` preset key ---
            status, created = call("scope_sandboxes", "POST", "/v2/sandboxes", {"templateID": "base", "timeout": 900})
            assert status == 201, f"sandbox create: {status} {created}"
            vm = created["sandboxID"]
            state["vm"] = vm
            # A second team's sandbox, for the cross-team refusals.
            status, created_blue = call("other_team", "POST", "/v2/sandboxes", {"templateID": "base", "timeout": 900})
            assert status == 201, f"blue sandbox create: {status} {created_blue}"
            blue_vm = created_blue["sandboxID"]

            def exec_ok(data):
                assert data.get("exit_code") == 0 and data["stdout"].strip() == "keyroles", data
                return "guest ran the command"
            def has_vm(data):
                assert vm in [r["sandboxID"] for r in data], "own sandbox not listed"
            def lacks_blue(data):
                assert blue_vm not in [r["sandboxID"] for r in data], "another team's sandbox listed"
            def lacks_vm(data):
                assert vm not in [r["sandboxID"] for r in data], "another team's sandbox listed"
            ALL = {200, 201, 204}
            F = {403}

            # Presets: each scope allowed one thing, refused others.
            case("preset admin: reaches everything, including operator-only routes", [
                ("scope_admin", "GET", "/cluster/nodes", None, {200}, None),
                ("scope_admin", "GET", "/sandboxes", None, {200}, has_vm),
                ("scope_admin", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("scope_admin", "GET", "/volumes", None, {200}, None),
                ("scope_admin", "GET", "/machines", None, {200}, None),
            ])
            case("preset inventory: lists, cannot read detail, create or reach other families", [
                ("scope_inventory", "GET", "/sandboxes", None, {200}, has_vm),
                ("scope_inventory", "GET", f"/sandboxes/{vm}", None, F, None),
                ("scope_inventory", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("scope_inventory", "GET", "/volumes", None, F, None),
            ])
            case("preset sandboxes: runs commands in its sandbox, refused other families", [
                ("scope_sandboxes", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("scope_sandboxes", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles", "timeout_secs": 30}, {200}, exec_ok),
                ("scope_sandboxes", "GET", "/volumes", None, F, None),
                ("scope_sandboxes", "GET", "/templates", None, F, None),
                ("scope_sandboxes", "GET", "/events/sandboxes", None, F, None),
                ("scope_sandboxes", "GET", "/machines", None, F, None),
            ])
            case("preset templates: lists templates, refused sandboxes and volumes", [
                ("scope_templates", "GET", "/templates", None, {200}, None),
                ("scope_templates", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("scope_templates", "GET", f"/sandboxes/{vm}", None, F, None),
                ("scope_templates", "GET", "/volumes", None, F, None),
            ])
            case("preset volumes: creates and lists volumes, refused sandboxes", [
                ("scope_volumes", "POST", "/volumes", {"name": "preset-vol"}, {201}, None),
                ("scope_volumes", "GET", "/volumes", None, {200}, lambda d: [r for r in d if r.get("name") == "preset-vol"] or (_ for _ in ()).throw(AssertionError("volume not listed"))),
                ("scope_volumes", "GET", f"/sandboxes/{vm}", None, F, None),
                ("scope_volumes", "GET", "/sandboxes", None, F, None),
            ])
            case("preset events: reads events, refused sandboxes and volumes", [
                ("scope_events", "GET", "/events/sandboxes?limit=100", None, {200}, None),
                ("scope_events", "GET", f"/events/sandboxes/{vm}", None, {200}, None),
                ("scope_events", "GET", "/sandboxes", None, F, None),
                ("scope_events", "GET", "/volumes", None, F, None),
            ])
            case("preset machines: reaches /machines only", [
                ("scope_machines", "GET", "/machines", None, {200}, None),
                ("scope_machines", "GET", "/sandboxes", None, F, None),
                ("scope_machines", "GET", "/volumes", None, F, None),
            ])
            case("preset telemetry: reaches /telemetry only", [
                ("scope_telemetry", "GET", "/telemetry", None, {404},
                 lambda d: "no telemetry destination" in d["message"] or (_ for _ in ()).throw(AssertionError("not the team's own answer"))),
                ("scope_telemetry", "GET", "/sandboxes", None, F, None),
                ("scope_telemetry", "GET", "/volumes", None, F, None),
            ])
            case("unknown and expired keys are 401", [
                ("unknown", "GET", "/sandboxes", None, {401}, None),
                ("expired", "GET", "/sandboxes", None, {401}, None),
                ("expired", "POST", "/v2/sandboxes", {"templateID": "base"}, {401}, None),
            ])

            # Named presets: the `preset` field of a policy entry.
            case("named preset read-only: lists, refused detail, exec, create and other families", [
                ("preset_read_only", "GET", "/sandboxes", None, {200}, has_vm),
                ("preset_read_only", "GET", "/cluster/nodes", None, {200}, None),
                ("preset_read_only", "GET", f"/sandboxes/{vm}", None, F, None),
                ("preset_read_only", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles"}, F, None),
                ("preset_read_only", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("preset_read_only", "DELETE", f"/sandboxes/{vm}", None, F, None),
                ("preset_read_only", "GET", "/volumes", None, F, None),
                ("preset_read_only", "GET", "/machines", None, F, None),
            ])
            case("named preset developer: runs commands, volumes and templates; refused machines and owner", [
                ("preset_developer", "GET", "/sandboxes", None, {200}, has_vm),
                ("preset_developer", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("preset_developer", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles", "timeout_secs": 30}, {200}, exec_ok),
                ("preset_developer", "GET", "/templates", None, {200}, None),
                ("preset_developer", "POST", "/volumes", {"name": "developer-vol"}, {201}, None),
                ("preset_developer", "GET", "/events/sandboxes?limit=100", None, {200}, None),
                ("preset_developer", "GET", "/machines", None, F, None),
                ("preset_developer", "POST", f"/sandboxes/{vm}/owner", {"principalId": "x"}, F, None),
            ])
            case("named preset machines: machines, inventory and events; refused sandboxes and volumes", [
                ("preset_machines", "GET", "/machines", None, {200}, None),
                ("preset_machines", "GET", "/sandboxes", None, {200}, has_vm),
                ("preset_machines", "GET", "/events/sandboxes?limit=100", None, {200}, None),
                ("preset_machines", "GET", f"/sandboxes/{vm}", None, F, None),
                ("preset_machines", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("preset_machines", "GET", "/volumes", None, F, None),
            ])
            case("named preset admin: global, reaches every family", [
                ("preset_admin", "GET", "/cluster/nodes", None, {200}, None),
                ("preset_admin", "GET", "/sandboxes", None, {200}, has_vm),
                ("preset_admin", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("preset_admin", "GET", "/volumes", None, {200}, None),
                ("preset_admin", "GET", "/machines", None, {200}, None),
            ])
            case("named preset admin with role observer is lowered: lists only", [
                ("preset_admin_observer", "GET", "/sandboxes", None, {200}, has_vm),
                ("preset_admin_observer", "GET", f"/sandboxes/{vm}", None, F, None),
                ("preset_admin_observer", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("preset_admin_observer", "GET", "/volumes", None, F, None),
            ])

            # Roles: operator (default) and observer are the only two there are.
            case("role operator: lists, reads, runs commands in the team's sandbox", [
                ("operator", "GET", "/sandboxes", None, {200}, has_vm),
                ("operator", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("operator", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles", "timeout_secs": 30}, {200}, exec_ok),
                ("operator", "POST", f"/sandboxes/{vm}/owner", {"principalId": "x"}, F, None),
                ("operator", "GET", "/cluster/nodes", None, {200}, None),  # its inventory scope reads it
                ("operator", "GET", "/volumes", None, F, None),
            ])
            case("role observer: lists the inventory, refused detail, exec, create, pause and delete", [
                ("observer", "GET", "/sandboxes", None, {200}, has_vm),
                ("observer", "GET", "/sandboxes/metrics", None, {200}, None),
                ("observer", "GET", f"/sandboxes/{vm}", None, F, None),
                ("observer", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles"}, F, None),
                ("observer", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("observer", "POST", f"/sandboxes/{vm}/pause", None, F, None),
                ("observer", "DELETE", f"/sandboxes/{vm}", None, F, None),
                ("observer", "GET", "/volumes", None, F, None),
            ])
            case("role observer caps even the admin scope (admin scope + observer role)", [
                ("observer_admin", "GET", "/sandboxes", None, {200}, has_vm),
                ("observer_admin", "GET", f"/sandboxes/{vm}", None, F, None),
                ("observer_admin", "POST", "/v2/sandboxes", {"templateID": "base"}, F, None),
                ("observer_admin", "DELETE", f"/sandboxes/{vm}", None, F, None),
            ])
            case("another team's operator key neither lists nor reaches the sandbox", [
                ("other_team", "GET", "/sandboxes", None, {200}, lacks_vm),
                ("other_team", "GET", f"/sandboxes/{vm}", None, F, None),
                ("other_team", "DELETE", f"/sandboxes/{vm}", None, F, None),
            ])
            # Team names for the existing roles, and the team-admin role.
            case("role viewer is the observer role: lists, refused detail, exec and delete", [
                ("viewer", "GET", "/sandboxes", None, {200}, has_vm),
                ("viewer", "GET", f"/sandboxes/{vm}", None, F, None),
                ("viewer", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles"}, F, None),
                ("viewer", "DELETE", f"/sandboxes/{vm}", None, F, None),
                ("viewer", "POST", f"/sandboxes/{vm}/owner", {"principalId": "x"}, F, None),
            ])
            case("role member is the operator role: reads and runs commands, no owner adoption", [
                ("member", "GET", "/sandboxes", None, {200}, has_vm),
                ("member", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("member", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles", "timeout_secs": 30}, {200}, exec_ok),
                ("member", "POST", f"/sandboxes/{vm}/owner", {"principalId": "x"}, F, None),
                ("member", "GET", f"/sandboxes/{blue_vm}", None, F, None),
            ])
            case("role team-admin: an operator in its team, plus owner adoption on its team's sandboxes", [
                ("team_admin", "GET", "/sandboxes", None, {200}, lambda d: (has_vm(d), lacks_blue(d))),
                ("team_admin", "GET", f"/sandboxes/{vm}", None, {200}, None),
                ("team_admin", "POST", f"/sandboxes/{vm}/exec", {"cmd": "echo keyroles", "timeout_secs": 30}, {200}, exec_ok),
                # the owner is k-sandboxes: the same owner is accepted, a different one is not transferred
                ("team_admin", "POST", f"/sandboxes/{vm}/owner", {"principalId": "k-sandboxes"}, {204}, None),
                ("team_admin", "POST", f"/sandboxes/{vm}/owner", {"principalId": "someone-else"}, {409}, None),
                ("team_admin", "GET", "/volumes", None, F, None),
            ])
            case("role team-admin never reaches another team's sandbox", [
                ("team_admin", "GET", f"/sandboxes/{blue_vm}", None, F, None),
                ("team_admin", "POST", f"/sandboxes/{blue_vm}/owner", {"principalId": "red-lead"}, F, None),
                ("team_admin", "POST", f"/sandboxes/{blue_vm}/exec", {"cmd": "echo keyroles"}, F, None),
                ("team_admin", "DELETE", f"/sandboxes/{blue_vm}", None, F, None),
                ("team_admin_blue", "POST", f"/sandboxes/{vm}/owner", {"principalId": "blue-lead"}, F, None),
                ("team_admin_blue", "GET", f"/sandboxes/{vm}", None, F, None),
            ])
            case("role team-admin stays out of administrator-only routes", [
                ("team_admin", "GET", "/cluster/events", None, F, None),
                ("team_admin", "GET", "/cluster/nodes/keyroles-kvm-node/registrations/pending", None, F, None),
                ("team_admin", "POST", f"/cluster/nodes/keyroles-kvm-node/sandboxes/{vm}/registration/reconcile", None, F, None),
                ("team_admin", "POST", "/templates", {"name": "x"}, F, None),
                ("team_admin", "GET", "/machines", None, F, None),
            ])
            case("role administrator (operator + admin scope): global, lists all teams and deletes", [
                ("scope_admin", "GET", "/cluster/nodes", None, {200}, None),
                ("scope_admin", "GET", "/sandboxes", None, {200}, has_vm),
                ("scope_admin", "DELETE", f"/sandboxes/{vm}", None, {204}, None),
                ("scope_admin", "DELETE", f"/sandboxes/{blue_vm}", None, {204}, None),
            ])
            case("deleted sandbox is gone for the team", [
                ("operator", "GET", "/sandboxes", None, {200}, lacks_vm),
            ])

            def extra(name, ok, detail):
                report["cases"].append({"name": name, "success": bool(ok), "requests": [], "detail": detail})
                print(("ok: " if ok else "FAILED: ") + name, flush=True)

            def log_count(needle):
                return (args.output / "control.log").read_text(errors="replace").count(needle)

            def put_policy(rows):
                tmp = directory / "keys.json.new"
                tmp.write_text(json.dumps(rows))
                os.replace(tmp, policies)

            # A file with an unknown preset keeps a control plane from starting.
            bad_file = directory / "bad-keys.json"
            bad_file.write_text(json.dumps([{"sha256": sha("unknown"), "expires_at": expiry, "preset": "superuser"}]))
            try:
                refused = subprocess.run(
                    [str(args.control), "--store", store, "--namespace", namespace + "-bad", "--port", str(port()),
                     "--proxy-port", str(port()), "--api-keys-file", str(bad_file)],
                    env={**env, "HV2_CLUSTER_TOKEN": keys["token"]}, stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
                out = refused.stdout.decode("utf-8", "replace")
                extra("a policy file with an unknown preset: the control plane refuses to start",
                      refused.returncode != 0 and "invalid API key policy JSON" in out,
                      {"exit_code": refused.returncode, "output": out[-300:]})
            except subprocess.TimeoutExpired:
                extra("a policy file with an unknown preset: the control plane refuses to start", False, "still running after 60 s")

            # A team-admin without a team, with the admin scope, or on read-only: refused at start.
            for label, row, needle in [
                ("team-admin without a team_id", {"sha256": sha("unknown"), "expires_at": expiry, "scopes": ["sandboxes"],
                                                  "role": "team-admin", "principal_id": "lead"}, "needs a team_id"),
                ("team-admin with the admin scope", {"sha256": sha("unknown"), "expires_at": expiry, "scopes": ["admin"],
                                                    "role": "team-admin", "principal_id": "lead", "team_id": "red"}, "admin scope"),
                ("team-admin on the read-only preset", {"sha256": sha("unknown"), "expires_at": expiry, "preset": "read-only",
                                                       "role": "team-admin", "principal_id": "lead", "team_id": "red"}, "cannot raise"),
            ]:
                bad_file.write_text(json.dumps([row]))
                try:
                    refused = subprocess.run(
                        [str(args.control), "--store", store, "--namespace", namespace + "-bad", "--port", str(port()),
                         "--proxy-port", str(port()), "--api-keys-file", str(bad_file)],
                        env={**env, "HV2_CLUSTER_TOKEN": keys["token"]}, stdin=subprocess.DEVNULL,
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
                    out = refused.stdout.decode("utf-8", "replace")
                    extra(f"a policy file with a {label}: the control plane refuses to start",
                          refused.returncode != 0 and needle in out,
                          {"exit_code": refused.returncode, "output": out[-300:]})
                except subprocess.TimeoutExpired:
                    extra(f"a policy file with a {label}: the control plane refuses to start", False, "still running after 60 s")

            # And a SIGHUP reload of such a file is rejected, leaving the old keys in force.
            good_policy = list(policy)
            before = log_count("API key reload rejected")
            put_policy([{"sha256": sha("unknown"), "expires_at": expiry, "preset": "superuser"}])
            control_proc.send_signal(__import__("signal").SIGHUP)
            eventually(lambda: log_count("API key reload rejected") > before or (_ for _ in ()).throw(AssertionError("no rejection logged")), 30)
            case("reload with an unknown preset is rejected; the old keys still work", [
                ("preset_developer", "GET", "/sandboxes", None, {200}, None),
                ("preset_read_only", "GET", "/sandboxes", None, {200}, None),
                ("preset_machines", "GET", "/machines", None, {200}, None),
                ("unknown", "GET", "/sandboxes", None, {401}, None),
            ])
            reloaded = log_count("API key policies reloaded")
            put_policy([{"sha256": sha("operator"), "expires_at": expiry, "preset": "read-only", "role": "operator",
                         "principal_id": "role-operator", "team_id": "red"}])
            before = log_count("API key reload rejected")
            control_proc.send_signal(__import__("signal").SIGHUP)
            eventually(lambda: log_count("API key reload rejected") > before or (_ for _ in ()).throw(AssertionError("no rejection logged")), 30)
            case("reload with read-only given role operator is rejected; the old keys still work", [
                ("preset_developer", "GET", "/sandboxes", None, {200}, None),
            ])
            put_policy(good_policy)
            control_proc.send_signal(__import__("signal").SIGHUP)
            eventually(lambda: log_count("API key policies reloaded") > reloaded or (_ for _ in ()).throw(AssertionError("no reload logged")), 30)
            case("a good reload afterwards applies", [
                ("preset_developer", "GET", "/sandboxes", None, {200}, None),
            ])
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
