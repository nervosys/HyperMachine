#!/usr/bin/env python3
"""Verify custom-domain HTTPS through an owned Redis, control plane and KVM node.

Requires redis-server, openssl and a guest image containing BusyBox httpd.
This is a functional verification, not a performance benchmark.
"""
import argparse
import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import urllib.parse


def free_port():
    with socket.socket() as stream:
        stream.bind(("127.0.0.1", 0))
        return stream.getsockname()[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


class LocalHTTPS(http.client.HTTPSConnection):
    """Connect locally while verifying the certificate for the bound DNS name."""
    def connect(self):
        stream = socket.create_connection(("127.0.0.1", self.port), self.timeout)
        try:
            self.sock = self._context.wrap_socket(stream, server_hostname=self.host)
        except BaseException:
            stream.close()
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("control-plane", "daemon", "kernel", "initrd", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--dns-ownership", action="store_true")
    args = parser.parse_args()
    if args.output.exists(): raise ValueError("preserve earlier evidence")
    paths = {name: getattr(args, name).resolve() for name in ("control_plane", "daemon", "kernel", "initrd")}
    paths["verifier"] = Path(__file__).resolve()
    report = {"functional_only": True, "success": False, "checks": [],
              "dns_ownership_enabled": args.dns_ownership,
              "artifact_sha256": {name: digest(path) for name, path in paths.items()},
              "cleanup_errors": []}
    key, token = secrets.token_hex(24), secrets.token_hex(24)
    processes, logs, sandboxes = [], [], set()
    ports = set()
    while len(ports) < (6 if args.dns_ownership else 5):
        ports.add(free_port())
    redis_port, node_port, node_proxy, control_port, proxy_port = sorted(ports)[:5]
    dns_port = sorted(ports)[5] if args.dns_ownership else None
    dns_records = {}
    dns_server = dns_thread = None
    base = f"http://127.0.0.1:{control_port}"
    node_base = f"http://127.0.0.1:{node_port}"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def api(method, path, body=None, node=False):
        data = None if body is None else json.dumps(body).encode()
        headers = {"Content-Type": "application/json"}
        headers["x-hv2-cluster-token" if node else "x-api-key"] = token if node else key
        request = urllib.request.Request((node_base if node else base) + path, data=data, method=method, headers=headers)
        try:
            response = opener.open(request, timeout=40)
        except urllib.error.HTTPError as error:
            return error.code, error.read(4096).decode(errors="replace")
        with response:
            data = response.read(1024 * 1024 + 1)
            if len(data) > 1024 * 1024:
                raise RuntimeError("API response exceeded bound")
            return response.status, json.loads(data) if data else None

    def require(method, path, body=None, status=200):
        if args.dns_ownership and method == "PUT" and "/domains/" in path:
            code, challenge = api("GET", path + "/challenge")
            if code != 200: raise RuntimeError("ownership challenge request failed")
            dns_records[challenge["record_name"]] = challenge["record_value"]
            body = dict(body, challenge_expires_at=challenge["expires_at"])
        code, value = api(method, path, body)
        if code != status:
            raise RuntimeError(f"{method} {path}: expected {status}, got {code}: {value}")
        return value

    def execute(sandbox, command):
        value = require("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 10})
        if value.get("exit_code") != 0 or value.get("timed_out"):
            raise RuntimeError(f"guest command failed: {value}")
        return value["stdout"]

    with tempfile.TemporaryDirectory(prefix="hm-custom-domains-", dir="/var/tmp") as scratch:
        scratch = Path(scratch)
        namespace = "domains-" + secrets.token_hex(8)
        store = f"redis://127.0.0.1:{redis_port}"
        env = dict(os.environ, HV2_API_KEY=key, HV2_CLUSTER_TOKEN=token,
                   HV2_KERNEL=str(paths["kernel"]), HV2_INITRD=str(paths["initrd"]), RUST_LOG="warn")

        def launch(name, command):
            log = (scratch / f"{name}-{len(processes)}.log").open("wb")
            logs.append(log)
            process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
            processes.append(process)
            return process

        def ready(process, probe):
            deadline = time.monotonic() + 40
            while True:
                if process.poll() is not None:
                    raise RuntimeError("owned service exited before readiness")
                try:
                    if probe():
                        return
                except OSError:
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError("service readiness timed out")
                time.sleep(.05)

        cert, private_key = scratch / "cert.pem", scratch / "key.pem"
        control_args = [str(paths["control_plane"]), "--store", store, "--namespace", namespace,
                        "--port", str(control_port), "--proxy-port", str(proxy_port),
                        "--tls-cert", str(cert), "--tls-key", str(private_key)]
        try:
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                            "-subj", "/CN=app.example.test", "-addext", "subjectAltName=DNS:app.example.test,DNS:localhost",
                            "-keyout", str(private_key), "-out", str(cert)], check=True, timeout=20,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if args.dns_ownership:
                class DNSHandler(http.server.BaseHTTPRequestHandler):
                    def log_message(self, *_): pass
                    def do_GET(self):
                        query=urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
                        name=query.get("name",[""])[0]
                        value=dns_records.get(name)
                        answer=[] if value is None else [{"name":name,"type":16,"data":'"'+value+'"'}]
                        raw=json.dumps({"Status":0,"TC":False,"Question":[{"name":name,"type":16}],"Answer":answer}).encode()
                        self.send_response(200);self.send_header("Content-Type","application/dns-json")
                        self.send_header("Content-Length",str(len(raw)));self.end_headers();self.wfile.write(raw)
                dns_server=http.server.ThreadingHTTPServer(("127.0.0.1",dns_port),DNSHandler)
                dns_server.daemon_threads=True
                dns_tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                dns_tls.load_cert_chain(str(cert),str(private_key))
                dns_server.socket=dns_tls.wrap_socket(dns_server.socket,server_side=True)
                dns_thread=threading.Thread(target=dns_server.serve_forever,daemon=True);dns_thread.start()
                policy=scratch/"dns-policy.json"
                policy.write_text(json.dumps({"namespace":namespace,"secret_hex":secrets.token_hex(32),
                    "resolver_url":f"https://localhost:{dns_port}/dns-query","resolver_ca_file":str(cert)}))
                control_args += ["--domain-verification-file",str(policy)]
            redis = launch("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port),
                                     "--save", "", "--appendonly", "no", "--dir", str(scratch)])
            def redis_ready():
                with socket.create_connection(("127.0.0.1", redis_port), timeout=1) as stream:
                    stream.sendall(b"*1\r\n$4\r\nPING\r\n")
                    return stream.recv(64) == b"+PONG\r\n"
            ready(redis, redis_ready)
            node = launch("node", [str(paths["daemon"]), "--port", str(node_port), "--proxy-port", str(node_proxy),
                                   "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "4",
                                   "--volume-dir", str(scratch / "volumes"), "--snapshot-store", str(scratch / "snapshots"),
                                   "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "domain-node",
                                   "--advertise-api", node_base, "--advertise-proxy", f"127.0.0.1:{node_proxy}"])
            ready(node, lambda: api("GET", "/templates", node=True)[0] == 200)
            control = launch("control", control_args)
            ready(control, lambda: api("GET", "/templates")[0] == 200)
            templates = require("GET", "/templates")
            report["template"] = next(item for item in templates if "base" in item.get("aliases", []))
            if report["template"].get("snapshot") is not True:
                raise RuntimeError("verification requires snapshot-backed base template")
            descriptor = require("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600, "autoResume": {"enabled": True}}, status=201)
            sandbox = descriptor["sandboxID"]
            sandboxes.add(sandbox)
            binding = f"/sandboxes/{sandbox}/domains/app.example.test"
            if args.dns_ownership:
                if api("PUT",binding,{"port":8080})[0]!=428: raise RuntimeError("missing DNS proof was accepted")
                challenge=require("GET",binding+"/challenge")
                if api("PUT",binding,{"port":8080,"challenge_expires_at":challenge["expires_at"]})[0]!=403:
                    raise RuntimeError("absent TXT proof was accepted")
                if api("PUT",binding,{"port":8080,"challenge_expires_at":0})[0]!=403:
                    raise RuntimeError("expired proof was accepted")
                if require("GET",f"/sandboxes/{sandbox}/domains")!=[]:
                    raise RuntimeError("failed DNS proofs mutated domain inventory")
                report["checks"].append("missing, absent and expired DNS proofs are refused without binding a real guest")
            marker = "domain-guest-" + secrets.token_hex(8)
            execute(sandbox, f"mkdir -p /root/domain-www; printf '%s' '{marker}' > /root/domain-www/index.html; busybox httpd -p 8080 -h /root/domain-www")
            require("PUT", binding, {"port": 8080})
            if args.dns_ownership:
                report["checks"].append("trusted owned HTTPS resolver proves sandbox-bound TXT before routing")
            context = ssl.create_default_context(cafile=str(cert))
            def guest():
                connection = LocalHTTPS("app.example.test", proxy_port, timeout=20, context=context)
                try:
                    connection.request("GET", "/", headers={"Host": f"app.example.test:{proxy_port}"})
                    response = connection.getresponse()
                    return response.status, response.read(4096).decode()
                finally:
                    connection.close()
            if guest() != (200, marker):
                raise RuntimeError("TLS alias did not reach the actual guest's web server")
            report["checks"].append("certificate-verified HTTPS alias reaches guest web server through both proxies")
            marker += "-port3000"
            execute(sandbox, f"mkdir -p /root/domain-alt; printf '%s' '{marker}' > /root/domain-alt/index.html; busybox httpd -p 3000 -h /root/domain-alt")
            require("PUT", binding, {"port": 3000})
            if guest() != (200, marker):
                raise RuntimeError("port update did not reach the second guest web server")
            report["checks"].append("owner port update reaches a distinct guest web server without proxy restart")
            if args.dns_ownership:
                dns_records.clear()
                challenge=require("GET",binding+"/challenge")
                if api("PUT",binding,{"port":8080,"challenge_expires_at":challenge["expires_at"]})[0]!=403:
                    raise RuntimeError("failed DNS revalidation changed the port")
                if guest()!=(200,marker): raise RuntimeError("failed proof changed the existing guest route")
                report["checks"].append("failed TXT revalidation preserves existing guest port and HTTPS response")
            stop(control)
            control = launch("control-restarted", control_args)
            ready(control, lambda: api("GET", "/templates")[0] == 200)
            if guest() != (200, marker):
                raise RuntimeError("binding lost across control-plane restart")
            report["checks"].append("Redis binding survives control-plane restart")
            require("POST", f"/sandboxes/{sandbox}/pause", {}, status=204)
            if guest() != (200, marker):
                raise RuntimeError("alias request did not resume paused guest web server")
            report["checks"].append("alias request resumes paused guest with retained web-server state")
            require("DELETE", binding, status=204)
            if guest()[0] != 400:
                raise RuntimeError("unbound alias still routes")
            require("PUT", binding, {"port": 3000})
            require("DELETE", f"/sandboxes/{sandbox}", status=204)
            sandboxes.remove(sandbox)
            if guest()[0] != 400:
                raise RuntimeError("deleted sandbox alias still routes")
            report["checks"].append("unbind and sandbox deletion stop alias routing")
            replacement = require("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600}, status=201)["sandboxID"]
            sandboxes.add(replacement)
            if args.dns_ownership:
                challenge=require("GET",f"/sandboxes/{replacement}/domains/app.example.test/challenge")
                if api("PUT",f"/sandboxes/{replacement}/domains/app.example.test",
                       {"port":8080,"challenge_expires_at":challenge["expires_at"]})[0]!=403:
                    raise RuntimeError("the prior sandbox's TXT proof transferred to its replacement")
                report["checks"].append("prior sandbox TXT cannot bind a replacement sandbox")
            require("PUT", f"/sandboxes/{replacement}/domains/app.example.test", {"port": 8080})
            require("DELETE", f"/sandboxes/{replacement}", status=204)
            sandboxes.remove(replacement)
            report["checks"].append("deleted sandbox releases hostname ownership for a new sandbox")
            report["success"] = True
        except Exception as error:
            report["error"] = str(error)
        finally:
            for sandbox in sandboxes:
                try:
                    code, _ = api("DELETE", f"/sandboxes/{sandbox}", node=True)
                    if code not in (204, 404):
                        raise RuntimeError(f"cleanup delete returned {code}")
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            if "node" in locals() and node.poll() is None:
                try:
                    code, records = api("GET", "/sandboxes", node=True)
                    if code != 200 or records != []:
                        raise RuntimeError("isolated node not empty after cleanup")
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            for process in reversed(processes):
                try:
                    stop(process)
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            report["owned_processes_reaped"] = all(process.poll() is not None for process in processes)
            if dns_server:
                dns_server.shutdown();dns_server.server_close();dns_thread.join(timeout=5)
                report["owned_dns_server_stopped"] = not dns_thread.is_alive()
                if dns_thread.is_alive(): report["cleanup_errors"].append("owned DNS server did not stop")
            for log in logs:
                log.close()
            report["log_tails"] = {path.name: path.read_text(errors="replace")[-4000:] for path in scratch.glob("*.log")}
    report["artifacts_unchanged"] = all(digest(path) == report["artifact_sha256"][name] for name, path in paths.items())
    report["success"] = report["success"] and report["artifacts_unchanged"] and report["owned_processes_reaped"] and not report["cleanup_errors"]
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({name: report[name] for name in ("success", "checks", "cleanup_errors")}, indent=2))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
