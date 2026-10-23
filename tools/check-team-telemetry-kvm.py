#!/usr/bin/env python3
"""A team's own telemetry, pushed by the control plane, on one real KVM cluster.

Runs an owned Redis, one hv2-sandboxd node and the hv2-control-plane over
verified TLS and node mTLS, with two teams' API keys and two collectors on
loopback. The control plane is given both collectors with
`--team-otlp-endpoint` and no others. Then, through the control plane's API:

1. A team with no destination has none (404), and one under no prefix the
   operator listed is refused (403). A key without the `telemetry` scope is
   refused (403).
2. Red sets collector A and blue collector B, each with a header. Each
   collector is sent its own team's numbers within the interval, and only
   them: the right resource attribute, the right counts for sandboxes red and
   blue made and paused, and the header the team set.
3. The numbers follow the sandboxes: red resumes a paused sandbox and the
   next push counts it; its usage over the last 24 hours is more than zero.
4. A team reads its destination back, without its header values, and with
   how the last push went; the other team sees only its own.
5. Deleting a destination stops its pushes, and the other team's go on.

With `--otelcol PATH` (the OpenTelemetry Collector's contrib binary), red's
destination is that real collector instead of the Python one: its OTLP/HTTP
receiver demands `Authorization: Bearer red-token` (bearertokenauth), so a
push without the team's header is refused, and its file exporter writes what
it accepted for the same assertions to read.

Writes report.json and each service's log into --output.
"""
import argparse, hashlib, http.client, http.server, json, socket, ssl, subprocess, tempfile, threading, time, uuid
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


class Collector:
    """An OTLP/HTTP collector on loopback that keeps what it is sent."""

    def __init__(self):
        self.pushes = []
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("content-length", 0)))
                try:
                    parsed = json.loads(body)
                except ValueError:
                    parsed = None
                outer.pushes.append((dict(self.headers), parsed))
                self.send_response(200)
                self.send_header("content-length", "0")
                self.end_headers()

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        self.base = f"http://127.0.0.1:{self.port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def last(self):
        return self.pushes[-1] if self.pushes else None


class RealCollector:
    """The OpenTelemetry Collector (contrib), as a process: an OTLP/HTTP receiver that wants a
    bearer token, and a file exporter whose lines are read back as pushes."""

    def __init__(self, binary, directory, token):
        self.port = port()
        self.base = f"http://127.0.0.1:{self.port}"
        self.output = Path(directory) / "collector-out.jsonl"
        self.output.write_text("")
        config = Path(directory) / "collector.yaml"
        config.write_text(f"""extensions:
  bearertokenauth:
    token: {token}
receivers:
  otlp:
    protocols:
      http:
        endpoint: 127.0.0.1:{self.port}
        auth:
          authenticator: bearertokenauth
exporters:
  file:
    path: {self.output}
    flush_interval: 1s
  file/logs:
    path: {Path(directory) / "collector-logs.jsonl"}
    flush_interval: 1s
  file/traces:
    path: {Path(directory) / "collector-traces.jsonl"}
    flush_interval: 1s
service:
  extensions: [bearertokenauth]
  telemetry:
    metrics:
      level: none
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [file]
    logs:
      receivers: [otlp]
      exporters: [file/logs]
    traces:
      receivers: [otlp]
      exporters: [file/traces]
""")
        self.log = (Path(directory) / "collector.log").open("wb")
        self.process = subprocess.Popen([str(binary), "--config", str(config)], stdin=subprocess.DEVNULL,
                                        stdout=self.log, stderr=subprocess.STDOUT)
        self.version = subprocess.run([str(binary), "--version"], capture_output=True, text=True).stdout.strip()
        deadline = time.monotonic() + 30
        while True:
            try:
                socket.create_connection(("127.0.0.1", self.port), timeout=0.2).close()
                break
            except OSError:
                assert self.process.poll() is None, "the collector exited"
                assert time.monotonic() < deadline, "the collector did not start"
                time.sleep(0.2)

    @property
    def pushes(self):
        rows = []
        for line in self.output.read_text().splitlines():
            try:
                rows.append(({}, json.loads(line)))
            except ValueError:
                pass
        return rows

    def last(self):
        rows = self.pushes
        return rows[-1] if rows else None


def gauge(body, name):
    for metric in body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]:
        if metric["name"] == name:
            return metric["gauge"]["dataPoints"][0]["asDouble"]
    raise AssertionError(f"no {name} in {body}")


