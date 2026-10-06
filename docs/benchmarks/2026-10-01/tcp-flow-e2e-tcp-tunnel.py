#!/usr/bin/env python3
"""Functional KVM TCP tunnel verification with owned Redis/TLS/CLI processes.

Requires a guest image containing tcp-fixture (tools/guest-image/tcp-fixture.c)
and the current static guest agent. Never reports performance scores.
"""
import argparse
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import selectors
import signal
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    return process.poll() is not None


def receive_all(stream):
    parts = []
    while True:
        data = stream.recv(65536)
        if not data:
            return b"".join(parts)
        parts.append(data)
        if sum(map(len, parts)) > 4 * 1024 * 1024:
            raise RuntimeError("unexpectedly large TCP response")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control-plane", "cli", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    paths = {name: getattr(args, name.replace("-", "_")) for name in ["daemon", "control-plane", "cli", "kernel", "initrd"]}
    paths["coordinator"] = Path(__file__)
    report = {"success": False, "purpose": "functional verification, no performance comparison",
              "artifact_sha256": {name: digest(path) for name, path in paths.items()},
              "cases": [], "cleanup_errors": [], "environment": "local WSL nested KVM; 1 vCPU / 1024 MiB guests"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    processes, guests, logs, sockets = [], set(), {}, []
    key, token, inventory_key = (uuid.uuid4().hex for _ in range(3))

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        try:
            result = action()
            if result is not None:
                row["result"] = result
            row["success"] = True
        except Exception as error:
            row["error"] = str(error)
            raise

    with tempfile.TemporaryDirectory(prefix="hm-tcp-e2e-", dir="/var/tmp") as temporary:
        directory = Path(temporary)

        def run(command):
            subprocess.run(command, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

        def start(name, command, environment):
            log = (directory / (name + ".log")).open("wb")
            logs[name] = log
            process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                                       stdout=log, stderr=subprocess.STDOUT)
            processes.append((name, process))
            return process

        env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn"}
        context = None
        api_url = None

        def api(method, path, body=None, supplied_key=key, expected=200):
            data = None if body is None else json.dumps(body).encode()
            request = urllib.request.Request(api_url + path, data=data, method=method,
                                             headers={"x-api-key": supplied_key, "content-type": "application/json"})
            try:
                response = urllib.request.urlopen(request, context=context, timeout=90)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                raw = response.read()
                if response.status != expected:
                    raise RuntimeError(f"{method} {path}: {response.status}: {raw[:1000]!r}")
                return json.loads(raw) if raw else None

        def command(id, cmd):
            result = api("POST", f"/sandboxes/{id}/exec", {"cmd": cmd, "timeout_secs": 10})
            if result.get("exit_code") != 0 or result.get("timed_out") or result.get("truncated"):
                raise RuntimeError(f"guest command failed: {result}")
            return result["stdout"]

        def tunnel(port, id, target_context=context, target_port=None, credential=key, cluster=False, expected=101):
            connection = http.client.HTTPSConnection("127.0.0.1", target_port or api_port,
                                                      timeout=10, context=target_context or context)
            headers = {"connection": "upgrade", "upgrade": "hv2-tcp/1",
                       ("x-hv2-cluster-token" if cluster else "x-api-key"): credential}
            connection.request("GET", f"/sandboxes/{id}/ports/{port}/tcp", headers=headers)
            response = connection.getresponse()
            if response.status != expected:
                raise RuntimeError(f"TCP handshake: {response.status}, wanted {expected}")
            if expected != 101:
                response.read()
                connection.close()
                return {"status": expected}
            if response.getheader("upgrade") != "hv2-tcp/1":
                raise RuntimeError("wrong upgraded protocol")
            # The fixture waits for input on tested raw TLS ports; there is
            # no response payload buffered in http.client's header reader.
            stream = connection.sock
            if stream is None:
                raise RuntimeError("HTTP client discarded upgraded connection")
            sockets.append(stream)
            return stream

        def cli(id, port, api_ca=True, supplied_key=key):
            command_line = [str(args.cli), "sandbox", "vm", "--endpoint", api_url]
            if api_ca:
                command_line += ["--api-ca-cert", str(directory / "ca.pem")]
            command_line += ["--request-timeout", "10", "tcp", id, "--port", str(port), "--listen", "127.0.0.1:0"]
            error_log = (directory / f"cli-{len(processes)}.log").open("wb")
            name = f"cli-{len(processes)}"
            logs[name] = error_log
            process = subprocess.Popen(command_line, env=dict(env, HV2_API_KEY=supplied_key),
                                       stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=error_log)
            processes.append((name, process))
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                if not selector.select(15):
                    raise TimeoutError("CLI did not report its bound listener")
                line = process.stdout.readline()
            if not line:
                process.wait(timeout=5)
                raise RuntimeError(f"CLI exited before listening: {process.returncode}")
            address = json.loads(line)["listen"]
            host, port = address.rsplit(":", 1)
            return process, (host, int(port))

        payload = bytes(range(256)) * 1024

        def mirror(id, count=1):
            process, address = cli(id, 18080)
            try:
                for _ in range(count):
                    with socket.create_connection(address, timeout=10) as stream:
                        stream.sendall(payload)
                        stream.shutdown(socket.SHUT_WR)
                        reply = receive_all(stream)
                        if reply != payload:
                            raise RuntimeError(f"binary reply mismatch: {len(reply)} bytes")
                return {"connections": count, "bytes_per_connection": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
            finally:
                process.send_signal(signal.SIGINT)
                process.wait(timeout=5)
                if process.returncode != 0:
                    raise RuntimeError(f"CLI interrupt exit {process.returncode}")

        try:
            run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=HM TCP fixture CA",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(directory / "ca.key"), "-out", str(directory / "ca.pem")])
            for name, usage, san in [("node", "serverAuth", "DNS:tcp-node.test,IP:127.0.0.1"),
                                     ("control", "clientAuth", "DNS:tcp-control.test"),
                                     ("api", "serverAuth", "IP:127.0.0.1,DNS:localhost")]:
                run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                     "-keyout", str(directory / (name + ".key")), "-out", str(directory / (name + ".csr"))])
                extensions = directory / (name + ".ext")
                extensions.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage={usage}\nsubjectAltName={san}\n")
                run(["openssl", "x509", "-req", "-in", str(directory / (name + ".csr")), "-CA", str(directory / "ca.pem"),
                     "-CAkey", str(directory / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(extensions),
                     "-out", str(directory / (name + ".pem"))])
            context = ssl.create_default_context(cafile=str(directory / "ca.pem"))
            node_context = ssl.create_default_context(cafile=str(directory / "ca.pem"))
            node_context.load_cert_chain(str(directory / "control.pem"), str(directory / "control.key"))
            ports = []
            while len(ports) < 5:
                port = free_port()
                if port not in ports:
                    ports.append(port)
            redis_port, node_port, node_proxy, api_port, api_proxy = ports
            api_url = f"https://127.0.0.1:{api_port}"
            store = f"redis://127.0.0.1:{redis_port}"
            namespace = "tcp-" + uuid.uuid4().hex
            start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "", "--appendonly", "no", "--dir", str(directory)], env)
            deadline = time.monotonic() + 5
            while True:
                try:
                    with socket.create_connection(("127.0.0.1", redis_port), timeout=.2):
                        break
                except OSError:
                    if time.monotonic() > deadline:
                        raise
                    time.sleep(.02)
            policies = directory / "keys.json"
            policies.write_text(json.dumps([{"sha256": hashlib.sha256(inventory_key.encode()).hexdigest(),
                                             "expires_at": int(time.time()) + 3600, "scopes": ["inventory"]}]))
            start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "4", "--volume-dir", str(directory / "volumes"),
                "--snapshot-store", str(directory / "snapshots"), "--cluster-store", store, "--cluster-namespace", namespace,
                "--node-id", "tcp-kvm-node", "--advertise-api", f"https://127.0.0.1:{node_port}",
                "--advertise-proxy", f"127.0.0.1:{node_proxy}", "--mtls-ca", str(directory / "ca.pem"),
                "--mtls-cert", str(directory / "node.pem"), "--mtls-key", str(directory / "node.key")],
                dict(env, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), HV2_CLUSTER_TOKEN=token))
            start("control", [str(args.control_plane), "--store", store, "--namespace", namespace, "--port", str(api_port),
                "--proxy-port", str(api_proxy), "--api-keys-file", str(policies), "--api-tls-cert", str(directory / "api.pem"),
                "--api-tls-key", str(directory / "api.key"), "--mtls-ca", str(directory / "ca.pem"),
                "--mtls-cert", str(directory / "control.pem"), "--mtls-key", str(directory / "control.key"),
                "--mtls-node-name", "tcp-node.test"], dict(env, HV2_API_KEY=key, HV2_CLUSTER_TOKEN=token))
            deadline = time.monotonic() + 40
            while True:
                if any(process.poll() is not None for _, process in processes):
                    raise RuntimeError("owned service exited during setup")
                try:
                    templates = api("GET", "/templates")
                    if any("base" in row.get("aliases", []) for row in templates):
                        break
                except OSError:
                    pass
                if time.monotonic() > deadline:
                    raise TimeoutError("prepared template was not advertised")
                time.sleep(.05)
            report["transport"] = "CLI -> verified API TLS -> control plane -> node mTLS plus cluster token -> vsock -> loopback guest TCP"
            created = api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 300, "allowInternetAccess": False}, expected=201)
            id = created["sandboxID"]
            guests.add(id)
            info = api("GET", f"/sandboxes/{id}")
            assert info["cpuCount"] == 1 and info["memoryMB"] == 1024
            command(id, "/bin/tcp-fixture </dev/null >/tmp/tcp-fixture.log 2>&1 &")
            deadline = time.monotonic() + 5
            while command(id, "test -f /tmp/tcp-fixture-ready && printf ready || true") != "ready":
                if time.monotonic() > deadline:
                    raise TimeoutError("guest fixture did not start")
                time.sleep(.01)
            case("bad-api-key", lambda: tunnel(18080, id, credential="wrong", expected=401))
            case("inventory-key-cannot-tunnel", lambda: tunnel(18080, id, credential=inventory_key, expected=403))
            case("invalid-port-zero", lambda: tunnel(0, id, expected=400))
            case("unavailable-guest-port", lambda: tunnel(18079, id, expected=502))
            case("node-cluster-token-required", lambda: tunnel(18082, id, target_context=node_context, target_port=node_port, credential="wrong", cluster=True, expected=401))

            def no_client_certificate():
                try:
                    tunnel(18082, id, target_context=context, target_port=node_port, credential=token, cluster=True)
                except (ssl.SSLError, ConnectionError, http.client.RemoteDisconnected):
                    return {"rejected_before_HTTP": True}
                raise RuntimeError("node accepted a client without mTLS identity")
            case("node-client-certificate-required", no_client_certificate)

            def direct_node_binary():
                stream = tunnel(18082, id, target_context=node_context, target_port=node_port, credential=token, cluster=True)
                try:
                    sample = payload[:65536]
                    stream.sendall(sample)
                    reply = b""
                    while len(reply) < len(sample):
                        chunk = stream.recv(len(sample) - len(reply))
                        if not chunk:
                            raise RuntimeError("node tunnel closed before reply")
                        reply += chunk
                    assert reply == sample
                    return {"bytes": len(sample), "sha256": hashlib.sha256(sample).hexdigest()}
                finally:
                    stream.close()
            case("direct-node-mTLS-binary", direct_node_binary)
            case("CLI-TLS-binary-client-EOF-multiple-connections", lambda: mirror(id, 2))
            case("CLI-graceful-interrupt-after-transfer-repeated", lambda: [mirror(id) for _ in range(12)])

            def reverse_eof():
                process, address = cli(id, 18081)
                try:
                    with socket.create_connection(address, timeout=10) as stream:
                        assert receive_all(stream) == b"ready\0\xff"
                        stream.sendall(payload)
                        stream.shutdown(socket.SHUT_WR)
                    expected = hashlib.sha256(payload).hexdigest()
                    deadline = time.monotonic() + 5
                    while True:
                        value = command(id, "sha256sum /tmp/tcp-reverse.bin 2>/dev/null || true").split()
                        if value and value[0] == expected:
                            break
                        if time.monotonic() > deadline:
                            raise RuntimeError("guest did not receive request after server EOF")
                        time.sleep(.02)
                    return {"received_request_bytes": len(payload), "guest_sha256": expected}
                finally:
                    process.send_signal(signal.SIGINT)
                    process.wait(timeout=5)
                    assert process.returncode == 0
            case("CLI-TLS-server-EOF-preserves-client-write", reverse_eof)

            def cli_missing_ca():
                output = subprocess.run([str(args.cli), "sandbox", "vm", "--endpoint", api_url, "tcp", id, "--port", "18080"],
                    env=dict(env, HV2_API_KEY=key), stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
                assert output.returncode == 1 and not output.stdout
                return {"exit_code": output.returncode, "TLS_trust_enforced": True}
            case("CLI-rejects-untrusted-API-certificate", cli_missing_ca)

            def idle_guard():
                response = api("POST", "/v2/sandboxes", {"templateID":"base","timeout":300,
                    "allowInternetAccess":False,"idleTimeout":30}, expected=201)
                idle_id = response["sandboxID"]
                guests.add(idle_id)
                command(idle_id,"/bin/tcp-fixture </dev/null >/tmp/tcp-fixture.log 2>&1 &")
                deadline = time.monotonic()+5
                while command(idle_id,"test -f /tmp/tcp-fixture-ready && printf ready || true") != "ready":
                    if time.monotonic()>deadline:raise TimeoutError("idle fixture did not start")
                    time.sleep(.01)
                process,address = cli(idle_id,18083)
                try:
                    with socket.create_connection(address,timeout=10) as stream:
                        stream.sendall(b"active tunnel across the idle window")
                        # No guest command or proxy request touches activity
                        # while the stream spans the complete idle window.
                        time.sleep(35)
                        detail = api("GET",f"/sandboxes/{idle_id}")
                        report["idle_probe"] = {"idle_window_seconds":30,"held_seconds":35,"observed_state":detail["state"]}
                        if detail["state"] != "running":
                            raise RuntimeError(f"open TCP tunnel was idle-paused: {detail['state']}")
                        stream.sendall(b"still open")
                        return {"idle_window_seconds":30,"held_seconds":35,"state":detail["state"]}
                finally:
                    process.send_signal(signal.SIGINT)
                    process.wait(timeout=5)
                    assert process.returncode==0
                    api("DELETE",f"/sandboxes/{idle_id}",expected=204)
                    guests.remove(idle_id)
            case("open-tunnel-prevents-automatic-idle-pause", idle_guard)

            def pause_resume():
                process, address = cli(id, 18083)
                try:
                    with socket.create_connection(address, timeout=10) as stream:
                        stream.sendall(b"held during pause")
                        api("POST", f"/sandboxes/{id}/pause", {}, expected=204)
                        assert receive_all(stream) == b""
                    tunnel(18080, id, expected=409)
                    api("POST", f"/sandboxes/{id}/resume", {"timeout": 300}, expected=201)
                    return mirror(id)
                finally:
                    process.send_signal(signal.SIGINT)
                    process.wait(timeout=5)
                    assert process.returncode == 0
            case("pause-closes-active-tunnel-resume-reopens-guest-port", pause_resume)

            def fork_port():
                response = api("POST", f"/sandboxes/{id}/fork", {"count": 1, "timeout": 300}, expected=201)
                forks = response
                assert len(forks) == 1
                child = forks[0]["sandbox"]["sandboxID"]
                guests.add(child)
                result = mirror(child)
                api("DELETE", f"/sandboxes/{child}", expected=204)
                guests.remove(child)
                return result
            case("fork-preserves-guest-listener", fork_port)

            def delete_closes():
                process, address = cli(id, 18083)
                try:
                    with socket.create_connection(address, timeout=10) as stream:
                        stream.sendall(b"held during delete")
                        api("DELETE", f"/sandboxes/{id}", expected=204)
                        guests.remove(id)
                        assert receive_all(stream) == b""
                    tunnel(18080, id, expected=404)
                    return {"active_connection_closed": True, "subsequent_handshake_status": 404}
                finally:
                    process.send_signal(signal.SIGINT)
                    process.wait(timeout=5)
                    assert process.returncode == 0
            case("delete-closes-active-tunnel-and-refuses-reopen", delete_closes)
            report["success"] = True
        except Exception as error:
            report["error"] = str(error)
        finally:
            for stream in sockets:
                try:
                    stream.close()
                except OSError:
                    pass
            if api_url and context:
                for id in list(guests):
                    try:
                        api("DELETE", f"/sandboxes/{id}", expected=204)
                        guests.remove(id)
                    except Exception as error:
                        report["cleanup_errors"].append(str(error))
                try:
                    report["remaining_sandboxes"] = len(api("GET", "/v2/sandboxes"))
                    assert report["remaining_sandboxes"] == 0
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            report["owned_processes_stopped"] = []
            for name, process in reversed(processes):
                try:
                    stop(process)
                    report["owned_processes_stopped"].append({"name": name, "exit_code": process.returncode})
                except Exception as error:
                    report["cleanup_errors"].append(f"{name}: {error}")
            for name, log in logs.items():
                log.close()
                raw = (directory / (name + ".log")).read_bytes()
                log_path = args.output.with_name(args.output.stem + "-" + name + ".log")
                log_path.write_bytes(raw)
            if report["cleanup_errors"]:
                report["success"] = False
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(report), flush=True)
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
