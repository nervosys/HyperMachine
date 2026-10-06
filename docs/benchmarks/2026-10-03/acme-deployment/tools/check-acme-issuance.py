#!/usr/bin/env python3
"""Owned Pebble/Certbot HTTP-01 protocol check; never contacts a public CA.

Requires Linux, OpenSSL, Certbot, Pebble and pebble-challtestsrv. All state and
private keys live in a temporary directory and are removed after the check.
With --control-plane and --deploy-hook, also checks live HyperMachine TLS
activation and rollback. It does not verify KVM traffic or renewal scheduling.
"""
import argparse
import datetime
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import signal
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
    parser.add_argument("--control-plane", type=Path)
    parser.add_argument("--deploy-hook", type=Path)
    for name in ("kvm-verifier", "daemon", "kernel", "initrd"):
        parser.add_argument("--" + name, type=Path)
    args = parser.parse_args()
    if bool(args.control_plane) != bool(args.deploy_hook):
        parser.error("--control-plane and --deploy-hook must be supplied together")
    kvm_options = (args.kvm_verifier, args.daemon, args.kernel, args.initrd)
    if any(kvm_options) and (not all(kvm_options) or not args.control_plane):
        parser.error("KVM options must all be supplied with --control-plane and --deploy-hook")
    if args.output.exists():
        raise SystemExit("Refusing to replace an existing report")
    binaries = {name: str(getattr(args, name).resolve())
                for name in ("pebble", "challenge_server", "certbot")}
    if args.control_plane:
        binaries.update(control_plane=str(args.control_plane.resolve()), deploy_hook=str(args.deploy_hook.resolve()))
    if args.kvm_verifier:
        binaries.update({name: str(getattr(args, name).resolve()) for name in ("kvm_verifier", "daemon", "kernel", "initrd")})
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
        def run(argv, acceptable=(0,), timeout=90):
            process = subprocess.Popen(argv, env=env, cwd=root, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, start_new_session=True)
            try:
                stdout, stderr = process.communicate(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate()
                raise
            result = subprocess.CompletedProcess(argv, process.returncode, stdout, stderr)
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
            if args.kvm_verifier:
                issue += ["-d", "fallback.example.test"]
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
            if args.control_plane:
                import sys
                generations = root / "generations"
                generations.mkdir(mode=0o700)
                initial = generations / "initial"
                initial.mkdir(mode=0o700)
                lineage = cert.parent
                for filename in ("fullchain.pem", "privkey.pem"):
                    shutil.copyfile(lineage / filename, initial / filename)
                    (initial / filename).chmod(0o600)
                bundle = root / "bundle.json"
                bundle.write_text(json.dumps({"certificates": [{"names": ["app.example.test"],
                    "cert_path": str(initial / "fullchain.pem"), "key_path": str(initial / "privkey.pem")}]}))
                proxy_port = port()
                control_env = {"PATH": env.get("PATH", "/usr/bin:/bin"), "RUST_LOG": "warn",
                               "HV2_API_KEY": secrets.token_hex(32)}
                control = subprocess.Popen([binaries["control_plane"], "--store", "memory:",
                    "--port", str(port()), "--proxy-port", str(proxy_port),
                    "--tls-bundle-file", str(bundle)], env=control_env,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                processes.append(control)
                guest_context = ssl.create_default_context(cafile=str(root / "issuer.pem"))
                def active_leaf():
                    with socket.create_connection(("127.0.0.1", proxy_port), timeout=2) as raw:
                        with guest_context.wrap_socket(raw, server_hostname="app.example.test") as tls:
                            return hashlib.sha256(tls.getpeercert(binary_form=True)).hexdigest()
                def await_leaf(expected):
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        if control.poll() is not None:
                            raise RuntimeError("Control plane exited during certificate deployment")
                        try:
                            if active_leaf() == expected:
                                return
                        except OSError:
                            pass
                        time.sleep(0.05)
                    raise RuntimeError("Control plane did not present the expected ACME leaf")
                first_der = hashlib.sha256(run(["openssl", "x509", "-in", str(cert), "-outform", "DER"]).stdout).hexdigest()
                await_leaf(first_der)
                hook = [sys.executable, binaries["deploy_hook"], "--manifest", str(bundle),
                    "--lineage", str(lineage), "--generations", str(generations),
                    "--control-plane", binaries["control_plane"], "--pid", str(control.pid),
                    "--port", str(proxy_port), "--domain", "app.example.test",
                    "--ca-file", str(root / "issuer.pem"), "--timeout", "2"]
                report["checks"].append("Actual control plane serves the issued ACME certificate")
            renewal = common + ["renew", "--cert-name", "owned-acme", "--force-renewal",
                                "--no-random-sleep-on-renew", "--new-key"]
            if args.control_plane:
                renewal += ["--deploy-hook", shlex.join(hook + ["--from-certbot"])]
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
            if args.control_plane:
                renewed_der = hashlib.sha256(run(["openssl", "x509", "-in", str(cert), "-outform", "DER"]).stdout).hexdigest()
                await_leaf(renewed_der)
                deployed = json.loads(bundle.read_text())["certificates"][0]
                if Path(deployed["cert_path"]).parent == initial:
                    raise RuntimeError("Certbot deploy hook did not publish a new generation")
                if Path(deployed["cert_path"]).read_bytes() != (lineage / "fullchain.pem").read_bytes():
                    raise RuntimeError("Deployed generation differs from renewed full chain")
                if (Path(deployed["key_path"]).stat().st_mode & 0o777) != 0o600:
                    raise RuntimeError("Deployed key permissions are not private")
                deployed_manifest = bundle.read_bytes()
                report["checks"].append("Certbot deploy hook activates the renewed leaf on actual control-plane TLS")
                before_retry = sorted(path.name for path in generations.iterdir())
                retried = json.loads(run(hook).stdout)
                if (not retried.get("unchanged") or bundle.read_bytes() != deployed_manifest
                        or sorted(path.name for path in generations.iterdir()) != before_retry):
                    raise RuntimeError("Deployment retry was not idempotent")
                report["checks"].append("Retrying an active immutable generation verifies TLS without rewriting or duplicating it")
                # A valid-looking leaf from an untrusted issuer must not leave an
                # unusable certificate active. Real client verification triggers rollback.
                bad_lineage = root / "untrusted-lineage"
                bad_lineage.mkdir()
                run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                     "-subj", "/CN=app.example.test", "-addext", "subjectAltName=DNS:app.example.test",
                     "-addext", "basicConstraints=critical,CA:FALSE",
                     "-keyout", str(bad_lineage / "privkey.pem"), "-out", str(bad_lineage / "fullchain.pem")])
                bad_hook = list(hook)
                bad_hook[bad_hook.index("--lineage") + 1] = str(bad_lineage)
                failed = run(bad_hook, acceptable=(1,))
                if b"previous manifest and TLS leaf restored" not in failed.stderr:
                    raise RuntimeError("Deployment failure did not confirm rollback")
                if bundle.read_bytes() != deployed_manifest:
                    raise RuntimeError("Failed activation did not restore exact manifest bytes")
                await_leaf(renewed_der)
                report["checks"].append("Untrusted certificate activation rolls back manifest and verified TLS leaf")
                shutil.copyfile(initial / "privkey.pem", bad_lineage / "privkey.pem")
                failed = run(bad_hook, acceptable=(1,))
                if b"certificate and private key differ" not in failed.stderr:
                    raise RuntimeError("Mismatched-key refusal did not identify the key mismatch")
                if bundle.read_bytes() != deployed_manifest or active_leaf() != renewed_der:
                    raise RuntimeError("Mismatched key changed active deployment")
                report["checks"].append("Mismatched private key is refused before manifest publication")
                wrong_pid = list(hook)
                wrong_pid[wrong_pid.index("--pid") + 1] = str(processes[0].pid)
                failed = run(wrong_pid, acceptable=(1,))
                if b"process executable differs" not in failed.stderr:
                    raise RuntimeError("Wrong-process refusal did not verify the executable")
                if bundle.read_bytes() != deployed_manifest or processes[0].poll() is not None:
                    raise RuntimeError("Wrong process was signaled or manifest changed")
                report["checks"].append("Wrong process executable is refused without signal or mutation")
                wrong_manifest = root / "different-bundle.json"
                wrong_manifest.write_bytes(deployed_manifest)
                wrong_hook = list(hook)
                wrong_hook[wrong_hook.index("--manifest") + 1] = str(wrong_manifest)
                failed = run(wrong_hook, acceptable=(1,))
                if b"process uses a different TLS manifest" not in failed.stderr:
                    raise RuntimeError("Wrong-manifest refusal did not verify the process arguments")
                if wrong_manifest.read_bytes() != deployed_manifest or bundle.read_bytes() != deployed_manifest:
                    raise RuntimeError("Wrong manifest was mutated")
                report["checks"].append("Wrong manifest is refused by checking the live process arguments")
                import fcntl
                with bundle.with_name(bundle.name + ".deploy.lock").open("rb") as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    failed = run(hook, acceptable=(1,))
                    if b"Resource temporarily unavailable" not in failed.stderr:
                        raise RuntimeError("Concurrent deployment did not refuse the locked manifest")
                if bundle.read_bytes() != deployed_manifest:
                    raise RuntimeError("Concurrent deploy bypassed the lock")
                report["checks"].append("Concurrent deployment lock refuses overlapping publication")
                # Load a currently-valid certificate, let it actually expire on
                # the running proxy, then recover with the renewed ACME lineage.
                expiry = root / "expiry"
                expiry.mkdir()
                (expiry / "newcerts").mkdir()
                (expiry / "index").write_text("")
                (expiry / "serial").write_text("1000\n")
                (expiry / "ca.conf").write_text("[ca]\ndefault_ca=owned\n[owned]\n"
                    f"database={expiry}/index\nnew_certs_dir={expiry}/newcerts\nserial={expiry}/serial\n"
                    f"certificate={root}/ca.pem\nprivate_key={root}/ca.key\n"
                    "default_md=sha256\ndefault_days=1\npolicy=names\n[names]\ncommonName=supplied\n")
                (expiry / "extensions").write_text("subjectAltName=DNS:app.example.test\n"
                    "basicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                    "extendedKeyUsage=serverAuth\n")
                run(["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes",
                     "-subj", "/CN=app.example.test", "-keyout", str(expiry / "key.pem"),
                     "-out", str(expiry / "request.pem")])
                now = datetime.datetime.now(datetime.timezone.utc)
                expires_at = now + datetime.timedelta(seconds=12)
                run(["openssl", "ca", "-batch", "-notext", "-config", str(expiry / "ca.conf"),
                     "-in", str(expiry / "request.pem"), "-out", str(expiry / "cert.pem"),
                     "-startdate", (now - datetime.timedelta(seconds=30)).strftime("%y%m%d%H%M%SZ"),
                     "-enddate", expires_at.strftime("%y%m%d%H%M%SZ"),
                     "-extfile", str(expiry / "extensions")])
                expiry_leaf = hashlib.sha256(run(["openssl", "x509", "-in", str(expiry / "cert.pem"),
                                                  "-outform", "DER"]).stdout).hexdigest()
                bundle.write_text(json.dumps({"certificates": [{"names": ["app.example.test"],
                    "cert_path": str(expiry / "cert.pem"), "key_path": str(expiry / "key.pem")}]}))
                control.send_signal(signal.SIGHUP)
                expiry_context = ssl.create_default_context(cafile=str(root / "ca.pem"))
                deadline = time.monotonic() + 5
                while True:
                    try:
                        with socket.create_connection(("127.0.0.1", proxy_port), timeout=1) as raw:
                            with expiry_context.wrap_socket(raw, server_hostname="app.example.test") as tls:
                                actual = hashlib.sha256(tls.getpeercert(binary_form=True)).hexdigest()
                        if actual == expiry_leaf:
                            break
                    except OSError:
                        pass
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Short-lived certificate did not activate before expiry")
                    time.sleep(0.05)
                while datetime.datetime.now(datetime.timezone.utc) <= expires_at + datetime.timedelta(seconds=1):
                    time.sleep(0.1)
                try:
                    with socket.create_connection(("127.0.0.1", proxy_port), timeout=1) as raw:
                        with expiry_context.wrap_socket(raw, server_hostname="app.example.test"):
                            raise RuntimeError("The short-lived certificate did not expire")
                except ssl.SSLCertVerificationError as error:
                    if error.verify_code != 10:
                        raise RuntimeError("Expected actual certificate-expired refusal") from error
                recovered = json.loads(run(hook).stdout)
                if recovered.get("previous_leaf_sha256") != expiry_leaf or not recovered.get("activation_verified"):
                    raise RuntimeError("Expired certificate recovery was not verified")
                await_leaf(renewed_der)
                deployed_manifest = bundle.read_bytes()
                report["checks"].append("Already-expired active certificate recovers through verified ACME deployment")
                report["expired_active_leaf_sha256"] = expiry_leaf
                report.update(hypermachine_deployment_verified=True,
                              initial_proxy_leaf_sha256=first_der, renewed_proxy_leaf_sha256=renewed_der,
                              control_plane_alive_after_deployment=control.poll() is None,
                              guest_kvm_traffic_verified=False)
            if args.kvm_verifier:
                renewal_command = root / "renew-command.json"
                # The guest verifier supplies its own pinned PID and manifest hook.
                renewal_command.write_text(json.dumps(renewal[:renewal.index("--deploy-hook")]))
                kvm_report_path = root / "kvm-report.json"
                before_kvm_requests = server.successful_challenges
                run([sys.executable, binaries["kvm_verifier"], "--control-plane", binaries["control_plane"],
                    "--daemon", binaries["daemon"], "--kernel", binaries["kernel"], "--initrd", binaries["initrd"],
                    "--output", str(kvm_report_path), "--dns-ownership", "--tls-bundle",
                    "--deploy-hook", binaries["deploy_hook"], "--certificate-lineage", str(lineage),
                    "--certificate-ca", str(root / "issuer.pem"), "--renew-command-file", str(renewal_command)], timeout=180)
                kvm_report = json.loads(kvm_report_path.read_text())
                kvm_requests = server.successful_challenges - before_kvm_requests
                if not kvm_report.get("success") or not kvm_report.get("acme_renewal_enabled") or kvm_requests < 1:
                    raise RuntimeError("Real ACME-to-KVM deployment did not pass")
                report.update(guest_kvm_traffic_verified=True, kvm_report=kvm_report,
                              kvm_renewal_http01_successful_requests=kvm_requests,
                              kvm_renewed_certificate_sha256=digest(cert))
                report["checks"].append("Real HTTP-01 renewal activates through KVM guest traffic while an old 16 MiB TLS download completes")
            before_refused_digest = digest(cert)
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            result = run(renewal, acceptable=(0, 1))
            if result.returncode == 0 or digest(cert) != before_refused_digest:
                raise RuntimeError("Missing HTTP challenge did not fail safely")
            if args.control_plane and (bundle.read_bytes() != deployed_manifest or active_leaf() != renewed_der):
                raise RuntimeError("Failed issuance changed deployed manifest or active certificate")
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