def resource(body, key):
    for attr in body["resourceMetrics"][0]["resource"]["attributes"]:
        if attr["key"] == key:
            return attr["value"]["stringValue"]
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--otelcol", type=Path, help="the OpenTelemetry Collector contrib binary")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = {name: getattr(args, name) for name in ["daemon", "control", "kernel", "initrd"]}
    inputs["driver"] = Path(__file__)
    report = {"success": False,
              "purpose": "local KVM functional verification of teams' own telemetry; the intervals are the "
                         "check's own waits and not a measurement",
              "input_sha256": {k: digest(v) for k, v in inputs.items()}, "cases": []}
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "plain", "admin", "token"]}
    env = {"PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "RUST_LOG": "warn"}
    processes, handles = [], []
    state = {}
    collector_a, collector_b = None, Collector()

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
            return json.loads(data) if data else None
        finally:
            conn.close()

    def eventually(action, seconds=60, pause=0.5):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(pause)

    def store_dump():
        """The cluster store's sandbox records, as the control plane reads them: a set of
        IDs and one key per record."""
        cli = ["redis-cli", "-p", str(state["redis_port"])]
        run = lambda *argv: subprocess.run(cli + list(argv), capture_output=True, text=True).stdout
        rows = []
        for index in run("--scan", "--pattern", "*:sandboxes").split():
            prefix = index[: -len("sandboxes")]
            for sandbox in sorted(run("SMEMBERS", index).split()):
                try:
                    record = json.loads(run("GET", f"{prefix}sandbox:{sandbox}"))
                except ValueError:
                    rows.append({"sandbox_id": sandbox, "record": "unreadable"})
                    continue
                rows.append({k: record.get(k) for k in ["sandbox_id", "team_id", "paused", "node_id", "cpu_count"]})
        return rows

    def create(key):
        return api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 1800}, key=key, status=201)["sandboxID"]

    with tempfile.TemporaryDirectory(prefix="hm-team-telemetry-", dir="/var/tmp") as temporary:
        directory = Path(temporary)
        collector_a = RealCollector(args.otelcol, directory, "red-token") if args.otelcol else Collector()
        report["real_collector"] = collector_a.version if args.otelcol else None

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
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=team-telemetry-ca",
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
            state["redis_port"] = redis_port
            store = f"redis://127.0.0.1:{redis_port}"
            namespace = "telemetry-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                            "--appendonly", "no", "--dir", str(directory)], env)

            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                    pass
            eventually(redis_ready, 5, 0.2)
            expiry = int(time.time()) + 3600
            policy = []
            for who, team, scopes in [("red", "red", ["sandboxes", "inventory", "templates", "telemetry"]),
                                      ("blue", "blue", ["sandboxes", "inventory", "templates", "telemetry"]),
                                      ("plain", "red", ["sandboxes", "inventory", "templates"])]:
                policy.append({"sha256": hashlib.sha256(keys[who].encode()).hexdigest(), "expires_at": expiry,
                               "scopes": scopes, "principal_id": f"{who}-a", "team_id": team})
            policies = directory / "keys.json"
            policies.write_text(json.dumps(policy))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                           "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "8",
                           "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "telemetry-node",
                           "--advertise-api", f"https://127.0.0.1:{node_port}", "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                           "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "node.pem"),
                           "--mtls-key", str(directory / "node.key")],
                  {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd), "HV2_CLUSTER_TOKEN": keys["token"]})
            start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                              "--proxy-port", str(proxy_port), "--api-keys-file", str(policies),
                              "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                              "--tls-cert", str(directory / "api.pem"), "--tls-key", str(directory / "api.key"),
                              "--mtls-ca", str(directory / "ca.pem"), "--mtls-cert", str(directory / "control.pem"),
                              "--mtls-key", str(directory / "control.key"), "--mtls-node-name", "tcp-node.test",
                              "--team-otlp-endpoint", collector_a.base, "--team-otlp-endpoint", collector_b.base],
                  {**env, "HV2_API_KEY": keys["admin"], "HV2_CLUSTER_TOKEN": keys["token"]})

            def prepared():
                assert all(child.poll() is None for child in processes), "an owned service exited"
                templates = api("GET", "/templates", key="admin")
                assert any(row.get("templateID") == "base" and row.get("snapshot") for row in templates), "template not ready"
            eventually(prepared, 300)

            def refusals():
                refused = {}
                api("GET", "/telemetry", key="red", status=404)
                refused["none set"] = 404
                unlisted = f"http://127.0.0.1:{port()}/v1/metrics"
                said = api("PUT", "/telemetry", {"endpoint": unlisted}, key="red", status=403)
                refused["not listed"] = said["message"]
                for label, body in [("a query", {"endpoint": collector_a.base + "/v1/metrics?key=k"}),
                                    ("credentials", {"endpoint": collector_a.base.replace("//", "//u:p@") + "/v1/metrics"}),
                                    ("a reserved header", {"endpoint": collector_a.base + "/v1/metrics",
                                                           "headers": {"Host": "x"}}),
                                    ("a quick interval", {"endpoint": collector_a.base + "/v1/metrics",
                                                          "intervalSeconds": 1})]:
                    api("PUT", "/telemetry", body, key="red", status=400)
                    refused[label] = 400
                api("GET", "/telemetry", key="plain", status=403)
                api("PUT", "/telemetry", {"endpoint": collector_a.base + "/v1/metrics"}, key="plain", status=403)
                refused["no telemetry scope"] = 403
                return {"refused": refused}
            case("nothing set, an unlisted destination, a bad one and a key without the scope are refused", refusals)

            def sandboxes():
                state["red_running"] = create("red")
                state["red_paused"] = create("red")
                api("POST", f"/sandboxes/{state['red_paused']}/pause", {}, key="red", status=(200, 204))
                state["blue_running"] = create("blue")
                return {"red": [state["red_running"], state["red_paused"]], "blue": [state["blue_running"]]}
            case("red runs one sandbox and pauses another, and blue runs one", sandboxes)

            def set_destinations():
                made = api("PUT", "/telemetry", {"endpoint": collector_a.base + "/v1/metrics", "intervalSeconds": 15,
                                                 "headers": {"Authorization": "Bearer red-token"}},
                           key="red", status=201)
                assert made["headers"] == ["authorization"] and "red-token" not in json.dumps(made), made
                api("PUT", "/telemetry", {"endpoint": collector_b.base + "/v1/metrics", "intervalSeconds": 15,
                                          "headers": {"X-Team-Key": "blue-token"}}, key="blue", status=201)

                def pushed():
                    assert collector_a.last() and collector_b.last(), "no push yet"
                eventually(pushed, 90)
                return {"red_endpoint": made["endpoint"]}
            case("each team sets a destination and is pushed to", set_destinations)

            def own_numbers():
                headers_a, body_a = collector_a.last()
                headers_b, body_b = collector_b.last()
                assert resource(body_a, "hypermachine.team") == "red", body_a
                assert resource(body_b, "hypermachine.team") == "blue", body_b
                if not args.otelcol:  # the real collector refuses a push without the token
                    assert headers_a.get("authorization", headers_a.get("Authorization")) == "Bearer red-token", headers_a
                assert headers_b.get("X-Team-Key", headers_b.get("x-team-key")) == "blue-token", headers_b
                counts = {}
                for label, body, running, paused in [("red", body_a, 1, 1), ("blue", body_b, 1, 0)]:
                    assert gauge(body, "hm_team_sandboxes_running") == running, (label, body)
                    assert gauge(body, "hm_team_sandboxes_paused") == paused, (label, body)
                    assert gauge(body, "hm_team_vcpus_running") == running, (label, body)
                    assert gauge(body, "hm_team_memory_mib_running") == 256 * running, (label, body)
                    counts[label] = {"running": running, "paused": paused}
                # Neither collector was ever sent the other team, or its key.
                assert all("blue" not in json.dumps(b) for _, b in collector_a.pushes)
                assert all("red" not in json.dumps(b) for _, b in collector_b.pushes)
                assert all("blue-token" not in json.dumps(h) for h, _ in collector_a.pushes)
                return counts
            case("each collector is sent its own team's numbers, with its own header, and nothing of the other's", own_numbers)

            def follows():
                # The resume route gives a sandbox 15 seconds to live unless told
                # otherwise, as E2B's does; one resumed without a timeout expires
                # and leaves the count, which is what the first version of this
                # check tripped over, 3 runs in 5.
                api("POST", f"/sandboxes/{state['red_paused']}/resume", {"timeout": 1800}, key="red",
                    status=(200, 201))
                before = len(collector_a.pushes)

                def moved():
                    assert len(collector_a.pushes) > before, "no new push"
                    _, body = collector_a.last()
                    assert gauge(body, "hm_team_sandboxes_running") == 2.0, body
                    assert gauge(body, "hm_team_sandboxes_paused") == 0.0, body
                    return body
                try:
                    body = eventually(moved, 90)
                except AssertionError as error:
                    raise AssertionError(f"{str(error)[:160]} ... the store holds: {store_dump()}") from None
                seconds = gauge(body, "hm_team_running_seconds_24h")
                assert seconds > 0, body
                return {"running": 2, "running_seconds_24h": round(seconds, 1),
                        "vcpu_seconds_24h": round(gauge(body, "hm_team_vcpu_seconds_24h"), 1)}
            case("the numbers follow the sandboxes: a resumed one is counted, and usage is more than zero", follows)

            def read_back():
                red = api("GET", "/telemetry", key="red")
                blue = api("GET", "/telemetry", key="blue")
                assert red["endpoint"].startswith(collector_a.base) and blue["endpoint"].startswith(collector_b.base)
                assert "red-token" not in json.dumps(red) and "blue-token" not in json.dumps(blue)
                assert red["lastPush"]["ok"] is True and red["headers"] == ["authorization"], red
                return {"red_last_push": red["lastPush"]}
            case("a team reads its destination back, without its header, with how the last push went", read_back)

            def deleted():
                api("DELETE", "/telemetry", key="red", status=204)
                api("GET", "/telemetry", key="red", status=404)
                api("DELETE", "/telemetry", key="red", status=404)
                time.sleep(2)
                settled = len(collector_a.pushes)
                blue_before = len(collector_b.pushes)
                time.sleep(40)
                assert len(collector_a.pushes) == settled, "red was pushed to after deleting"
                assert len(collector_b.pushes) > blue_before, "blue's pushes stopped"
                return {"red_pushes_after": 0, "blue_pushes_meanwhile": len(collector_b.pushes) - blue_before}
            case("deleting a destination stops its pushes and not the other team's", deleted)

            report["success"] = True
        finally:
            if args.otelcol:
                stop(collector_a.process)
                collector_a.log.close()
            else:
                collector_a.server.shutdown()
            collector_b.server.shutdown()
            for child in reversed(processes):
                stop(child)
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
