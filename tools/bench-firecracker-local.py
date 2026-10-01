#!/usr/bin/env python3
"""Measure cold Firecracker creation through a verified shared guest command."""
import argparse
import errno
import hashlib
import http.client
import json
import math
from pathlib import Path
import platform
import socket
import struct
import subprocess
import tempfile
import time
import uuid

BOOT_ARGS = "console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=3 8250.nr_uarts=1 i8042.noaux i8042.nomux i8042.nopnp i8042.dumbkbd"
MAX_FRAME = 8 * 1024 * 1024
GUEST_READY_TIMEOUT_SECONDS = 15


class UnixHTTP(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost", timeout=5)
        self.path = str(path)

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.path)


def api(path, method, route, value=None):
    connection = UnixHTTP(path)
    try:
        connection.request(method, route, None if value is None else json.dumps(value), {"Content-Type":"application/json"})
        response = connection.getresponse()
        body = response.read(1024 * 1024 + 1)
        if len(body) > 1024 * 1024 or not 200 <= response.status < 300:
            raise RuntimeError(f"Firecracker API {route}: {response.status}: {body[:1000]!r}")
        return json.loads(body) if body else None
    finally:
        connection.close()


def wait_api(path, deadline, process):
    """Socket creation precedes listen readiness; probe without replaying PUTs."""
    while time.perf_counter() < deadline:
        if process.poll() is not None:
            raise RuntimeError("Firecracker exited before API readiness")
        try:
            api(path, "GET", "/machine-config")
            return
        except OSError as error:
            if error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
                raise
            time.sleep(.001)
    raise TimeoutError("API readiness deadline exceeded")


def exact(stream, size):
    data = bytearray()
    while len(data) < size:
        part = stream.recv(size - len(data))
        if not part: raise RuntimeError("guest closed its response stream")
        data.extend(part)
    return bytes(data)


def rpc(stream, id, operation):
    payload = json.dumps({"id":id,"version":3,"op":operation}).encode()
    stream.sendall(struct.pack("<I", len(payload)) + payload)
    size = struct.unpack("<I", exact(stream,4))[0]
    if not 0 < size <= MAX_FRAME: raise RuntimeError("invalid guest frame length")
    response = json.loads(exact(stream,size))
    if response.get("id") != id or response.get("version") != 3:
        raise RuntimeError("guest protocol version/ID mismatch")
    return response["result"]


def guest(path, deadline, process):
    while time.perf_counter() < deadline:
        if process.poll() is not None: raise RuntimeError("Firecracker exited before guest readiness")
        stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        stream.settimeout(min(1, max(.001, deadline - time.perf_counter())))
        try:
            stream.connect(str(path))
            stream.sendall(b"CONNECT 1024\n")
            line = bytearray()
            while len(line) < 64 and not line.endswith(b"\n"):
                part = stream.recv(1)
                if not part: raise ConnectionError("guest port not listening yet")
                line.extend(part)
            if not line.startswith(b"OK ") or not line.endswith(b"\n"):
                raise RuntimeError("invalid vsock acknowledgement")
            stream.settimeout(max(.001, deadline - time.perf_counter()))
            if rpc(stream,1,{"kind":"ping"}).get("kind") != "pong":
                raise RuntimeError("guest ping failed")
            return stream
        except (OSError, ConnectionError):
            stream.close()
            time.sleep(.001)
        except Exception:
            stream.close()
            raise
    raise TimeoutError("guest did not become ready before deadline")


