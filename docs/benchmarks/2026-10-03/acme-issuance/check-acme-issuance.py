#!/usr/bin/env python3
"""Owned Pebble/Certbot HTTP-01 protocol check; never contacts a public CA.

Requires Linux, OpenSSL, Certbot, Pebble and pebble-challtestsrv. All state and
private keys live in a temporary directory and are removed after the check.
This verifies the issuer, not HyperMachine deployment or renewal scheduling.
"""
import argparse
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.request


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def send_response(self, code, message=None):
        if code == 200 and self.path.startswith("/.well-known/acme-challenge/"):
            with self.server.counter_lock:
                self.server.successful_challenges += 1
        super().send_response(code, message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("pebble", "challenge-server", "certbot", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit("Refusing to replace an existing report")
    binaries = {name: str(getattr(args, name).resolve())
                for name in ("pebble", "challenge_server", "certbot")}
    before = {name: digest(path) for name, path in binaries.items()}
    env = {key: value for key, value in os.environ.items()
           if key.lower() not in ("http_proxy", "https_proxy", "all_proxy", "no_proxy")
           and not key.startswith("PEBBLE_")}
    # No always-valid mode; disable random delays and cached authorizations so
    # both renewal and the negative attempt must fetch an actual HTTP challenge.
    env.update(PEBBLE_VA_NOSLEEP="1", PEBBLE_AUTHZREUSE="0", PEBBLE_WFE_NONCEREJECT="0")
    processes = []
    server = thread = None
    report = {"functional_only": True, "public_ca_contacted": False,
              "hypermachine_deployment_verified": False, "checks": []}
    with tempfile.TemporaryDirectory(prefix="hm-acme-") as temporary:
        root = Path(temporary)
        def run(argv, acceptable=(0,)):
            result = subprocess.run(argv, env=env, cwd=root, capture_output=True, timeout=90)
            if result.returncode not in acceptable:
                # Diagnostics are ephemeral: never copy account keys/tokens to reports.
                raise RuntimeError("Command failed: " + str(argv[0]) + "\n" +
                                   result.stderr.decode(errors="replace")[-3000:])
            return result
        try:
            run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                 "-keyout", "ca.key", "-out", "ca.pem", "-days", "2",
                 "-subj", "/CN=Owned ACME transport CA",
                 "-addext", "basicConstraints=critical,CA:TRUE",
                 "-addext", "keyUsage=critical,keyCertSign,cRLSign"])
            run(["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
                 "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=localhost"])
            (root / "extensions").write_text("basicConstraints=critical,CA:FALSE\n"
                "subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n")
            run(["openssl", "x509", "-req", "-in", "server.csr", "-CA", "ca.pem",
                 "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.pem",
                 "-days", "2", "-extfile", "extensions"])
            webroot = root / "webroot"
            webroot.mkdir()
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0),
                functools.partial(QuietHandler, directory=str(webroot)))
            server.counter_lock = threading.Lock()
            server.successful_challenges = 0
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            acme_port, management_port, dns_port, dns_management = [port() for _ in range(4)]
            config = {"pebble": {"listenAddress": f"127.0.0.1:{acme_port}",
                "managementListenAddress": f"127.0.0.1:{management_port}",
                "certificate": str(root / "server.pem"), "privateKey": str(root / "server.key"),
                "httpPort": server.server_port, "tlsPort": port(),
                "externalAccountBindingRequired": False,
                "retryAfter": {"authz": 1, "order": 1}}}
            (root / "config.json").write_text(json.dumps(config))
            for command in ([binaries["challenge_server"], "-dnsserver", f"127.0.0.1:{dns_port}",
                             "-management", f"127.0.0.1:{dns_management}", "-http01=", "-https01=",
                             "-tlsalpn01=", "-doh=", "-defaultIPv6="],
                            [binaries["pebble"], "-config", str(root / "config.json"),
                             "-dnsserver", f"127.0.0.1:{dns_port}"]):
                processes.append(subprocess.Popen(command, env=env, cwd=root,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
            context = ssl.create_default_context(cafile=str(root / "ca.pem"))
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                                                urllib.request.HTTPSHandler(context=context))
            directory = f"https://localhost:{acme_port}/dir"
            deadline = time.monotonic() + 15
            while True:
                try:
                    with opener.open(directory, timeout=1) as response:
                        json.load(response)
                    break
                except (OSError, ValueError):
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Owned CA failed to start")
                    time.sleep(0.1)
            env["REQUESTS_CA_BUNDLE"] = str(root / "ca.pem")
            common = [binaries["certbot"], "--non-interactive", "--agree-tos",
                "--register-unsafely-without-email", "--server", directory,
                "--config-dir", str(root / "certbot"), "--work-dir", str(root / "work"),
                "--logs-dir", str(root / "logs")]
            issue = common + ["certonly", "--webroot", "-w", str(webroot),
                              "-d", "app.example.test", "--cert-name", "owned-acme"]
            run(issue)
            issuance_requests = server.successful_challenges
            if issuance_requests < 1:
                raise RuntimeError("Issuance skipped actual HTTP-01 validation")
            cert = root / "certbot/live/owned-acme/cert.pem"
            first = digest(cert)
            with opener.open(f"https://localhost:{management_port}/roots/0", timeout=5) as response:
                (root / "issuer.pem").write_bytes(response.read())
            chain = root / "certbot/live/owned-acme/chain.pem"
            run(["openssl", "verify", "-CAfile", str(root / "issuer.pem"),
                 "-untrusted", str(chain), "-verify_hostname", "app.example.test", str(cert)])
            report["checks"].append("HTTP-01 issuance and issuer/hostname verification")
            renewal = common + ["renew", "--cert-name", "owned-acme", "--force-renewal",
                                "--no-random-sleep-on-renew"]
            run(renewal)
            renewal_requests = server.successful_challenges - issuance_requests
            if renewal_requests < 1:
                raise RuntimeError("Renewal skipped actual HTTP-01 validation")
            renewed = digest(cert)
            if renewed == first:
                raise RuntimeError("Renewal did not replace the certificate")
            run(["openssl", "verify", "-CAfile", str(root / "issuer.pem"),
                 "-untrusted", str(chain), "-verify_hostname", "app.example.test", str(cert)])
            report["checks"].append("Forced renewal produces a different valid certificate")
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            result = run(renewal, acceptable=(0, 1))
            if result.returncode == 0 or digest(cert) != renewed:
                raise RuntimeError("Missing HTTP challenge did not fail safely")
            report["checks"].append("Unavailable HTTP-01 server refuses renewal and retains certificate")
            report.update(initial_certificate_sha256=first, renewed_certificate_sha256=renewed,
                          issuance_http01_successful_requests=issuance_requests,
                          renewal_http01_successful_requests=renewal_requests,
                          certbot_version=run([binaries["certbot"], "--version"]).stdout.decode().strip(),
                          pebble_version=run([binaries["pebble"], "-version"]).stdout.decode().strip())
        finally:
            if server:
                server.shutdown()
                server.server_close()
            if thread:
                thread.join(timeout=5)
            for process in reversed(processes):
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        report["processes_reaped"] = all(process.poll() is not None for process in processes)
        report["http_thread_stopped"] = thread is not None and not thread.is_alive()
    if before != {name: digest(path) for name, path in binaries.items()}:
        raise RuntimeError("Input executable changed during verification")
    report["input_sha256"] = before
    report["driver_sha256"] = digest(__file__)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
