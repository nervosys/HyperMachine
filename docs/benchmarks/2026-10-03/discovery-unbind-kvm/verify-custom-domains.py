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
import shlex
import socket
import ssl
import signal
import subprocess
import sys
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
    parser.add_argument("--tls-bundle", action="store_true")
    parser.add_argument("--deploy-hook", type=Path)
    parser.add_argument("--certificate-lineage", type=Path)
    parser.add_argument("--certificate-ca", type=Path)
    parser.add_argument("--renew-command-file", type=Path)
    parser.add_argument("--renewal-worker", type=Path)
    parser.add_argument("--acme-ca", type=Path)
    parser.add_argument("--initial-issuance-webroot", type=Path)
    args = parser.parse_args()
    if args.initial_issuance_webroot and not args.renewal_worker:
        parser.error("initial issuance requires the configured certificate worker")
    if args.deploy_hook and not args.tls_bundle:
        parser.error("--deploy-hook requires --tls-bundle")
    acme_options = (args.certificate_lineage, args.certificate_ca, args.renew_command_file)
    if any(acme_options) and (not all(acme_options) or not args.deploy_hook):
        parser.error("ACME certificate options must all be supplied with --deploy-hook")
    if bool(args.renewal_worker) != bool(args.acme_ca) or (args.renewal_worker and not args.certificate_lineage):
        parser.error("--renewal-worker and --acme-ca require the ACME certificate options")
    if args.output.exists(): raise ValueError("preserve earlier evidence")
    paths = {name: getattr(args, name).resolve() for name in ("control_plane", "daemon", "kernel", "initrd")}
    paths["verifier"] = Path(__file__).resolve()
    if args.deploy_hook:
        paths["deploy_hook"] = args.deploy_hook.resolve()
    if args.certificate_lineage:
        args.certificate_lineage = args.certificate_lineage.resolve()
        paths["certificate_ca"] = args.certificate_ca.resolve()
        paths["renew_command_file"] = args.renew_command_file.resolve()
    if args.renewal_worker:
        paths["renewal_worker"] = args.renewal_worker.resolve()
        if args.initial_issuance_webroot:
            paths["discovery_planner"] = paths["renewal_worker"].with_name("domain-certificate-jobs.py")
        paths["acme_ca"] = args.acme_ca.resolve()
        if paths["renewal_worker"].with_name("deploy-tls-certificate.py") != paths["deploy_hook"]:
            parser.error("renewal worker and deploy hook must be installed together")
    report = {"functional_only": True, "success": False, "checks": [],
              "dns_ownership_enabled": args.dns_ownership,
              "tls_bundle_enabled": args.tls_bundle,
              "acme_renewal_enabled": bool(args.certificate_lineage),
              "renewal_worker_enabled": bool(args.renewal_worker),
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
        env = {name:value for name,value in env.items() if name.upper() not in {"HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","NO_PROXY"}}

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
        bundle_file = scratch / "tls-bundle.json"
        control_args = [str(paths["control_plane"]), "--store", store, "--namespace", namespace,
                        "--port", str(control_port), "--proxy-port", str(proxy_port),
                        "--tls-cert", str(cert), "--tls-key", str(private_key)]
        try:
            api_cert = scratch / "api-cert.pem"
            if args.initial_issuance_webroot:
                api_key_file = scratch / "api-key.pem"
                subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                    "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost",
                    "-keyout", str(api_key_file), "-out", str(api_cert)], check=True, timeout=20,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                control_args += ["--api-tls-cert", str(api_cert), "--api-tls-key", str(api_key_file)]
                base = f"https://localhost:{control_port}"
                opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                    urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=str(api_cert))))
            if args.certificate_lineage:
                cert.write_bytes((args.certificate_lineage / "fullchain.pem").read_bytes())
                private_key.write_bytes((args.certificate_lineage / "privkey.pem").read_bytes())
                private_key.chmod(0o600)
            else:
                subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                                "-subj", "/CN=app.example.test", "-addext", "subjectAltName=DNS:app.example.test,DNS:localhost",
                                "-addext", "basicConstraints=critical,CA:FALSE", "-addext", "extendedKeyUsage=serverAuth",
                                "-keyout", str(private_key), "-out", str(cert)], check=True, timeout=20,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if args.tls_bundle:
                bundle_doc={"certificates":[{"names":["app.example.test"],"cert_path":str(cert),"key_path":str(private_key)}]}
                bundle_file.write_text(json.dumps(bundle_doc))
                tls_index = control_args.index("--tls-cert")
                del control_args[tls_index:tls_index + 4]
                control_args += ["--tls-bundle-file", str(bundle_file)]
            if args.dns_ownership:
                dns_ca,dns_ca_key=scratch/"dns-ca.pem",scratch/"dns-ca.key"
                dns_cert,dns_key=scratch/"dns-cert.pem",scratch/"dns-key.pem"
                dns_csr,dns_ext=scratch/"dns.csr",scratch/"dns.ext"
                dns_ext.write_text("subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\nkeyUsage=digitalSignature,keyEncipherment\n")
                for command in [
                    ["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1","-subj","/CN=hm-fixture-ca",
                     "-addext","basicConstraints=critical,CA:TRUE","-keyout",str(dns_ca_key),"-out",str(dns_ca)],
                    ["openssl","req","-new","-newkey","rsa:2048","-nodes","-subj","/CN=localhost","-keyout",str(dns_key),"-out",str(dns_csr)],
                    ["openssl","x509","-req","-in",str(dns_csr),"-CA",str(dns_ca),"-CAkey",str(dns_ca_key),
                     "-set_serial","1","-days","1","-extfile",str(dns_ext),"-out",str(dns_cert)]]:
                    subprocess.run(command,check=True,timeout=20,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
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
                dns_tls.load_cert_chain(str(dns_cert),str(dns_key))
                dns_server.socket=dns_tls.wrap_socket(dns_server.socket,server_side=True)
                dns_thread=threading.Thread(target=dns_server.serve_forever,daemon=True);dns_thread.start()
                policy=scratch/"dns-policy.json"
                policy.write_text(json.dumps({"namespace":namespace,"secret_hex":secrets.token_hex(32),
                    "resolver_url":f"https://localhost:{dns_port}/dns-query","resolver_ca_file":str(dns_ca)}))
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
            control_log = Path(logs[-1].name)
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
                code,value=api("PUT",binding,{"port":8080,"challenge_expires_at":challenge["expires_at"]})
                if code!=403:
                    raise RuntimeError(f"absent TXT proof: expected 403, got {code}: {value}")
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
            context = ssl.create_default_context(cafile=str(paths.get("certificate_ca", cert)))
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
            if args.tls_bundle:
                def peer_fingerprint():
                    connection=LocalHTTPS("app.example.test",proxy_port,timeout=20,context=context)
                    try:
                        connection.connect()
                        return hashlib.sha256(connection.sock.getpeercert(binary_form=True)).hexdigest()
                    finally: connection.close()
                initial_fingerprint=peer_fingerprint()
                unexpected=LocalHTTPS("unbound.example.test",proxy_port,timeout=20,context=context)
                try:
                    unexpected.connect()
                except ssl.SSLError: pass
                else: raise RuntimeError("unconfigured SNI handshake was accepted")
                finally: unexpected.close()
                execute(sandbox,"dd if=/dev/zero of=/root/domain-www/large.bin bs=1048576 count=16 2>/dev/null")
                old_connection=LocalHTTPS("app.example.test",proxy_port,timeout=20,context=context)
                try:
                    old_connection.connect()
                    old_connection.request("GET","/large.bin")
                    old_response=old_connection.getresponse()
                    if old_response.status!=200 or old_response.read(1024)!=bytes(1024):
                        raise RuntimeError("guest stream setup failed")
                    renewed_cert,renewed_key=scratch/"renewed-cert.pem",scratch/"renewed-key.pem"
                    if not args.certificate_lineage:
                        subprocess.run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1",
                            "-subj","/CN=app.example.test","-addext","subjectAltName=DNS:app.example.test,DNS:fallback.example.test",
                            "-addext","basicConstraints=critical,CA:FALSE","-addext","extendedKeyUsage=serverAuth",
                            "-keyout",str(renewed_key),"-out",str(renewed_cert)],check=True,timeout=20,
                            stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                        context.load_verify_locations(cafile=str(renewed_cert))
                    def publish_bundle(doc):
                        proposed=bundle_file.with_suffix(".next")
                        proposed.write_text(json.dumps(doc));os.replace(proposed,bundle_file)
                    if args.deploy_hook:
                        lineage = args.certificate_lineage or scratch / "renewed-lineage"
                        if not args.certificate_lineage:
                            lineage.mkdir(mode=0o700)
                            (lineage / "fullchain.pem").write_bytes(renewed_cert.read_bytes())
                            (lineage / "privkey.pem").write_bytes(renewed_key.read_bytes())
                            (lineage / "privkey.pem").chmod(0o600)
                        generations = scratch / "certificate-generations"
                        generations.mkdir(mode=0o700)
                        trust = scratch / "certificate-trust.pem"
                        trust.write_bytes(paths["certificate_ca"].read_bytes() if args.certificate_lineage
                                          else cert.read_bytes() + renewed_cert.read_bytes())
                        hook = [sys.executable, str(paths["deploy_hook"]),
                            "--manifest", str(bundle_file), "--lineage", str(lineage),
                            "--generations", str(generations), "--control-plane", str(paths["control_plane"]),
                            "--pid", str(control.pid), "--port", str(proxy_port),
                            "--domain", "app.example.test", "--ca-file", str(trust)]
                        if args.certificate_lineage:
                            if paths["renew_command_file"].stat().st_size > 65536:
                                raise RuntimeError("renewal command exceeds size limit")
                            command = json.loads(paths["renew_command_file"].read_text())
                            if not isinstance(command, list) or not command or not all(isinstance(value, str) for value in command):
                                raise RuntimeError("renewal command must be an explicit argv array")
                            if args.renewal_worker:
                                def value(flag):
                                    if command.count(flag) != 1:
                                        raise RuntimeError("renewal command must configure exactly one " + flag)
                                    return command[command.index(flag) + 1]
                                worker_config = scratch / "renewal-worker.json"
                                worker_state = scratch / "renewal-state"
                                worker_state.mkdir(mode=0o700)
                                worker_config.write_text(json.dumps({"certbot": command[0],
                                    "config_dir": value("--config-dir"), "work_dir": value("--work-dir"),
                                    "logs_dir": value("--logs-dir"), "server": value("--server"),
                                    "acme_ca_file": str(paths["acme_ca"]), "jobs": [{"id": "guest-app",
                                    "cert_name": value("--cert-name"), "lineage": str(lineage),
                                    "manifest": str(bundle_file), "generations": str(generations),
                                    "control_plane": str(paths["control_plane"]), "domains": ["app.example.test"],
                                    "port": proxy_port, "ca_file": str(trust)}]}))
                                if args.initial_issuance_webroot:
                                    initial_config = json.loads(worker_config.read_bytes())
                                    initial_config.update(config_dir=str(scratch / "initial-account"),
                                        work_dir=str(scratch / "initial-work"), logs_dir=str(scratch / "initial-logs"))
                                    lineage = scratch / "initial-account/live/guest-initial"
                                    initial_config["jobs"][0].update(cert_name="guest-initial", lineage=str(lineage),
                                        initial_issuance={"webroot": str(args.initial_issuance_webroot.resolve()),
                                            "email": "operator@example.test", "agree_tos": True})
                                    worker_config.write_text(json.dumps(initial_config))
                                    discovery_config = scratch / "domain-discovery.json"
                                    template = initial_config["jobs"][0]
                                    for field in ("id", "cert_name", "lineage", "domains"):
                                        template.pop(field)
                                    discovery_config.write_text(json.dumps({"origin": base, "ca_file": str(api_cert),
                                        "allowed_suffixes": ["example.test"], "template": template}))
                                    initial_config["jobs"] = []
                                    worker_config.write_text(json.dumps(initial_config))
                                    lineage = scratch / "initial-account/live" / ("domain-" + hashlib.sha256(b"app.example.test").hexdigest())
                                    publish_bundle({"default": {"cert_path": str(cert), "key_path": str(private_key)},
                                                    "certificates": []})
                                    control.send_signal(signal.SIGHUP)
                                worker_command = [sys.executable, str(paths["renewal_worker"]),
                                    "--config", str(worker_config), "--state", str(worker_state / "state.json"),
                                    "--force-renewal"]
                                if args.initial_issuance_webroot:
                                    worker_command += ["--discovery-config", str(discovery_config)]
                                result = subprocess.run(worker_command, env=dict(env, HV2_DOMAIN_DISCOVERY_API_KEY=key),
                                                        capture_output=True, timeout=120, check=True)
                                job = json.loads(result.stdout)["jobs"][0]
                                if not job.get("success") or not job.get("activation_verified"):
                                    raise RuntimeError("renewal worker did not verify guest TLS activation")
                                report["renewal_worker"] = job
                                if args.initial_issuance_webroot:
                                    report["initial_issuance_kvm_verified"] = True
                                    report["automatic_discovery_kvm_verified"] = True
                                    report["checks"].append("fresh-account initial issuance provisions a named TLS group during real guest traffic")
                                    fault = scratch / "pending-discovery-fault.py"
                                    fault.write_text("import importlib.util\n"
                                        "spec=importlib.util.spec_from_file_location('owned_worker'," + repr(str(paths["renewal_worker"])) + ")\n"
                                        "module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)\n"
                                        "original=module.run_cycle\n"
                                        "def unavailable(*args): raise OSError('owned activation fault')\n"
                                        "def cycle(*args,**kwargs):\n kwargs['activate_fn']=unavailable\n return original(*args,**kwargs)\n"
                                        "module.run_cycle=cycle\nraise SystemExit(module.main())\n")
                                    fault_command = list(worker_command); fault_command[1] = str(fault)
                                    worker_env = dict(env, HV2_DOMAIN_DISCOVERY_API_KEY=key)
                                    failed = subprocess.run(fault_command, env=worker_env, capture_output=True, timeout=120)
                                    state_file = worker_state / "state.json"
                                    pending_bytes = state_file.read_bytes()
                                    pending_state = json.loads(pending_bytes)["jobs"][job["id"]]
                                    if failed.returncode != 1 or pending_state["pending"]["phase"] != "retry_deployment":
                                        raise RuntimeError("Owned activation failure did not retain pending discovered deployment")
                                    manifest_before_unbind = bundle_file.read_bytes()
                                    issued_hash = digest(lineage / "fullchain.pem")
                                    require("DELETE", binding, status=204)
                                    refused = subprocess.run(worker_command, env=worker_env, capture_output=True, timeout=60)
                                    if (refused.returncode != 1 or b"remove a pending job" not in refused.stderr
                                            or state_file.read_bytes() != pending_bytes or bundle_file.read_bytes() != manifest_before_unbind
                                            or digest(lineage / "fullchain.pem") != issued_hash):
                                        raise RuntimeError("Unbound pending job was executed or lost its durable state")
                                    require("PUT", binding, {"port": 8080})
                                    recovered = subprocess.run(worker_command, env=worker_env, capture_output=True, timeout=120, check=True)
                                    recovery_job = json.loads(recovered.stdout)["jobs"][0]
                                    if not recovery_job.get("success") or digest(lineage / "fullchain.pem") != issued_hash:
                                        raise RuntimeError("Restored claim did not activate the retained issued certificate")
                                    report["pending_claim_interruption"] = {"unbind_refused_without_mutation": True,
                                        "retained_certificate_activated": True, "fault_wrapper_sha256": digest(fault),
                                        "fault_wrapper_source": fault.read_text()}
                                    report["checks"].append("real unbind preserves and refuses a pending discovered job; restored claim activates retained certificate")
                            else:
                                subprocess.run(command + ["--deploy-hook", shlex.join(hook + ["--from-certbot"])],
                                               env=env, capture_output=True, timeout=90, check=True)
                            renewed_cert.write_bytes((lineage / "fullchain.pem").read_bytes())
                            renewed_key.write_bytes((lineage / "privkey.pem").read_bytes())
                            renewed_key.chmod(0o600)
                            deployed = json.loads(bundle_file.read_text())["certificates"][0]
                            if Path(deployed["cert_path"]).read_bytes() != renewed_cert.read_bytes():
                                raise RuntimeError("ACME deployment manifest does not contain the renewed chain")
                            report["certificate_deployment"] = {"certbot_deploy_hook": not bool(args.renewal_worker),
                                "renewal_worker": bool(args.renewal_worker), "activation_verified": False}
                        else:
                            deployment = subprocess.run(hook, env=env, capture_output=True, timeout=45, check=True)
                            report["certificate_deployment"] = json.loads(deployment.stdout)
                            if not report["certificate_deployment"].get("activation_verified"):
                                raise RuntimeError("deploy hook did not verify TLS activation")
                    else:
                        renewed={"certificates":[{"names":["app.example.test"],"cert_path":str(renewed_cert),"key_path":str(renewed_key)}]}
                        publish_bundle(renewed);control.send_signal(signal.SIGHUP)
                    renewed={"certificates":[{"names":["app.example.test"],"cert_path":str(renewed_cert),"key_path":str(renewed_key)}]}
                    der = subprocess.run(["openssl", "x509", "-in", str(renewed_cert), "-outform", "DER"],
                                         capture_output=True, timeout=10, check=True).stdout
                    expected_fingerprint=hashlib.sha256(der).hexdigest()
                    deadline=time.monotonic()+10
                    while peer_fingerprint()!=expected_fingerprint:
                        if time.monotonic()>=deadline: raise RuntimeError("certificate renewal was not activated")
                        time.sleep(.02)
                    if expected_fingerprint==initial_fingerprint: raise RuntimeError("renewal did not change the leaf")
                    if args.certificate_lineage:
                        report["certificate_deployment"].update(activation_verified=True,
                            previous_leaf_sha256=initial_fingerprint, active_leaf_sha256=expected_fingerprint)
                    remaining=16*1048576-1024
                    while remaining:
                        chunk=old_response.read(min(1048576,remaining))
                        if not chunk or any(chunk): raise RuntimeError("old TLS guest stream failed after renewal")
                        remaining-=len(chunk)
                    if old_response.read(1): raise RuntimeError("guest stream length exceeded expected payload")
                finally: old_connection.close()
                report["checks"].append("new TLS handshakes use the renewed certificate while the old guest stream completes")
                publish_bundle({"certificates":[{"names":["app.example.test"],"cert_path":str(renewed_cert),"key_path":str(private_key)}]})
                control.send_signal(signal.SIGHUP)
                deadline=time.monotonic()+10
                while "TLS certificate bundle reload rejected" not in control_log.read_text(errors="replace"):
                    if time.monotonic()>=deadline: raise RuntimeError("invalid reload rejection was not observed")
                    time.sleep(.02)
                retained_fingerprint=peer_fingerprint()
                if retained_fingerprint!=expected_fingerprint or guest()!=(200,marker):
                    raise RuntimeError("invalid certificate/key reload changed active routing or TLS identity")
                publish_bundle(renewed)
                report["checks"].append("mismatched certificate/key reload preserves the active TLS identity and guest route")
                report["checks"].append("unconfigured SNI handshake is refused without a default certificate")
                def fallback_peer():
                    connection=LocalHTTPS("fallback.example.test",proxy_port,timeout=20,context=context)
                    try:
                        connection.connect()
                        return hashlib.sha256(connection.sock.getpeercert(binary_form=True)).hexdigest()
                    finally: connection.close()
                with_default=dict(renewed,default={"cert_path":str(cert),"key_path":str(private_key)})
                publish_bundle(with_default);control.send_signal(signal.SIGHUP)
                deadline=time.monotonic()+10
                while True:
                    try:
                        default_fingerprint=fallback_peer()
                        if default_fingerprint!=initial_fingerprint: raise RuntimeError("default certificate identity differs")
                        break
                    except ssl.SSLError:
                        if time.monotonic()>=deadline: raise RuntimeError("default certificate was not activated")
                        time.sleep(.02)
                publish_bundle(renewed);control.send_signal(signal.SIGHUP)
                deadline=time.monotonic()+10
                while True:
                    try: fallback_peer()
                    except ssl.SSLError: break
                    if time.monotonic()>=deadline: raise RuntimeError("removed default certificate still accepts SNI")
                    time.sleep(.02)
                report["checks"].append("optional default certificate activates and is removed through validated TLS reload")
                if control.poll() is not None: raise RuntimeError("control plane exited during certificate reload checks")
                report["tls_reload"]={"initial_leaf_sha256":initial_fingerprint,"renewed_leaf_sha256":expected_fingerprint,
                    "rejected_reload_retained_leaf_sha256":retained_fingerprint,"default_leaf_sha256":default_fingerprint,
                    "old_connection_guest_bytes_verified":16*1048576,"control_plane_pid":control.pid,
                    "control_plane_alive_after_reload":True,"rejected_reload_observed_in_log":True}
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
