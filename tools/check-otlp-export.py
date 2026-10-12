#!/usr/bin/env python3
"""OTLP export, against a real OpenTelemetry Collector.

Runs the OpenTelemetry Collector (contrib) given by --collector, with an OTLP
HTTP receiver that demands a bearer token and two exporters: a file of what
it received, and a Prometheus endpoint of its own. Then runs hv2-sandboxd
pushing to it, and checks:

1. The collector accepts what the node pushes, and the numbers it then
   serves as Prometheus are the numbers the node serves at /metrics: every
   gauge, every counter, and every bucket, sum and count of every histogram,
   with sandboxes created and deleted so the histograms are not empty.
2. What arrived names the node: service.name and service.instance.id.
3. Without the collector, pushes fail and are counted; with it back, they
   succeed again.
4. OpenTelemetry's own variables configure it as the flags do.
5. A wrong token is refused by the collector and counted as failed; the
   token is in no log of the node's.
6. A protocol other than http/json is refused when the node starts.

Writes report.json, the collector's configuration and the logs into --output.
"""
import argparse, hashlib, json, os, re, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

TOKEN = "otlp-check-token-5f2c91d0"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


SAMPLE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{([^}]*)\})? (\S+)')


def samples(text, keep=lambda name: True):
    """{(name, frozenset(labels)): value} from Prometheus text, without the
    labels a collector adds of its own."""
    found = {}
    for line in text.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        match = SAMPLE.match(line)
        assert match, line
        name, _, labels, value = match.groups()
        if not keep(name):
            continue
        pairs = []
        for pair in re.findall(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"', labels or ""):
            if pair[0] in ("job", "instance") or pair[0].startswith("otel_"):
                continue
            pairs.append(pair)
        found[(name, frozenset(pairs))] = float(value)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "collector", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of OTLP metric export against the OpenTelemetry "
                         "Collector; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd", "collector"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    version = subprocess.run([str(args.collector), "--version"], capture_output=True, text=True).stdout.strip()
    report["collector_version"] = version
    work = Path(tempfile.mkdtemp(prefix="hm-otlp-check-", dir="/var/tmp"))
    receive, scrape = port(), port()
    received = work / "received.jsonl"
    config = f"""extensions:
  bearertokenauth:
    token: "{TOKEN}"
receivers:
  otlp:
    protocols:
      http:
        endpoint: 127.0.0.1:{receive}
        auth:
          authenticator: bearertokenauth
exporters:
  file:
    path: {received}
  prometheus:
    endpoint: 127.0.0.1:{scrape}
service:
  extensions: [bearertokenauth]
  telemetry:
    metrics:
      level: none
  pipelines:
    metrics:
      receivers: [otlp]
      exporters: [file, prometheus]
"""
    (args.output / "collector.yaml").write_text(config.replace(TOKEN, "<token>"))
    (work / "collector.yaml").write_text(config)
    procs = {}
    logs = {}

    def start(name, argv, env=None):
        logs[name] = (args.output / f"{name}.log").open("wb")
        procs[name] = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=logs[name],
                                       stderr=subprocess.STDOUT)
        return procs[name]

    def stop(name):
        proc = procs.pop(name, None)
        if proc is not None and proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        if name in logs:
            logs.pop(name).close()

    def http(method, url, body=None, timeout=180):
        req = urllib.request.Request(url, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def collector():
        start("collector", [str(args.collector), "--config", str(work / "collector.yaml")])
        deadline = time.monotonic() + 60
        while True:
            assert procs["collector"].poll() is None, "the collector exited"
            try:
                with socket.create_connection(("127.0.0.1", receive), timeout=1):
                    return
            except OSError:
                assert time.monotonic() < deadline, "collector readiness"
                time.sleep(0.2)

    def node(name, extra, env_extra=None, template=False, wait=True):
        api = port()
        env = {k: v for k, v in os.environ.items() if not k.startswith("OTEL_")}
        env.update(HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
        env.update(env_extra or {})
        argv = [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
                "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"]
        if not template:
            argv.append("--no-template")
        proc = start(name, argv + extra, env)
        base = f"http://127.0.0.1:{api}"
        if wait:
            deadline = time.monotonic() + 180
            while True:
                assert proc.poll() is None, f"{name} exited"
                try:
                    if http("GET", base + "/metrics", timeout=5)[0] == 200:
                        break
                except OSError:
                    pass
                assert time.monotonic() < deadline, f"{name} readiness"
                time.sleep(0.2)
        return base

    def node_metrics(base):
        status, text = http("GET", base + "/metrics")
        assert status == 200, status
        return text

    def pushes(base):
        found = samples(node_metrics(base), lambda n: n == "hv2_node_otlp_pushes_total")
        return {dict(labels)["result"]: value for (_, labels), value in found.items()}

    def until(what, check, seconds=30):
        deadline = time.monotonic() + seconds
        while True:
            value = check()
            if value:
                return value
            assert time.monotonic() < deadline, what
            time.sleep(0.25)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    try:
        collector()
        main_node = node("node", ["--otlp-endpoint", f"http://127.0.0.1:{receive}/v1/metrics",
                                  "--otlp-header", f"Authorization=Bearer {TOKEN}",
                                  "--otlp-interval", "1"], template=True)

        def same_numbers():
            # Activity, so the counters and the histograms have something in them.
            made = []
            for _ in range(3):
                status, sandbox = http("POST", main_node + "/sandboxes", {"templateID": "base", "timeout": 120})
                assert status in (200, 201), (status, sandbox)
                made.append(sandbox["sandboxID"])
            for sandbox in made:
                status, value = http("DELETE", f"{main_node}/sandboxes/{sandbox}")
                assert status in (200, 204), (status, value)
            ours = lambda n: n.startswith("hv2_node_") and not n.startswith("hv2_node_otlp_")
            # Settled: the node's numbers no longer change, and the collector
            # has had two pushes since.
            expected = until("the node's numbers settle", lambda: (
                lambda a: a if a == samples(node_metrics(main_node), ours) else None)(
                    samples(node_metrics(main_node), ours)))
            sent = pushes(main_node).get("ok", 0)
            until("two more pushes", lambda: pushes(main_node).get("ok", 0) >= sent + 2)
            status, text = http("GET", f"http://127.0.0.1:{scrape}/metrics")
            assert status == 200, status
            (args.output / "collector-prometheus.txt").write_text(text)
            (args.output / "node-metrics.txt").write_text(node_metrics(main_node))
            got = samples(text, ours)
            missing = sorted(f"{n}{sorted(l)}" for n, l in expected if (n, l) not in got)
            assert not missing, f"not in what the collector serves: {missing[:10]}"
            wrong = {f"{n}{sorted(l)}": (v, got[(n, l)]) for (n, l), v in expected.items()
                     if abs(got[(n, l)] - v) > 1e-9 * max(1.0, abs(v))}
            assert not wrong, f"different numbers: {wrong}"
            create = {dict(l).get("le"): v for (n, l), v in expected.items() if n == "hv2_node_create_seconds_bucket"}
            assert create["+Inf"] == 3, create
            kinds = {"gauge": 0, "counter": 0, "histogram": 0}
            for line in node_metrics(main_node).splitlines():
                if line.startswith("# TYPE hv2_node_") and "otlp" not in line:
                    kinds[line.split()[-1]] += 1
            return {"samples_compared": len(expected), "families": kinds,
                    "create_seconds_count": create["+Inf"]}
        case("the collector serves the numbers the node serves, every bucket included", same_numbers)

        def names_the_node():
            lines = [json.loads(line) for line in received.read_text().splitlines() if line.strip()]
            assert lines, "the collector's file is empty"
            last = lines[-1]["resourceMetrics"][0]
            attributes = {a["key"]: a["value"]["stringValue"] for a in last["resource"]["attributes"]}
            assert attributes["service.name"] == "hv2-sandboxd", attributes
            assert attributes["service.instance.id"] == "local", attributes
            metrics = {m["name"]: m for m in last["scopeMetrics"][0]["metrics"]}
            histogram = metrics["hv2_node_create"]["histogram"]["dataPoints"][0]
            assert sum(int(c) for c in histogram["bucketCounts"]) == int(histogram["count"]) == 3, histogram
            assert metrics["hv2_node_create"]["unit"] == "s"
            assert "hv2_node_creates" in metrics and "hv2_node_creates_total" not in metrics
            return {"pushes_in_file": len(lines), "resource": attributes}
        case("what arrived names the node, and a histogram's buckets add up to its count", names_the_node)

        def collector_away():
            stop("collector")
            failed = pushes(main_node).get("failed", 0)
            until("a failed push is counted", lambda: pushes(main_node).get("failed", 0) >= failed + 2)
            collector()
            sent = pushes(main_node).get("ok", 0)
            until("pushes succeed again", lambda: pushes(main_node).get("ok", 0) >= sent + 2)
            log = (args.output / "node.log").read_text(errors="replace")
            assert "OTLP: push to" in log and "OTLP: pushing again" in log, "the node did not say so"
            # Said once each, not at every failure.
            assert log.count("OTLP: push to") == 1, log.count("OTLP: push to")
            return pushes(main_node)
        case("without the collector pushes fail and are counted, and resume when it is back", collector_away)
        stop("node")

        def from_the_environment():
            env_node = node("node-env", [], {
                "OTEL_EXPORTER_OTLP_ENDPOINT": f"http://127.0.0.1:{receive}",
                "OTEL_EXPORTER_OTLP_HEADERS": f"Authorization=Bearer%20{TOKEN}",
                "OTEL_SERVICE_NAME": "fleet-check",
                "OTEL_METRIC_EXPORT_INTERVAL": "1000",
                "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
            })
            until("pushes from the environment's configuration", lambda: pushes(env_node).get("ok", 0) >= 2)
            def from_fleet():
                for line in received.read_text().splitlines():
                    resource = json.loads(line)["resourceMetrics"][0]["resource"]["attributes"]
                    if {"key": "service.name", "value": {"stringValue": "fleet-check"}} in resource:
                        return True
                return False
            until("the collector has the environment's service name", from_fleet)
            stop("node-env")
            return None
        case("OpenTelemetry's own variables configure it as the flags do", from_the_environment)

        def wrong_token():
            bad = node("node-bad", ["--otlp-endpoint", f"http://127.0.0.1:{receive}/v1/metrics",
                                    "--otlp-header", "Authorization=Bearer not-the-token",
                                    "--otlp-interval", "1"])
            until("refused pushes are counted", lambda: pushes(bad).get("failed", 0) >= 2)
            assert pushes(bad).get("ok", 0) == 0
            stop("node-bad")
            log = (args.output / "node-bad.log").read_text(errors="replace")
            assert "401" in log, log[-2000:]
            for name in ["node", "node-env", "node-bad"]:
                text = (args.output / f"{name}.log").read_text(errors="replace")
                assert TOKEN not in text and "not-the-token" not in text, f"a token is in {name}.log"
            return {"said": [l for l in log.splitlines() if "OTLP: push to" in l][0][-200:]}
        case("a wrong token is refused and counted, and no token is in any log", wrong_token)

        def wrong_protocol():
            refused = node("node-grpc", [], {"OTEL_EXPORTER_OTLP_ENDPOINT": f"http://127.0.0.1:{receive}",
                                             "OTEL_EXPORTER_OTLP_PROTOCOL": "grpc"}, wait=False)
            assert refused.startswith("http")
            code = procs["node-grpc"].wait(timeout=30)
            stop("node-grpc")
            said = (args.output / "node-grpc.log").read_text(errors="replace").strip()
            assert code != 0 and "http/json" in said, (code, said)
            return {"exit": code, "said": said[-200:]}
        case("a protocol other than http/json is refused when the node starts", wrong_protocol)

        report["success"] = True
    finally:
        for name in list(procs):
            stop(name)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
