#!/usr/bin/env python3
"""Compare native TCP forwarding transactions on matched, already-ready guests.

Requires Linux KVM and an initrd containing the explicit tcp-fixture service.
HyperMachine uses its authenticated HTTP upgrade; Firecracker uses its native
Unix vsock socket and the same guest Forward operation. No CLI, TLS, cluster,
managed competitor or sustained fleet capacity is measured. Optional concurrent
streams are synchronized client bursts, not sustained-arrival capacity.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import uuid

spec = importlib.util.spec_from_file_location("engines", Path(__file__).with_name("bench-local-engines.py"))
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
fc = engines.fc


def line(stream, limit):
    data = bytearray()
    while len(data) < limit:
        part = stream.recv(1)
        if not part:
            raise RuntimeError("connection closed during handshake")
        data.extend(part)
        if part == b"\n":
            return bytes(data)
    raise RuntimeError("handshake line exceeded limit")


def hm_forward(address, sandbox, key, port=18082):
    stream = socket.create_connection(address, timeout=10)
    try:
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        request = (f"GET /sandboxes/{sandbox}/ports/{port}/tcp HTTP/1.1\r\n"
                   f"Host: {address[0]}:{address[1]}\r\nConnection: Upgrade\r\n"
                   f"Upgrade: hv2-tcp/1\r\nx-hv2-cluster-token: {key}\r\n\r\n")
        stream.sendall(request.encode("ascii"))
        status = line(stream, 256)
        if status.split()[1:2] != [b"101"]:
            raise RuntimeError(f"node refused TCP handshake: {status!r}")
        headers, consumed = {}, len(status)
        while True:
            header = line(stream, 8192 - consumed)
            consumed += len(header)
            if header == b"\r\n":
                break
            name, value = header.split(b":", 1)
            headers[name.strip().lower()] = value.strip()
        if headers.get(b"upgrade") != b"hv2-tcp/1":
            raise RuntimeError("node selected an unsupported tunnel protocol")
        return stream
    except Exception:
        stream.close()
        raise


def fc_forward(path, port=18082):
    stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    stream.settimeout(10)
    try:
        stream.connect(str(path))
        stream.sendall(b"CONNECT 1024\n")
        reply = line(stream, 64)
        if not reply.startswith(b"OK "):
            raise RuntimeError(f"invalid Firecracker vsock acknowledgement: {reply!r}")
        if fc.rpc(stream, 1, {"kind": "forward", "port": port}).get("kind") != "acknowledged":
            raise RuntimeError("guest refused TCP port")
        return stream
    except Exception:
        stream.close()
        raise


def transfer(connect, payload):
    row = {"success": False, "attempted": True, "payload_bytes": len(payload)}
    started, stream, phase, writer = time.perf_counter(), None, "handshake", None
    write_errors = []
    try:
        stream = connect()
        connected = time.perf_counter()
        row["handshake_ms"] = (connected - started) * 1000
        phase = "transfer"
        def send():
            try:
                stream.sendall(payload)
            except Exception as error:
                write_errors.append(str(error))
        writer = threading.Thread(target=send)
        writer.start()
        response = bytearray()
        while len(response) < len(payload):
            part = stream.recv(min(65536, len(payload) - len(response)))
            if not part:
                break
            response.extend(part)
        writer.join(timeout=10)
        if writer.is_alive() or write_errors:
            raise RuntimeError(f"request writer failed: {write_errors}")
        completed = time.perf_counter()
        if response != payload:
            raise RuntimeError(f"guest byte mismatch: {len(response)} of {len(payload)}")
        row.update(success=True, transfer_ms=(completed - connected) * 1000,
                   transaction_ms=(completed - started) * 1000,
                   aggregate_payload_mib_s=2 * len(payload) / (1024 * 1024) / (completed - connected),
                   payload_sha256=hashlib.sha256(payload).hexdigest())
    except Exception as error:
        row.update(error=str(error), failure_phase=phase, failure_elapsed_ms=(time.perf_counter() - started) * 1000)
    finally:
        if stream is not None:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            stream.close()
        if writer is not None:
            writer.join(timeout=10)
            if writer.is_alive():
                row.update(success=False, error="request writer failed to stop")
    return row


def transfer_round(connect, payloads):
    """Start one transaction per payload together; retain every failed attempt."""
    if len(payloads) == 1:
        return [transfer(connect, payloads[0])]
    barrier = threading.Barrier(len(payloads))
    def run(payload):
        try:
            barrier.wait(timeout=15)
        except threading.BrokenBarrierError:
            return {"success": False, "attempted": False, "payload_bytes": len(payload),
                    "failure_phase": "client_barrier", "error": "client start barrier failed"}
        return transfer(connect, payload)
    with ThreadPoolExecutor(max_workers=len(payloads)) as executor:
        return list(executor.map(run, payloads))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["hypermachine", "firecracker", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=20)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--concurrency", type=int, default=1,
                        help="synchronized streams per guest and size (1..64)")
    parser.add_argument("--environment", required=True)
    parser.add_argument("--fixture-nodelay", action="store_true",
                        help="require the diagnostic fixture's --nodelay mode")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("requires Linux x86_64 KVM")
    if not 1 <= args.pairs <= 100 or not 1 <= args.rounds <= 100:
        parser.error("pairs and rounds must be 1..100")
    if not 1 <= args.concurrency <= 64:
        parser.error("concurrency must be 1..64")
    paths = {name: getattr(args, name).resolve() for name in ["hypermachine", "firecracker", "kernel", "initrd"]}
    paths.update(harness=Path(__file__).resolve(), engine_harness=Path(engines.__file__).resolve(), fc_harness=Path(fc.__file__).resolve())
    identities = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()}
    report = {"success": False, "environment": args.environment, "artifact_sha256": identities,
              "host": platform.platform(), "host_affinity": sorted(os.sched_getaffinity(0)),
              "cpu_count": 1, "memory_mb": 1024, "concurrency": args.concurrency, "pairs": args.pairs,
              "rounds_per_size_per_guest": args.rounds, "preparation": [], "rows": [], "cleanup_errors": [],
              "transport": {"hypermachine": "authenticated loopback HTTP/1.1 upgrade",
                            "firecracker": "native Unix vsock socket plus same guest Forward RPC"},
              "limitations": ["No startup score: verified readiness and service preparation precede timing",
                              "Different native host transports; no CLI, TLS, cluster, managed service or fleet comparison",
                              "Concurrent rows include post-barrier client scheduling; bursts are not sustained-arrival capacity",
                              "Repeated transactions share a guest; percentile rows are not independent guests",
                              "Fixed-length streaming echo on port 18082; no client half-close or server EOF requirement",
                              "MiB/s sums request and echo bytes; includes per-transaction writer-thread creation; not sustained line-rate capacity",
                              "Shared nested KVM host; no dedicated-host or concurrent load claim"]}
    report["client_tcp_nodelay"] = True
    report["fixture_tcp_nodelay"] = args.fixture_nodelay
    token = uuid.uuid4().hex
    payloads = {size: bytes(range(256)) * (size // 256) for size in [4096, 1048576]}
    process = None
    with tempfile.TemporaryDirectory(prefix="hm-tcp-bench-", dir="/var/tmp") as temporary:
        directory = Path(temporary)
        port, proxy = engines.free_port(), engines.free_port()
        while proxy == port:
            proxy = engines.free_port()
        address, url = ("127.0.0.1", port), f"http://127.0.0.1:{port}"
        def request(method, path, body=None):
            class NoRedirect(urllib.request.HTTPRedirectHandler):
                def redirect_request(self, *args, **kwargs):
                    return None
            data = None if body is None else json.dumps(body).encode()
            message = urllib.request.Request(url + path, data=data, method=method,
                headers={"content-type": "application/json", "x-hv2-cluster-token": token})
            opener = urllib.request.build_opener(NoRedirect, urllib.request.ProxyHandler({}))
            with opener.open(message, timeout=30) as response:
                raw = response.read(1048577)
                if len(raw) > 1048576:
                    raise RuntimeError("excess API response")
                return json.loads(raw) if raw else None
        try:
            with (directory / "node.log").open("wb") as log:
                process = subprocess.Popen([str(args.hypermachine), "--port", str(port), "--proxy-port", str(proxy),
                    "--no-template", "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "2",
                    "--volume-dir", str(directory / "volumes"), "--snapshot-store", str(directory / "snapshots")],
                    env={"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn", "HV2_CLUSTER_TOKEN": token,
                         "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(args.initrd)},
                    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None:
                        raise RuntimeError("node exited before readiness")
                    try:
                        request("GET", "/templates")
                        break
                    except OSError:
                        if time.monotonic() > deadline:
                            raise
                        time.sleep(.01)
                for pair in range(args.pairs):
                    for engine in (["hypermachine", "firecracker"] if pair % 2 == 0 else ["firecracker", "hypermachine"]):
                        preparation = {"pair": pair, "engine": engine, "success": False, "cleanup_success": False}
                        report["preparation"].append(preparation)
                        guest_id, fc_process, fc_log = None, None, None
                        try:
                            if engine == "hypermachine":
                                value = request("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 300, "allowInternetAccess": False})
                                guest_id = value["sandboxID"]
                                info = request("GET", f"/sandboxes/{guest_id}")
                                assert info["cpuCount"] == 1 and info["memoryMB"] == 1024
                                def execute(cmd):
                                    result = request("POST", f"/sandboxes/{guest_id}/exec", {"cmd": cmd, "timeout_secs": 10})
                                    assert result["exit_code"] == 0 and not result.get("timed_out") and not result.get("truncated")
                                    return result["stdout"]
                                connect = lambda: hm_forward(address, guest_id, token)
                            else:
                                api_path, vsock = directory / "fc-api.sock", directory / "fc-vsock.sock"
                                for name in [api_path, vsock]:
                                    if name.exists():
                                        name.unlink()
                                fc_log = (directory / f"fc-{pair}.log").open("wb")
                                fc_process = subprocess.Popen([str(args.firecracker), "--api-sock", str(api_path)],
                                    stdin=subprocess.DEVNULL, stdout=fc_log, stderr=subprocess.STDOUT)
                                fc.wait_api(api_path, time.perf_counter() + 10, fc_process)
                                fc.api(api_path, "PUT", "/machine-config", {"vcpu_count": 1, "mem_size_mib": 1024})
                                fc.api(api_path, "PUT", "/boot-source", {"kernel_image_path": str(args.kernel), "initrd_path": str(args.initrd), "boot_args": fc.BOOT_ARGS})
                                fc.api(api_path, "PUT", "/vsock", {"guest_cid": 3, "uds_path": str(vsock)})
                                fc.api(api_path, "PUT", "/actions", {"action_type": "InstanceStart"})
                                config = fc.api(api_path, "GET", "/machine-config")
                                assert config["vcpu_count"] == 1 and config["mem_size_mib"] == 1024
                                def execute(cmd):
                                    with fc.guest(vsock, time.perf_counter() + 15, fc_process) as stream:
                                        result = fc.rpc(stream, 2, {"kind": "exec", "program": "/bin/sh", "args": ["-c", cmd], "timeout_ms": 10000})
                                    assert result["kind"] == "exited" and result["exit_code"] == 0 and not result.get("timed_out") and not result.get("truncated")
                                    return result["stdout"]
                                connect = lambda: fc_forward(vsock)
                            marker = "tcp-bench-" + uuid.uuid4().hex
                            assert execute(f"printf '%s' '{marker}'") == marker
                            option = " --nodelay" if args.fixture_nodelay else ""
                            execute(f"/bin/tcp-fixture{option} </dev/null >/tmp/tcp-fixture.log 2>&1 &")
                            deadline = time.monotonic() + 5
                            ready = "ready-nodelay" if args.fixture_nodelay else "ready"
                            while execute("test -f /tmp/tcp-fixture-ready && cat /tmp/tcp-fixture-ready || true") != ready:
                                if time.monotonic() > deadline:
                                    raise TimeoutError("guest TCP fixture unavailable or mode unsupported")
                                time.sleep(.01)
                            preparation["success"] = True
                            for round_index in range(args.rounds):
                                for size, payload in payloads.items():
                                    batch = []
                                    for client in range(args.concurrency):
                                        marker = f"pair={pair},round={round_index},size={size},client={client}\n".encode()
                                        batch.append(marker + payload[len(marker):])
                                    rows = transfer_round(connect, batch)
                                    for client, row in enumerate(rows):
                                        row.update(engine=engine, pair=pair, round=round_index, client=client)
                                        report["rows"].append(row)
                        except Exception as error:
                            preparation.update(success=False, error=str(error))
                        finally:
                            try:
                                if guest_id:
                                    request("DELETE", f"/sandboxes/{guest_id}")
                                if fc_process:
                                    engines.stop(fc_process)
                                if fc_log:
                                    fc_log.close()
                                    preparation["console_utf8"] = (directory / f"fc-{pair}.log").read_bytes().decode(errors="replace")
                                preparation["cleanup_success"] = True
                            except Exception as error:
                                preparation["cleanup_error"] = str(error)
                report["remaining_sandboxes"] = len(request("GET", "/v2/sandboxes"))
        except Exception as error:
            report["setup_error"] = str(error)
        finally:
            if process is not None:
                try:
                    report["node_stopped"] = engines.stop(process)
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            report["node_log_utf8"] = (directory / "node.log").read_bytes().decode(errors="replace")
    report["artifacts_unchanged"] = all(hashlib.sha256(path.read_bytes()).hexdigest() == identities[name] for name, path in paths.items())
    report["summaries"] = {}
    for engine in ["hypermachine", "firecracker"]:
        for size in payloads:
            rows = [row for row in report["rows"] if row["engine"] == engine and row["payload_bytes"] == size and row["success"]]
            report["summaries"][f"{engine}-{size}"] = {field: fc.summary([row[field] for row in rows]) for field in ["handshake_ms", "transfer_ms", "transaction_ms", "aggregate_payload_mib_s"]}
    report["success"] = (len(report["rows"]) == args.pairs * 2 * args.rounds * len(payloads) * args.concurrency
        and all(row["success"] for row in report["rows"])
        and all(row["success"] and row["cleanup_success"] for row in report["preparation"])
        and not report["cleanup_errors"] and report.get("remaining_sandboxes") == 0 and report.get("node_stopped") and report["artifacts_unchanged"])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"success": report["success"], "rows": len(report["rows"]), "summaries": report["summaries"]}))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
