#!/usr/bin/env python3
"""Functional KVM TCP tunnel verification with owned Redis/TLS/CLI processes.

Requires a guest image containing tcp-fixture (tools/guest-image/tcp-fixture.c)
and the current static guest agent. Never reports performance scores.
"""
import argparse
import base64
import hashlib
import http.client
import http.server
import threading
import json
import os
from pathlib import Path
import selectors
import shlex
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


def terminal_check(argv, environment, register):
    """Exercise an actual local OpenSSH tty and guest terminal control events."""
    import fcntl
    import pty
    import struct
    import termios
    master, slave = pty.openpty()
    process = None
    output = bytearray()
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        process = subprocess.Popen(argv, env=environment, stdin=slave, stdout=slave,
                                   stderr=slave, start_new_session=True)
        register(process)
        os.close(slave)
        slave = None
        with selectors.DefaultSelector() as selector:
            selector.register(master, selectors.EVENT_READ)
            def until(marker):
                deadline = time.monotonic() + 15
                while marker not in output:
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f"terminal marker absent: {marker!r}; output: {bytes(output)!r}")
                    if not selector.select(min(.5, deadline - time.monotonic())):
                        continue
                    try:
                        chunk = os.read(master, 8192)
                    except OSError as error:
                        raise RuntimeError(f"terminal closed before {marker!r}: {bytes(output)!r}") from error
                    if not chunk:
                        raise RuntimeError(f"terminal EOF before {marker!r}: {bytes(output)!r}")
                    output.extend(chunk)
                    if len(output) > 128 * 1024:
                        raise RuntimeError("unexpectedly large terminal output")
            until(b"terminal-initial:24 80")
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 43, 132, 0, 0))
            # The fixture drives the same local event a window resize sends.
            process.send_signal(signal.SIGWINCH)
            until(b"terminal-resized:43 132")
            # OpenSSH places its local tty in raw mode; this byte reaches the
            # guest line discipline, which signals its foreground process.
            os.write(master, b"\x03")
            until(b"terminal-interrupt-ok")
        process.wait(timeout=10)
        assert process.returncode == 0, bytes(output)
        return {"initial_rows": 24, "initial_columns": 80, "resized_rows": 43,
                "resized_columns": 132, "guest_SIGWINCH": True, "guest_SIGINT": True,
                "exit_code": process.returncode, "output_utf8": output.decode(errors="replace")}
    finally:
        if slave is not None:
            os.close(slave)
        os.close(master)
        if process is not None:
            stop(process)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control-plane", "cli", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--ssh-fixture", type=Path,
                        help="build-ssh-fixture.py output; opt in to real OpenSSH guest checks")
    parser.add_argument("--ssh-by-name", action="store_true", help="resolve the SSH guest by its metadata label")
    parser.add_argument("--ssh-pty", action="store_true", help="verify guest PTY allocation and terminal input")
    parser.add_argument("--ssh-terminal", action="store_true", help="verify local/guest terminal resize and interrupt")
    parser.add_argument("--scheduled-dispatch", action="store_true", help="verify explicit scheduled VM dispatch and durable receipts")
    parser.add_argument("--scheduled-worker", action="store_true", help="verify automatic VM worker continuation and durable receipts")
    parser.add_argument("--scheduled-calendar", action="store_true", help="verify cron fold occurrences through the VM worker")
    parser.add_argument("--scheduled-calendar-batch", action="store_true", help="verify bounded calendar catch-up before VM dispatch")
    parser.add_argument("--node-response-loss", action="store_true", help="verify real control-plane response loss and timeout after node completion")
    parser.add_argument("--node-publication-fault", action="store_true", help="verify committed named guest survives a Redis event publication failure")
    parser.add_argument("--node-registration-fault", action="store_true", help="verify a registration write denial preserves the local guest and pending ownership")
    parser.add_argument("--node-registration-reconcile", action="store_true", help="reconcile the preserved guest with its original locally retained ownership")
    parser.add_argument("--node-name-operation", action="store_true", help="verify authenticated node completion and recovery without a creation descriptor")
    parser.add_argument("--reserved-create", action="store_true", help="verify name reservation during VM creation, duplicate refusal and reuse")
    parser.add_argument("--reserved-alias", action="store_true", help="assign a reserved alias with the CLI and verify named SSH without metadata")
    args = parser.parse_args()
    if args.node_response_loss and not args.node_name_operation:
        parser.error("--node-response-loss requires --node-name-operation")
    if args.node_publication_fault and not args.node_name_operation:
        parser.error("--node-publication-fault requires --node-name-operation")
    if args.node_registration_fault and not args.node_name_operation:
        parser.error("--node-registration-fault requires --node-name-operation")
    if args.node_registration_reconcile and not args.node_registration_fault:
        parser.error("--node-registration-reconcile requires --node-registration-fault")
    if args.node_name_operation and not args.reserved_create:
        parser.error("--node-name-operation requires --reserved-create")
    if args.reserved_create and (not (args.ssh_by_name and args.ssh_fixture) or args.reserved_alias):
        parser.error("--reserved-create requires --ssh-by-name and --ssh-fixture, without --reserved-alias")
    if args.reserved_alias and not (args.ssh_by_name and args.ssh_fixture):
        parser.error("--reserved-alias requires --ssh-by-name and --ssh-fixture")
    if args.scheduled_calendar_batch and not args.scheduled_calendar:
        parser.error("--scheduled-calendar-batch requires --scheduled-calendar")
    if args.scheduled_calendar and not args.scheduled_worker:
        parser.error("--scheduled-calendar requires --scheduled-worker")
    if args.ssh_by_name and not args.ssh_fixture:
        parser.error("--ssh-by-name requires --ssh-fixture")
    if args.ssh_pty and not args.ssh_fixture:
        parser.error("--ssh-pty requires --ssh-fixture")
    if args.ssh_terminal and not (args.ssh_fixture and args.ssh_pty):
        parser.error("--ssh-terminal requires --ssh-fixture and --ssh-pty")
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
            create_body = {"templateID": "base", "timeout": 300, "allowInternetAccess": False}
            ssh_name = "ssh-e2e-" + uuid.uuid4().hex
            if args.ssh_by_name and not args.reserved_alias:
                create_body["metadata"] = {"hm.name": ssh_name, "fixture.marker":"preserve-through-fork"}
            created = api("POST", "/v2/sandboxes", create_body, expected=201)
            id = created["sandboxID"]
            guests.add(id)
            info = api("GET", f"/sandboxes/{id}")
            assert info["cpuCount"] == 1 and info["memoryMB"] == 1024
            if args.reserved_alias or args.reserved_create:
                def alias_cli(*arguments, succeeds=True):
                    result = subprocess.run([str(args.cli), "sandbox", "vm", "--endpoint", api_url,
                        "--api-ca-cert", str(directory / "ca.pem"), "alias", *arguments],
                        env=dict(env, HV2_API_KEY=key), stdin=subprocess.DEVNULL,
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
                    assert (result.returncode == 0) == succeeds, result.stderr[:1000]
                    return json.loads(result.stdout) if succeeds else None
            if args.reserved_alias:
                assert "hm.name" not in info["metadata"]
                def reserved_alias():
                    expected = {"name":ssh_name,"sandboxID":id}
                    assert alias_cli("bind",id,ssh_name) == expected
                    assert alias_cli("bind",id,ssh_name) == expected
                    assert alias_cli("inspect",ssh_name) == expected
                    api("PUT",f"/sandboxes/{id}/names/{ssh_name}",{},supplied_key=inventory_key,expected=403)
                    return {"CLI_assignment_and_replay":True,"CLI_inspection":True,
                            "metadata_name_absent":True,"inventory_assignment_refused":True}
                case("reserved-alias-CLI-TLS-assignment-and-lookup",reserved_alias)
            elif args.reserved_create:
                def reserved_creation():
                    assert info["metadata"]["hm.name"] == ssh_name
                    expected = {"name":ssh_name,"sandboxID":id}
                    assert alias_cli("inspect",ssh_name) == expected
                    assert api("GET",f"/sandbox-names/{ssh_name}") == expected
                    for path in ["/sandboxes","/v2/sandboxes"]:
                        api("POST",path,create_body,expected=409)
                    api("POST","/v2/sandboxes",create_body,supplied_key=inventory_key,expected=403)
                    return {"creation_bound_name":True,"CLI_inspection":True,"both_create_routes_refuse_duplicate":True,"inventory_creation_refused":True}
                case("reserved-name-creation-and-duplicate-refusal",reserved_creation)
                if args.node_name_operation:
                    def node_request(body, operation=None, expected=201, discard_descriptor=False, cluster_credential=token):
                        connection = http.client.HTTPSConnection("127.0.0.1",node_port,context=node_context,timeout=90)
                        headers = {"content-type":"application/json","x-hv2-cluster-token":cluster_credential}
                        if operation is not None: headers["x-hv2-name-operation"] = operation
                        try:
                            connection.request("POST","/v2/sandboxes",body=json.dumps(body).encode(),headers=headers)
                            response=connection.getresponse()
                            assert response.status == expected, response.status
                            if discard_descriptor:
                                return None
                            raw=response.read()
                            if operation is not None: assert operation.encode() not in raw
                            return json.loads(raw) if raw else None
                        finally: connection.close()
                    def node_operation_refusal():
                        before=api("GET","/v2/sandboxes")
                        body={"templateID":"base","metadata":{"hm.name":"refused-"+uuid.uuid4().hex}}
                        node_request(body,expected=409)
                        node_request(body,"invalid-private-operation",expected=400)
                        node_request(body,str(uuid.uuid4()),expected=409)
                        node_request(body,str(uuid.uuid4()),expected=401,cluster_credential="wrong")
                        assert api("GET","/v2/sandboxes") == before
                        return {"missing_context_status":409,"invalid_context_status":400,
                                "unowned_context_status":409,"cluster_authentication_status":401,
                                "inventory_unchanged":True}
                    case("node-name-operation-refused-before-boot",node_operation_refusal)
            elif args.ssh_by_name:
                assert info["metadata"]["hm.name"] == ssh_name
            command(id, "/bin/tcp-fixture </dev/null >/tmp/tcp-fixture.log 2>&1 &")
            deadline = time.monotonic() + 5
            while command(id, "test -f /tmp/tcp-fixture-ready && printf ready || true") != "ready":
                if time.monotonic() > deadline:
                    raise TimeoutError("guest fixture did not start")
                time.sleep(.01)
            if args.scheduled_dispatch or args.scheduled_worker:
                def scheduled_dispatch():
                    job_store = directory / "scheduled-jobs"
                    profiles = directory / "job-profiles.json"
                    profiles.write_text(json.dumps({"profiles": {"local": {
                        "endpoint": api_url, "api_key_env": "HV2_API_KEY",
                        "ca": "ca.pem", "request_timeout_secs": 60}}}))
                    spec = directory / "vm-schedule.json"
                    marker = "literal'$(printf must-not-expand)"
                    script = "printf '%s\\n' \"$SCHEDULE_MARKER\" \"$HM_JOB_ID\"; printf x >> /tmp/scheduled-dispatch-count; exit 7"
                    times = (100, 110, 120)
                    recurrence = {"first_ms": 100, "every_ms": 10}
                    if args.scheduled_calendar:
                        from datetime import datetime, timezone
                        stamp = lambda day, hour: int(datetime(2025, 11, day, hour, 30, tzinfo=timezone.utc).timestamp() * 1000)
                        times = (stamp(2, 8), stamp(2, 9), stamp(3, 9))
                        assert int(time.time() * 1000) > times[2], "calendar fixture must be overdue"
                        recurrence = {"first_ms": times[0] - 1800000, "cron": {"expression": "30 1 * * *", "timezone": "America/Los_Angeles"}}
                    spec.write_text(json.dumps({**recurrence,
                        "vm": {"sandbox_id": id, "connection_profile": "local", "timeout_secs": 30},
                        "job": {"command": ["/bin/sh", "-c", script],
                                "env": {"SCHEDULE_MARKER": marker}, "workdir": "/tmp"}}))

                    def jobs(*arguments, succeeds=True):
                        result = subprocess.run([str(args.cli), "jobs", "--store", str(job_store), "schedule", *arguments],
                            env=dict(env, HV2_API_KEY=key), stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)
                        if (result.returncode == 0) != succeeds:
                            raise RuntimeError(f"schedule CLI status {result.returncode}: {result.stderr[:1000]!r}")
                        return json.loads(result.stdout) if succeeds and result.stdout.strip() else None

                    jobs("create", "guest-job", str(spec))
                    jobs("publish", "guest-job", "--now-ms", str(times[0]), "--limit", "1")
                    if args.scheduled_calendar_batch:
                        jobs("publish", "guest-job", "--now-ms", str(times[2]), "--limit", "2")
                        assert [row["scheduled_ms"] for row in jobs("occurrences", "guest-job")] == list(times)
                    api("POST", f"/sandboxes/{id}/pause", {}, expected=204)
                    result = jobs("worker", "guest-job", "--profiles", str(profiles), "--limit", "1", "--ticks", "1") if args.scheduled_worker else jobs("dispatch", "guest-job", str(times[0]), "--profiles", str(profiles))
                    assert result["exit_code"] == 7 and result["timed_out"] is False
                    assert result["stdout"] == marker + f"\nguest-job--{times[0]}\n"
                    assert command(id, "cat /tmp/scheduled-dispatch-count") == "x"
                    receipt = jobs("receipt", "guest-job", str(times[0]))["completion"]
                    assert receipt["stdout"] == result["stdout"] and receipt["exit_code"] == 7
                    assert receipt["stdout_truncated"] is False
                    if args.scheduled_calendar:
                        assert receipt["origin"] == "api_response"
                        assert jobs("status", "guest-job")["schedule"]["cron"]["tzdb_version"]
                        assert times[1] - times[0] == 3600000
                    jobs("dispatch", "guest-job", str(times[0]), "--profiles", str(profiles), succeeds=False)
                    assert command(id, "cat /tmp/scheduled-dispatch-count") == "x"
                    if args.scheduled_worker:
                        continued = jobs("worker", "guest-job", "--profiles", str(profiles), "--limit", "1", "--ticks", "1")
                        assert continued["scheduled_ms"] == times[1] and continued["exit_code"] == 7
                        assert continued["stdout"] == marker + f"\nguest-job--{times[1]}\n"
                        assert command(id, "cat /tmp/scheduled-dispatch-count") == "xx"
                        assert jobs("receipt", "guest-job", str(times[1]))["completion"]["stdout"] == continued["stdout"]
                    if args.scheduled_worker:
                        occurrences = jobs("occurrences", "guest-job")
                        if args.scheduled_calendar_batch:
                            assert [row["scheduled_ms"] for row in occurrences[:3]] == list(times)
                            assert len(occurrences) == 5
                        else:
                            assert occurrences[-1]["scheduled_ms"] == times[2]
                    jobs("cancel", "guest-job")
                    jobs("publish", "guest-job", "--now-ms", str(times[1]), succeeds=False)
                    assert jobs("receipt", "guest-job", str(times[0]))["completion"] == receipt
                    if args.scheduled_worker:
                        assert jobs("worker", "guest-job", "--profiles", str(profiles), "--limit", "1", "--ticks", "1") is None
                        assert command(id, "cat /tmp/scheduled-dispatch-count") == "xx"
                        assert jobs("receipt", "guest-job", str(times[2]), succeeds=False) is None
                    return {"paused_guest_resumed": True, "guest_exit_code": 7,
                            "literal_environment_preserved": True, "durable_output_recovered": True,
                            "duplicate_guest_execution_refused": True, "history_survives_cancellation": True, "automatic_worker": args.scheduled_worker,
                            "restart_continues_next_occurrence": args.scheduled_worker,
                            "cancelled_worker_leaves_pending_work_untouched": args.scheduled_worker,
                            "calendar_fold_distinct_utc_occurrences": args.scheduled_calendar,
                            "bounded_calendar_batch_verified": args.scheduled_calendar_batch,
                            "scheduled_utc_ms": list(times) if args.scheduled_calendar else None}
                case("scheduled-calendar-VM-worker-TLS-fold-resume-restart-and-no-replay" if args.scheduled_calendar else "scheduled-VM-worker-TLS-resume-restart-and-no-replay" if args.scheduled_worker else "scheduled-VM-dispatch-TLS-resume-receipt-and-no-replay", scheduled_dispatch)

            if args.ssh_fixture:
                fixture = args.ssh_fixture
                build = json.loads((fixture / "build.json").read_text())
                assert build["success"] and digest(args.initrd) == build["image_sha256"]
                assert digest(fixture / "client-key.pub") == build["client_public_key_sha256"]
                trusted_public = (fixture / "host-key.pub").read_text().strip()
                assert digest(fixture / "host-key.pub") == build["host_public_key_sha256"]
                assert command(id, "cat /etc/dropbear/fixture-key.pub").strip() == trusted_public
                command(id, "/usr/sbin/dropbear -F -E -s -j -k -r /etc/dropbear/fixture-key -p 127.0.0.1:22 -P /var/run/ssh-fixture.pid </dev/null >/tmp/ssh-fixture.log 2>&1 &")
                known_hosts = directory / "ssh-known-hosts"
                known_hosts.write_text("hm-ssh-fixture " + trusted_public + "\n")
                target = ["--name", ssh_name] if args.ssh_by_name else [id]
                proxy = shlex.join([str(args.cli), "sandbox", "vm", "--endpoint", api_url,
                    "--api-ca-cert", str(directory / "ca.pem"), "tcp-stdio", *target, "--port", "22"])
                def ssh_argv(remote_command, identity=None, known=None, pty=False, sandbox_name=None):
                    selected_proxy = proxy if sandbox_name is None else shlex.join([str(args.cli),"sandbox","vm","--endpoint",api_url,
                        "--api-ca-cert",str(directory/"ca.pem"),"tcp-stdio","--name",sandbox_name,"--port","22"])
                    return ["ssh", "-F", "/dev/null", "-tt" if pty else "-T", "-o", "BatchMode=yes",
                        "-o", "IdentitiesOnly=yes", "-o", "IdentityAgent=none", "-o", "StrictHostKeyChecking=yes",
                        "-o", "GlobalKnownHostsFile=/dev/null", "-o", "HostKeyAlias=hm-ssh-fixture",
                        "-o", "UserKnownHostsFile=" + str(known or known_hosts),
                        "-o", "ProxyCommand=" + selected_proxy, "-o", "ConnectTimeout=10",
                        "-i", str(identity or fixture / "client-key"), "root@" + (sandbox_name or id), remote_command]
                def ssh(remote_command, input_bytes=b"", identity=None, known=None, pty=False, sandbox_name=None):
                    return subprocess.run(ssh_argv(remote_command, identity, known, pty, sandbox_name),
                        input=input_bytes, env=dict(env, HV2_API_KEY=key), stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, timeout=30)
                deadline = time.monotonic() + 10
                while True:
                    probe = ssh("printf ssh-ready")
                    if probe.returncode == 0 and probe.stdout == b"ssh-ready":
                        break
                    if time.monotonic() > deadline:
                        raise RuntimeError("guest SSH did not start: " + probe.stderr.decode(errors="replace"))
                    time.sleep(.05)
                report["ssh_fixture"] = {"build_sha256": digest(fixture / "build.json"),
                    "resolution": "reserved alias" if args.reserved_alias else "reserved creation" if args.reserved_create else "hm.name metadata" if args.ssh_by_name else "sandbox ID",
                    "server_version": build["server_version"], "host_public_key_sha256": build["host_public_key_sha256"],
                    "client_public_key_sha256": build["client_public_key_sha256"],
                    "openssh_version": subprocess.check_output(["ssh", "-V"], stderr=subprocess.STDOUT, text=True).strip()}
                if args.node_name_operation:
                    def recover_discarded_descriptor():
                        recovered_name="recovered-"+uuid.uuid4().hex
                        operation=str(uuid.uuid4())
                        pending={"name":recovered_name,"token":operation,"sandbox_id":None}
                        subprocess.run(["redis-cli","-p",str(redis_port),"HSET",f"hv2:{namespace}:name-reservations",recovered_name,json.dumps(pending)],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                        body={"templateID":"base","timeout":300,"metadata":{"hm.name":recovered_name}}
                        node_request(body,operation,discard_descriptor=True)
                        resolved=alias_cli("inspect",recovered_name)
                        recovered_id=resolved["sandboxID"]
                        guests.add(recovered_id)
                        assert resolved == {"name":recovered_name,"sandboxID":recovered_id}
                        assert api("GET",f"/sandbox-names/{recovered_name}") == resolved
                        api("POST","/v2/sandboxes",body,expected=409)
                        assert operation not in command(recovered_id,"env")
                        command(recovered_id,"/usr/sbin/dropbear -F -E -s -j -k -r /etc/dropbear/fixture-key -p 127.0.0.1:22 -P /var/run/ssh-fixture.pid </dev/null >/tmp/ssh-fixture.log 2>&1 &")
                        deadline=time.monotonic()+10
                        while True:
                            probe=ssh("printf recovered-guest",sandbox_name=recovered_name)
                            if probe.returncode == 0 and probe.stdout == b"recovered-guest":break
                            if time.monotonic()>deadline:raise RuntimeError(probe.stderr.decode(errors="replace"))
                            time.sleep(.05)
                        api("DELETE",f"/sandboxes/{recovered_id}",expected=204)
                        guests.remove(recovered_id)
                        api("GET",f"/sandbox-names/{recovered_name}",expected=404)
                        return {"creation_descriptor_discarded":True,"CLI_inspection_recovers_ID":True,
                                "named_SSH_reaches_recovered_guest":True,"duplicate_creation_refused":True,
                                "operation_absent_from_guest_environment":True,"deleted_name_released":True}
                    case("node-atomic-completion-with-discarded-descriptor",recover_discarded_descriptor)
                if args.node_response_loss:
                    def response_loss(mode):
                        fault_name="fault-"+mode+"-"+uuid.uuid4().hex
                        release=threading.Event()
                        backend_completed=threading.Event()
                        handler_finished=threading.Event()
                        outcomes={"backend_requests":0}
                        node_process=next(process for name,process in processes if name=="node")
                        class Relay(http.server.BaseHTTPRequestHandler):
                            protocol_version="HTTP/1.1"
                            def log_message(self,*arguments):pass
                            def do_POST(self):
                                node_process.send_signal(signal.SIGCONT)
                                backend=None
                                try:
                                    assert self.path == "/v2/sandboxes"
                                    size=int(self.headers["content-length"])
                                    assert 0 < size <= 65536
                                    payload=self.rfile.read(size)
                                    assert json.loads(payload)["metadata"]["hm.name"] == fault_name
                                    outcomes["backend_requests"]+=1
                                    backend=http.client.HTTPSConnection("127.0.0.1",node_port,context=node_context,timeout=90)
                                    backend.request("POST",self.path,body=payload,headers={
                                        "content-type":"application/json",
                                        "x-hv2-cluster-token":self.headers["x-hv2-cluster-token"],
                                        "x-hv2-name-operation":self.headers["x-hv2-name-operation"]})
                                    response=backend.getresponse()
                                    assert response.status == 201, response.status
                                    response.read()  # Discard the committed node's descriptor.
                                    backend_completed.set()
                                    if mode == "timeout":release.wait(timeout=75)
                                    try:self.connection.shutdown(socket.SHUT_RDWR)
                                    except OSError:pass
                                    self.close_connection=True
                                except Exception as error:
                                    outcomes["relay_error"]=str(error)
                                    self.close_connection=True
                                finally:
                                    if backend is not None:backend.close()
                                    handler_finished.set()
                        class Server(http.server.ThreadingHTTPServer):
                            daemon_threads=False
                        server=Server(("127.0.0.1",0),Relay)
                        tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                        tls.load_cert_chain(str(directory/"node.pem"),str(directory/"node.key"))
                        tls.load_verify_locations(cafile=str(directory/"ca.pem"))
                        tls.verify_mode=ssl.CERT_REQUIRED
                        tls.set_alpn_protocols(["http/1.1"])
                        server.socket=tls.wrap_socket(server.socket,server_side=True)
                        listener=threading.Thread(target=lambda:server.serve_forever(poll_interval=.05))
                        listener.start()
                        def advertise(url):
                            script="local value=redis.call('GET',KEYS[1]); if not value then return 0 end; local node=cjson.decode(value); node.api=ARGV[1]; redis.call('SET',KEYS[1],cjson.encode(node),'KEEPTTL'); return 1"
                            result=subprocess.check_output(["redis-cli","-p",str(redis_port),"EVAL",script,"1",f"hv2:{namespace}:node:tcp-kvm-node",url])
                            assert result.strip() == b"1", result
                        recovered_id=None
                        result=None
                        try:
                            # Freeze only the owned node until the relay receives
                            # the request, so a heartbeat cannot undo scheduling.
                            node_process.send_signal(signal.SIGSTOP)
                            advertise(f"https://127.0.0.1:{server.server_address[1]}")
                            body={"templateID":"base","timeout":300,"metadata":{"hm.name":fault_name}}
                            started=time.monotonic()
                            error=api("POST","/v2/sandboxes",body,expected=502)
                            elapsed=time.monotonic()-started
                            assert error["code"] == 502
                            assert backend_completed.is_set() and "relay_error" not in outcomes,outcomes
                            assert outcomes["backend_requests"] == 1
                            if mode == "timeout":assert 59 <= elapsed < 85,elapsed
                            resolved=alias_cli("inspect",fault_name)
                            recovered_id=resolved["sandboxID"]
                            guests.add(recovered_id)
                            assert api("GET",f"/sandbox-names/{fault_name}") == resolved
                            api("POST","/v2/sandboxes",body,expected=409)
                            assert {row["sandboxID"] for row in api("GET","/v2/sandboxes")} == {id,recovered_id}
                            command(recovered_id,"/usr/sbin/dropbear -F -E -s -j -k -r /etc/dropbear/fixture-key -p 127.0.0.1:22 -P /var/run/ssh-fixture.pid </dev/null >/tmp/ssh-fixture.log 2>&1 &")
                            deadline=time.monotonic()+10
                            while True:
                                probe=ssh("printf recovered-after-loss",sandbox_name=fault_name)
                                if probe.returncode==0 and probe.stdout==b"recovered-after-loss":break
                                if time.monotonic()>deadline:raise RuntimeError(probe.stderr.decode(errors="replace"))
                                time.sleep(.05)
                            api("DELETE",f"/sandboxes/{recovered_id}",expected=204)
                            guests.remove(recovered_id)
                            api("GET",f"/sandbox-names/{fault_name}",expected=404)
                            result={"control_plane_status":502,"node_committed_before_response_loss":True,
                                    "backend_requests":outcomes["backend_requests"],"elapsed_seconds":elapsed,
                                    "CLI_recovers_ID":True,"named_SSH_reaches_same_guest":True,
                                    "duplicate_creation_refused":True,"no_extra_VM":True,"deleted_name_released":True}
                        finally:
                            node_process.send_signal(signal.SIGCONT)
                            release.set()
                            try:advertise(f"https://127.0.0.1:{node_port}")
                            finally:
                                server.shutdown()
                                server.server_close()
                                listener.join(timeout=5)
                                assert not listener.is_alive()
                            if backend_completed.is_set() and recovered_id is None:
                                resolved=api("GET",f"/sandbox-names/{fault_name}")
                                guests.add(resolved["sandboxID"])
                        assert handler_finished.is_set()
                        result["relay_listener_and_handler_stopped"]=True
                        return result
                    case("control-plane-node-response-drop-after-commit",lambda:response_loss("drop"))
                    case("control-plane-node-response-timeout-after-commit",lambda:response_loss("timeout"))
                if args.node_publication_fault:
                    def publication_fault():
                        fault_name="publication-"+uuid.uuid4().hex
                        body={"templateID":"base","timeout":300,"metadata":{"hm.name":fault_name}}
                        def publication_permission(rule):
                            result=subprocess.check_output(["redis-cli","-p",str(redis_port),"ACL","SETUSER","default",rule])
                            assert result.strip()==b"OK",result
                        recovered_id=None
                        try:
                            publication_permission("-xadd")
                            error=api("POST","/v2/sandboxes",body,expected=503)
                            assert error["code"]==503
                            resolved=alias_cli("inspect",fault_name)
                            recovered_id=resolved["sandboxID"]
                            guests.add(recovered_id)
                            assert api("GET",f"/sandbox-names/{fault_name}")==resolved
                            assert {row["sandboxID"] for row in api("GET","/v2/sandboxes")}=={id,recovered_id}
                            api("POST","/v2/sandboxes",body,expected=409)
                            command(recovered_id,"/usr/sbin/dropbear -F -E -s -j -k -r /etc/dropbear/fixture-key -p 127.0.0.1:22 -P /var/run/ssh-fixture.pid </dev/null >/tmp/ssh-fixture.log 2>&1 &")
                            deadline=time.monotonic()+10
                            while True:
                                probe=ssh("printf publication-survivor",sandbox_name=fault_name)
                                if probe.returncode==0 and probe.stdout==b"publication-survivor":break
                                if time.monotonic()>deadline:raise RuntimeError(probe.stderr.decode(errors="replace"))
                                time.sleep(.05)
                            acl=json.loads(subprocess.check_output(["redis-cli","-p",str(redis_port),"--json","ACL","LOG","10"]))
                            assert any(row.get("object")=="xadd" and row.get("reason")=="command" for row in acl),acl
                        finally:
                            publication_permission("+xadd")
                            if recovered_id is None:
                                recovered=api("GET",f"/sandbox-names/{fault_name}",expected=200)
                                guests.add(recovered["sandboxID"])
                        api("DELETE",f"/sandboxes/{recovered_id}",expected=204)
                        guests.remove(recovered_id)
                        api("GET",f"/sandbox-names/{fault_name}",expected=404)
                        return {"control_plane_status":503,"redis_xadd_denial_observed":True,
                                "committed_VM_preserved":True,"CLI_recovers_ID":True,
                                "named_SSH_reaches_same_guest":True,"duplicate_creation_refused":True,
                                "no_extra_VM":True,"publication_permission_restored":True,"deleted_name_released":True}
                    case("node-post-commit-event-publication-fault",publication_fault)
                if args.node_registration_fault:
                    def registration_fault():
                        fault_name="registration-"+uuid.uuid4().hex
                        body={"templateID":"base","timeout":300,"metadata":{"hm.name":fault_name}}
                        if args.node_registration_reconcile:body["idleTimeout"]=30
                        def direct(method,path,body=None,expected=200,cluster_credential=token):
                            connection=http.client.HTTPSConnection("127.0.0.1",node_port,context=node_context,timeout=90)
                            try:
                                connection.request(method,path,body=None if body is None else json.dumps(body).encode(),
                                    headers={"content-type":"application/json","x-hv2-cluster-token":cluster_credential})
                                response=connection.getresponse()
                                raw=response.read()
                                assert response.status==expected,(response.status,raw)
                                return json.loads(raw) if raw else None
                            finally:connection.close()
                        def permission(rule):
                            result=subprocess.check_output(["redis-cli","-p",str(redis_port),"ACL","SETUSER","default",rule])
                            assert result.strip()==b"OK",result
                        local_id=None
                        try:
                            permission("-sadd")
                            error=api("POST","/v2/sandboxes",body,expected=503)
                            assert error["code"]==503
                            permission("+sadd")
                            local=direct("GET","/v2/sandboxes")
                            matching=[row for row in local if row.get("metadata",{}).get("hm.name")==fault_name]
                            assert len(matching)==1,local
                            local_id=matching[0]["sandboxID"]
                            assert {row["sandboxID"] for row in local}=={id,local_id}
                            assert {row["sandboxID"] for row in api("GET","/v2/sandboxes")}=={id}
                            api("GET",f"/sandbox-names/{fault_name}",expected=409)
                            api("POST","/v2/sandboxes",body,expected=409)
                            reply=direct("POST",f"/sandboxes/{local_id}/exec",{"cmd":"printf preserved-before-registration","timeout_secs":10})
                            assert reply["exit_code"]==0 and reply["stdout"]=="preserved-before-registration",reply
                            record=subprocess.check_output(["redis-cli","-p",str(redis_port),"EXISTS",f"hv2:{namespace}:sandbox:{local_id}"])
                            indexed=subprocess.check_output(["redis-cli","-p",str(redis_port),"SISMEMBER",f"hv2:{namespace}:sandboxes",local_id])
                            names=subprocess.check_output(["redis-cli","-p",str(redis_port),"SCARD",f"hv2:{namespace}:reserved-names:{local_id}"])
                            assert record.strip()==indexed.strip()==names.strip()==b"0"
                            pending=json.loads(subprocess.check_output(["redis-cli","-p",str(redis_port),"HGET",f"hv2:{namespace}:name-reservations",fault_name]))
                            assert pending["name"]==fault_name and pending["sandbox_id"] is None
                            assert {row["sandboxID"] for row in direct("GET","/v2/sandboxes")}=={id,local_id}
                            if args.node_registration_reconcile:
                                connected=direct("POST",f"/sandboxes/{local_id}/connect",{})
                                assert connected["sandboxID"]==local_id
                                direct("POST",f"/sandboxes/{local_id}/connect",{"timeout":600},expected=409)
                                direct("POST",f"/sandboxes/{local_id}/checkpoints",{"name":"uncertain-registration"},expected=201)
                                direct("POST",f"/sandboxes/{local_id}/checkpoints/uncertain-registration/restore",expected=409)
                                time.sleep(35)  # Cross the configured 30-second idle window.
                                idle_rows=[row for row in direct("GET","/v2/sandboxes") if row["sandboxID"]==local_id]
                                assert len(idle_rows)==1 and idle_rows[0]["state"]=="running",idle_rows
                                still_live=direct("POST",f"/sandboxes/{local_id}/exec",{"cmd":"printf pending-idle-survivor","timeout_secs":10})
                                assert still_live["exit_code"]==0 and still_live["stdout"]=="pending-idle-survivor"
                                direct("POST",f"/sandboxes/{local_id}/pause",{},expected=409)
                                direct("POST",f"/sandboxes/{local_id}/fork",{"count":1},expected=409)
                                direct("POST",f"/sandboxes/{local_id}/timeout",{"timeout":300},expected=409)
                                path=f"/sandboxes/{local_id}/registration/reconcile"
                                direct("POST",path,expected=401,cluster_credential="wrong")
                                replacement=dict(pending,token=str(uuid.uuid4()))
                                def install(value):
                                    subprocess.run(["redis-cli","-p",str(redis_port),"HSET",f"hv2:{namespace}:name-reservations",fault_name,json.dumps(value)],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
                                try:
                                    install(replacement)
                                    direct("POST",path,expected=409)
                                    assert {row["sandboxID"] for row in direct("GET","/v2/sandboxes")}=={id,local_id}
                                    assert {row["sandboxID"] for row in api("GET","/v2/sandboxes")}=={id}
                                finally:install(pending)
                                reconciled=direct("POST",path)
                                assert reconciled["sandboxID"]==local_id
                                assert pending["token"] not in json.dumps(reconciled)
                                assert alias_cli("inspect",fault_name)=={"name":fault_name,"sandboxID":local_id}
                                assert {row["sandboxID"] for row in api("GET","/v2/sandboxes")}=={id,local_id}
                                api("POST","/v2/sandboxes",body,expected=409)
                                direct("POST",path,expected=409)
                                direct("POST",f"/sandboxes/{local_id}/timeout",{"timeout":300},expected=204)
                                extended=direct("POST",f"/sandboxes/{local_id}/connect",{"timeout":600})
                                assert extended["sandboxID"]==local_id
                                restored=direct("POST",f"/sandboxes/{local_id}/checkpoints/uncertain-registration/restore")
                                assert restored["sandboxID"]==local_id
                                direct("POST",f"/sandboxes/{local_id}/pause",{},expected=204)
                                resumed=direct("POST",f"/sandboxes/{local_id}/resume",{"timeout":300},expected=201)
                                assert resumed["sandboxID"]==local_id
                                assert alias_cli("inspect",fault_name)=={"name":fault_name,"sandboxID":local_id}
                                forked=direct("POST",f"/sandboxes/{local_id}/fork",{"count":1,"timeout":300},expected=201)
                                assert len(forked)==1
                                child_id=forked[0]["sandbox"]["sandboxID"]
                                guests.add(child_id)
                                try:
                                    child=api("GET",f"/sandboxes/{child_id}")
                                    assert "hm.name" not in child.get("metadata",{})
                                    assert alias_cli("inspect",fault_name)=={"name":fault_name,"sandboxID":local_id}
                                finally:
                                    direct("DELETE",f"/sandboxes/{child_id}",expected=204)
                                    guests.remove(child_id)
                                assert pending["token"] not in command(local_id,"env")
                                command(local_id,"/usr/sbin/dropbear -F -E -s -j -k -r /etc/dropbear/fixture-key -p 127.0.0.1:22 -P /var/run/ssh-fixture.pid </dev/null >/tmp/ssh-fixture.log 2>&1 &")
                                deadline=time.monotonic()+10
                                while True:
                                    probe=ssh("printf reconciled-same-guest",sandbox_name=fault_name)
                                    if probe.returncode==0 and probe.stdout==b"reconciled-same-guest":break
                                    if time.monotonic()>deadline:raise RuntimeError(probe.stderr.decode(errors="replace"))
                                    time.sleep(.05)
                        finally:
                            permission("+sadd")
                            if local_id is None:
                                matching=[row for row in direct("GET","/v2/sandboxes") if row.get("metadata",{}).get("hm.name")==fault_name]
                                if matching:local_id=matching[0]["sandboxID"]
                            if local_id is not None:direct("DELETE",f"/sandboxes/{local_id}",expected=204)
                        assert {row["sandboxID"] for row in direct("GET","/v2/sandboxes")}=={id}
                        if args.node_registration_reconcile:
                            api("GET",f"/sandbox-names/{fault_name}",expected=404)
                            return {"control_plane_status":503,"local_guest_preserved_and_executable":True,
                                    "partial_registration_writes_absent":True,"wrong_cluster_credential_refused":True,
                                    "uncertain_pause_fork_timeout_refused":True,"timeout_available_after_reconciliation":True,
                                    "uncertain_read_only_connect_and_checkpoint_creation_allowed":True,
                                    "uncertain_connect_extension_and_checkpoint_restore_refused":True,
                                    "connect_extension_and_checkpoint_restore_available_after_reconciliation":True,
                                    "uncertain_guest_survives_idle_window_seconds":35,
                                    "pause_resume_fork_available_after_reconciliation":True,
                                    "replacement_owner_refused_without_guest_loss":True,"original_owner_reconciles_same_VM":True,
                                    "CLI_and_named_SSH_recover_same_guest":True,"duplicate_creation_refused":True,
                                    "reconciliation_context_cleared_after_success":True,"operation_not_exposed":True,
                                    "local_guest_deleted_and_bound_name_released":True}
                        api("GET",f"/sandbox-names/{fault_name}",expected=409)
                        api("POST","/v2/sandboxes",body,expected=409)
                        return {"control_plane_status":503,"local_guest_preserved_and_executable":True,
                                "VM_record_absent":True,"global_and_name_indexes_absent":True,
                                "pending_ownership_preserved":True,"duplicate_creation_refused":True,
                                "no_extra_local_VM":True,"write_permission_restored":True,
                                "local_guest_deleted":True,"pending_not_blindly_released_after_delete":True}
                    case("node-registration-fault-same-guest-reconciliation" if args.node_registration_reconcile else "node-registration-write-permission-fault",registration_fault)
                def ssh_binary():
                    sample = bytes(index % 251 for index in range(1024 * 1024))
                    result = ssh("cat > /tmp/ssh-transfer.bin; cat /tmp/ssh-transfer.bin", sample)
                    assert result.returncode == 0, result.stderr.decode(errors="replace")
                    assert result.stdout == sample
                    expected = hashlib.sha256(sample).hexdigest()
                    assert command(id, "sha256sum /tmp/ssh-transfer.bin").split()[0] == expected
                    return {"bytes": len(sample), "sha256": expected, "strict_host_key_checking": True}
                case("OpenSSH-API-TLS-node-mTLS-binary-roundtrip", ssh_binary)
                def ssh_exit():
                    result = ssh("exit 7")
                    assert result.returncode == 7 and not result.stdout
                    return {"exit_code": result.returncode}
                case("OpenSSH-preserves-remote-exit-code", ssh_exit)
                if args.ssh_pty:
                    def ssh_pty():
                        # -tt forces allocation despite piped local stdin.
                        # Disable guest echo before reading so only the marker
                        # is expected; initial line echo can race stty setup.
                        marker = "hm-pty-" + uuid.uuid4().hex
                        result = ssh("test -t 0 && test -t 1 && stty -echo && "
                            "IFS= read -r line && test \"$line\" = " + shlex.quote(marker) +
                            " && printf 'pty-input-ok\\n'", (marker + "\n").encode(), pty=True)
                        assert result.returncode == 0, result.stderr.decode(errors="replace")
                        assert b"pty-input-ok\r\n" in result.stdout, result.stdout
                        # The guest program tests both terminal fds and exact
                        # line contents before emitting its success marker.
                        return {"stdin_is_terminal": True, "stdout_is_terminal": True,
                            "canonical_input_verified": True, "terminal_newline": "CRLF"}
                    case("OpenSSH-guest-PTY-and-canonical-input", ssh_pty)
                if args.ssh_terminal:
                    def ssh_terminal():
                        remote = "stty -echo; " \
                            "trap 'printf \"terminal-resized:%s\\n\" \"$(stty size)\"' WINCH; " \
                            "trap 'printf \"terminal-interrupt-ok\\n\"; exit 0' INT; " \
                            "printf 'terminal-initial:%s\\n' \"$(stty size)\"; " \
                            "while :; do IFS= read -r line || :; done"
                        return terminal_check(ssh_argv(remote, pty=True), dict(env, HV2_API_KEY=key),
                            lambda process: processes.append(("ssh-terminal", process)))
                    case("OpenSSH-terminal-size-SIGWINCH-and-Ctrl-C", ssh_terminal)
                run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(directory / "wrong-ssh-key")])
                def ssh_bad_identity():
                    result = ssh("printf should-not-run", identity=directory / "wrong-ssh-key")
                    assert result.returncode == 255 and not result.stdout
                    assert b"Permission denied" in result.stderr, result.stderr
                    return {"exit_code": result.returncode, "authentication_rejected": True}
                case("OpenSSH-rejects-unregistered-client-key", ssh_bad_identity)
                wrong_known = directory / "ssh-wrong-known-hosts"
                wrong_known.write_text("hm-ssh-fixture " + (directory / "wrong-ssh-key.pub").read_text())
                def ssh_bad_host():
                    result = ssh("printf should-not-run", known=wrong_known)
                    assert result.returncode == 255 and not result.stdout
                    assert b"Host key verification failed" in result.stderr, result.stderr
                    return {"exit_code": result.returncode, "host_identity_rejected": True}
                case("OpenSSH-rejects-mismatched-guest-host-key", ssh_bad_host)
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
                if args.reserved_alias or args.reserved_create:
                    api("PUT",f"/sandboxes/{child}/names/{ssh_name}",{},expected=409)
                    kept = ssh("printf reserved-parent")
                    assert kept.returncode == 0 and kept.stdout == b"reserved-parent"
                    assert alias_cli("inspect",ssh_name) == {"name":ssh_name,"sandboxID":id}
                    assert "hm.name" not in api("GET",f"/sandboxes/{child}")["metadata"]
                    result["alias_transfer_refused"] = True
                    result["reserved_name_keeps_parent_after_fork"] = True
                    result["child_name_not_inherited"] = True
                    if args.reserved_alias:
                        result["alias_keeps_parent_after_fork"] = True
                    else:
                        parent_metadata = api("GET",f"/sandboxes/{id}")["metadata"]
                        child_metadata = api("GET",f"/sandboxes/{child}")["metadata"]
                        assert parent_metadata["hm.name"] == ssh_name
                        assert parent_metadata["fixture.marker"] == child_metadata["fixture.marker"] == "preserve-through-fork"
                        result["parent_metadata_name_unchanged"] = True
                        result["other_metadata_preserved"] = True
                elif args.ssh_by_name:
                    kept = ssh("printf named-parent")
                    assert kept.returncode == 0 and kept.stdout == b"named-parent"
                    assert "hm.name" not in api("GET",f"/sandboxes/{child}")["metadata"]
                    result["child_name_not_inherited"] = True
                api("DELETE", f"/sandboxes/{child}", expected=204)
                guests.remove(child)
                if args.ssh_by_name:
                    restored = ssh("printf name-resolves-again")
                    assert restored.returncode == 0 and restored.stdout == b"name-resolves-again"
                    result["name_resolves_after_duplicate_deleted"] = True
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
                    result = {"active_connection_closed": True, "subsequent_handshake_status": 404}
                    if args.reserved_alias:
                        alias_cli("inspect",ssh_name,succeeds=False)
                        replacement = api("POST","/v2/sandboxes",{"templateID":"base","timeout":300},expected=201)["sandboxID"]
                        guests.add(replacement)
                        assert alias_cli("bind",replacement,ssh_name) == {"name":ssh_name,"sandboxID":replacement}
                        assert alias_cli("inspect",ssh_name) == {"name":ssh_name,"sandboxID":replacement}
                        api("DELETE",f"/sandboxes/{replacement}",expected=204)
                        guests.remove(replacement)
                        alias_cli("inspect",ssh_name,succeeds=False)
                        result["alias_deleted_and_reused"] = True
                    elif args.reserved_create:
                        api("GET",f"/sandbox-names/{ssh_name}",expected=404)
                        alias_cli("inspect",ssh_name,succeeds=False)
                        replacement = api("POST","/v2/sandboxes",create_body,expected=201)["sandboxID"]
                        guests.add(replacement)
                        assert replacement != id
                        assert alias_cli("inspect",ssh_name) == {"name":ssh_name,"sandboxID":replacement}
                        assert api("GET",f"/sandboxes/{replacement}")["metadata"]["hm.name"] == ssh_name
                        api("DELETE",f"/sandboxes/{replacement}",expected=204)
                        guests.remove(replacement)
                        api("GET",f"/sandbox-names/{ssh_name}",expected=404)
                        result["creation_name_deleted_and_reused"] = True
                    return result
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