def sample(args, index):
    row = {"index":index,"success":False,"cleanup_success":False}
    process = None
    with tempfile.TemporaryDirectory(prefix="hm-fc-", dir="/var/tmp") as directory:
        directory = Path(directory)
        socket_path = directory / "api.sock"
        vsock_path = directory / "vsock.sock"
        logfile = directory / "console.log"
        marker = "hm-engine-" + uuid.uuid4().hex
        try:
            with logfile.open("wb") as log:
                started = time.perf_counter()
                deadline = started + args.timeout
                process = subprocess.Popen([str(args.firecracker.resolve()),"--api-sock",str(socket_path)],
                    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                wait_api(socket_path, deadline, process)
                api(socket_path,"PUT","/machine-config",{"vcpu_count":1,"mem_size_mib":1024})
                api(socket_path,"PUT","/boot-source",{"kernel_image_path":str(args.kernel.resolve()),
                    "initrd_path":str(args.initrd.resolve()),"boot_args":BOOT_ARGS})
                api(socket_path,"PUT","/vsock",{"guest_cid":3,"uds_path":str(vsock_path)})
                api(socket_path,"PUT","/actions",{"action_type":"InstanceStart"})
                guest_deadline = min(deadline, time.perf_counter() + GUEST_READY_TIMEOUT_SECONDS)
                with guest(vsock_path,guest_deadline,process) as stream:
                    result = rpc(stream,2,{"kind":"exec","program":"/bin/sh",
                        "args":["-c",f"printf '%s' '{marker}'"],"timeout_ms":10000})
                row["ready_ms"] = (time.perf_counter() - started) * 1000
                if result.get("kind") != "exited" or result.get("exit_code") != 0 or result.get("stdout") != marker or result.get("timed_out") or result.get("truncated"):
                    raise RuntimeError("guest command output/status mismatch")
                config = api(socket_path,"GET","/machine-config")
                if config.get("vcpu_count") != 1 or config.get("mem_size_mib") != 1024:
                    raise RuntimeError("machine resources mismatch")
                row["success"] = True
        except Exception as error:
            row["error"] = str(error)
            if logfile.exists():
                with logfile.open("rb") as log:
                    log.seek(max(0,logfile.stat().st_size-4000))
                    row["console_tail"] = log.read(4000).decode(errors="replace")
        finally:
            if process is None:
                row["cleanup_success"] = True
            else:
                try:
                    if process.poll() is None:
                        process.terminate()
                        try: process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill(); process.wait(timeout=5)
                    row["cleanup_success"] = process.poll() is not None
                except Exception as error:
                    row["cleanup_error"] = str(error)
    return row


def summary(values):
    if not values: return None
    values = sorted(values)
    return {"n":len(values),"min":values[0],"max":values[-1],"mean":sum(values)/len(values),
        **{f"p{p}":values[math.ceil(p/100*len(values))-1] for p in (50,95,99)}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("firecracker","kernel","initrd"):
        parser.add_argument("--"+name, type=Path, required=True)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--timeout", type=int, default=30)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64": parser.error("requires Linux x86_64")
    if not 1 <= args.samples <= 1000 or not 1 <= args.timeout <= 300: parser.error("invalid samples/timeout")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    identities = {name:digest(getattr(args,name)) for name in ("firecracker","kernel","initrd")}
    source_digest = digest(Path(__file__))
    records = [sample(args,index) for index in range(args.samples)]
    try:
        unchanged = source_digest == digest(Path(__file__)) and all(identities[name] == digest(getattr(args,name)) for name in identities)
    except OSError:
        unchanged = False
    passed = unchanged and all(row["success"] and row["cleanup_success"] for row in records)
    print(json.dumps({"schema_version":1,"engine":"firecracker","lifecycle":"cold-create",
        "concurrency":1,"environment":args.environment,"host":platform.platform(),"cpu_count":1,"memory_mb":1024,
        "boot_args":BOOT_ARGS,"guest_readiness_timeout_s":GUEST_READY_TIMEOUT_SECONDS,
        "total_startup_timeout_s":args.timeout,"artifact_sha256":identities,"harness_sha256":source_digest,"artifacts_unchanged":unchanged,
        "samples":records,"ready_ms":summary([row["ready_ms"] for row in records if row["success"] and row["cleanup_success"]]),
        "success":passed},indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
