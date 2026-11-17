#!/usr/bin/env python3
"""Template builds by teamless keys with a principal (O14), through a real control plane and node on KVM.

Starts an owned Redis, one hv2-sandboxd node and one hv2-control-plane (plain HTTP on loopback,
shared cluster token) with an --api-keys-file of three keys generated at run time: alice and bob
(principal, no team, scope `sandboxes`+`inventory`) and an administrator. Drives E2B's build API
(POST /v3/templates, POST /v2/templates/{id}/builds/{buildID}, GET .../status) through the
control plane.

Cases: 1 alice's build reaches `ready` on the node under a scoped name; 2 alice's sandbox from
`app` reads /hello; 3 bob gets 404 on alice's template (plain and canonical name) for list,
build status, sandbox create, DELETE, and his own `app` is a different scoped name; 4 admin lists
alice's template; 5 alice deletes hers.
"""
import argparse, hashlib, json, os, signal, socket, subprocess, tempfile, time, uuid
import urllib.error, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "kernel", "initrd", "output", "redis"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--redis-libs", type=Path, default=None)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {k: getattr(args, k) for k in ["daemon", "control", "kernel", "initrd", "redis"]}
    inputs["driver"] = Path(__file__)
    report = {"success": False,
              "purpose": "functional verification of principal-scoped template builds (O14) on KVM; no timing claim",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": [], "calls": []}
    keys = {n: uuid.uuid4().hex for n in ["alice", "bob", "admin", "token", "legacy"]}
    processes, handles = [], []
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "info"}
    state = {}

    def start(name, argv, environment):
        handle = (args.output / (name + ".log")).open("wb")
        handles.append(handle)
        child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle,
                                 stderr=subprocess.STDOUT)
        processes.append((name, child))
        return child

    def call(who, method, path, body=None, timeout=120, note=True):
        req = urllib.request.Request(state["base"] + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                     headers={"x-api-key": keys[who], "content-type": "application/json"})
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
        try:
            value = json.loads(raw) if raw else None
        except ValueError:
            value = raw.decode(errors="replace")
        if note:
            report["calls"].append({"as": who, "method": method, "path": path, "status": response.status,
                                    "response": value if len(json.dumps(value)) < 600 else "(long)"})
        return response.status, value

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail is not None:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(who, sandbox, command):
        status, value = call(who, "POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 30})
        assert status == 200 and value["exit_code"] == 0, (status, value)
        return value["stdout"]

    def build(who, name, steps):
        status, made = call(who, "POST", "/v3/templates", {"name": name})
        assert status in (200, 201, 202), (status, made)
        tid, bid = made["templateID"], made["buildID"]
        status, value = call(who, "POST", f"/v2/templates/{tid}/builds/{bid}", {"fromTemplate": "base", "steps": steps})
        assert status == 202, (status, value)
        deadline = time.monotonic() + 600
        while True:
            status, value = call(who, "GET", f"/templates/{tid}/builds/{bid}/status", note=False)
            assert status == 200, (status, value)
            if value["status"] in ("ready", "error"):
                report["calls"].append({"as": who, "method": "GET", "path": f"/templates/{tid}/builds/{bid}/status",
                                        "status": status, "response": {"status": value["status"], "reason": value.get("reason")}})
                return made, value
            assert time.monotonic() < deadline, ("build", value["status"])
            time.sleep(0.5)

    with tempfile.TemporaryDirectory(prefix="hm-principal-templates-", dir="/var/tmp") as temporary:
        directory = Path(temporary)
        try:
            ports = set()
            while len(ports) < 5:
                ports.add(port())
            redis_port, node_port, node_proxy, api_port, proxy_port = list(ports)
            state["base"] = f"http://127.0.0.1:{api_port}"
            store = f"redis://127.0.0.1:{redis_port}"
            namespace = "principal-templates-" + uuid.uuid4().hex
            redis_env = dict(env)
            if args.redis_libs:
                redis_env["LD_LIBRARY_PATH"] = str(args.redis_libs)
            start("redis", [str(args.redis), "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], redis_env)
            deadline = time.monotonic() + 10
            while True:
                try:
                    socket.create_connection(("127.0.0.1", redis_port), timeout=0.2).close()
                    break
                except OSError:
                    assert time.monotonic() < deadline, "redis"
                    time.sleep(0.1)
            now = int(time.time())
            sha = lambda k: hashlib.sha256(keys[k].encode()).hexdigest()
            policy = [{"sha256": sha(k), "expires_at": now + 3000, "scopes": ["sandboxes", "inventory", "templates"],
                       "principal_id": k} for k in ["alice", "bob"]]
            policy.append({"sha256": sha("admin"), "expires_at": now + 3000, "scopes": ["admin", "sandboxes", "inventory", "templates"],
                           "role": "operator", "principal_id": "admin"})
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            report["policy"] = [{k: v for k, v in r.items() if k != "sha256"} for r in policy]
            (directory / "kit").mkdir()
            start("node", [str(args.daemon), "--guest-kit", str(directory / "kit"), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6",
                           "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots"),
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "ptpl-kvm-node",
                           "--advertise-api", f"http://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}"],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": keys["token"]})
            start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                              "--proxy-port", str(proxy_port), "--api-keys-file", str(policies)],
                  {**env, "HV2_API_KEY": keys["legacy"], "HV2_CLUSTER_TOKEN": keys["token"]})
            deadline = time.monotonic() + 300
            while True:
                assert all(c.poll() is None for _, c in processes), "an owned service exited"
                try:
                    status, templates = call("admin", "GET", "/templates", note=False)
                    if status == 200 and any(r.get("templateID") == "base" and r.get("snapshot") for r in templates):
                        socket.create_connection(("127.0.0.1", node_port), timeout=0.2).close()
                        break
                except OSError:
                    pass
                assert time.monotonic() < deadline, "template not ready"
                time.sleep(0.3)

            steps = [{"type": "RUN", "args": ["echo hi > /hello"]}]

            def alice_build():
                made, value = build("alice", "app", steps)
                assert value["status"] == "ready", value
                state["alice_made"] = made
                state["alice_id"] = made["templateID"]
                # What the node holds: ask the node directly with the cluster token.
                req = urllib.request.Request(f"http://127.0.0.1:{node_port}/templates",
                                             headers={"authorization": "Bearer " + keys["token"], "x-api-key": keys["token"]})
                try:
                    node_names = json.loads(urllib.request.urlopen(req, timeout=10).read())
                except Exception as error:  # recorded, not fatal: the admin listing below is the primary evidence
                    node_names = f"node listing unavailable: {error}"
                state["node_names"] = node_names
                return {"build_response": made, "final_status": value["status"], "node_templates": node_names}
            case("1. alice's `app` build reaches ready on the node", alice_build)

            def alice_sandbox():
                status, made = call("alice", "POST", "/sandboxes", {"templateID": "app", "timeout": 600})
                assert status in (200, 201), (status, made)
                state["alice_sb"] = made["sandboxID"]
                out = sh("alice", made["sandboxID"], "cat /hello")
                assert out == "hi\n", repr(out)
                return {"create_status": status, "cat_hello": out}
            case("2. alice's sandbox from `app` prints hi from /hello", alice_sandbox)

            def canonical():
                status, rows = call("admin", "GET", "/templates")
                assert status == 200
                scoped = [r for r in rows if "scope." in json.dumps(r)]
                assert scoped, rows
                state["admin_rows"] = rows
                names = set()
                for r in scoped:
                    for v in [r.get("templateID")] + list(r.get("aliases") or []) + list(r.get("names") or []):
                        if isinstance(v, str) and v.startswith("scope."):
                            names.add(v)
                state["canon"] = sorted(names)
                return sorted(names)
            canon_names = case("3a. admin finds the canonical scope.<hash>--app name", canonical)

            def bob():
                alice_id = state["alice_id"]
                canon = [n for n in state["canon"] if n.endswith("--app")]
                assert canon, state["canon"]
                canon = canon[0]
                state["canon_name"] = canon
                out = {}
                status, rows = call("bob", "GET", "/templates")
                assert status == 200
                text = json.dumps(rows)
                assert "scope." not in text and alice_id not in text and canon not in text, rows
                out["bob_list_has_no_alice_template"] = True
                bid = state["alice_made"]["buildID"]
                for label, ident in [("plain", "app"), ("canonical", canon), ("template_id", alice_id)]:
                    status, value = call("bob", "GET", f"/templates/{ident}/builds/{bid}/status")
                    out[f"status_{label}"] = status
                    assert status == 404, (label, status, value)
                    status, value = call("bob", "POST", "/sandboxes", {"templateID": ident, "timeout": 60})
                    out[f"create_{label}"] = status
                    # `app` plain for bob is not a template at all for him (alice's is hidden) -> 404 too
                    assert status == 404, (label, status, value)
                    status, value = call("bob", "DELETE", f"/templates/{ident}")
                    out[f"delete_{label}"] = status
                    assert status == 404, (label, status, value)
                return out
            case("3b. bob gets 404 for alice's template: list, status, create, delete (plain and canonical)", bob)

            def bob_own():
                made, value = build("bob", "app", steps)
                assert value["status"] == "ready", value
                state["bob_made"] = made
                status, rows = call("admin", "GET", "/templates")
                names = sorted({v for r in rows for v in [r.get("templateID")] + list(r.get("aliases") or [])
                                if isinstance(v, str) and v.startswith("scope.") and v.endswith("--app")})
                assert len(names) == 2 and names[0] != names[1], names
                assert state["canon_name"] in names
                state["bob_canon"] = [n for n in names if n != state["canon_name"]][0]
                # alice cannot see bob's, and bob sees his own
                _, mine = call("bob", "GET", "/templates")
                assert state["bob_canon"] in json.dumps(mine) or "app" in json.dumps(mine), mine
                assert state["canon_name"] not in json.dumps(mine)
                status, made_sb = call("bob", "POST", "/sandboxes", {"templateID": "app", "timeout": 600})
                assert status in (200, 201), (status, made_sb)
                out = sh("bob", made_sb["sandboxID"], "cat /hello")
                assert out == "hi\n"
                call("bob", "DELETE", f"/sandboxes/{made_sb['sandboxID']}")
                return {"alice_scoped": state["canon_name"], "bob_scoped": state["bob_canon"], "bob_build": made,
                        "bob_sandbox_from_own_app_cat_hello": out}
            case("3c. bob's own `app` build gets a different scoped name and works for bob", bob_own)

            def admin_sees():
                status, rows = call("admin", "GET", "/templates")
                assert status == 200
                assert state["canon_name"] in json.dumps(rows), rows
                return {"admin_list_includes": state["canon_name"], "status": status}
            case("4. an admin key sees alice's template in GET /templates", admin_sees)

            def alice_delete():
                call("alice", "DELETE", f"/sandboxes/{state['alice_sb']}")
                status, value = call("alice", "DELETE", "/templates/app")
                assert status in (200, 204), (status, value)
                status, rows = call("alice", "GET", "/templates")
                assert state["canon_name"] not in json.dumps(rows)
                _, adm = call("admin", "GET", "/templates")
                assert state["canon_name"] not in json.dumps(adm), adm
                assert state["bob_canon"] in json.dumps(adm), "bob's must survive"
                return {"delete_status": status, "gone_from_admin_list": True, "bob_template_survives": True}
            case("5. alice can DELETE her template (bob's stays)", alice_delete)

            time.sleep(1)
            lines = (args.output / "node.log").read_text(errors="replace").splitlines()
            counts = {"kvm_lines": sum("platform: Kvm" in l for l in lines),
                      "tcg_lines": sum("platform: Tcg" in l for l in lines)}
            report["log_counts"] = counts
            assert counts["tcg_lines"] == 0 and counts["kvm_lines"] >= 1, counts
            report["success"] = True
        finally:
            for name, child in reversed(processes):
                if child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                    try:
                        child.wait(timeout=30)
                    except subprocess.TimeoutExpired:
                        child.kill()
            for h in handles:
                h.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json", "success:", report["success"])


if __name__ == "__main__":
    main()
