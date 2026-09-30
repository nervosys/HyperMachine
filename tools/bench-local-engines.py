#!/usr/bin/env python3
"""Compare native cold-create paths on one Linux host using identical guest files."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import socket
import subprocess
import tempfile
import time
import urllib.request
import urllib.error
import uuid

spec = importlib.util.spec_from_file_location("fc", Path(__file__).with_name("bench-firecracker-local.py"))
fc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fc)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def free_port():
    with socket.socket() as stream:
        stream.bind(("127.0.0.1",0))
        return stream.getsockname()[1]


def request(url, method, path, body=None):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs): return None
    opener = urllib.request.build_opener(NoRedirect,urllib.request.ProxyHandler({}))
    data = None if body is None else json.dumps(body).encode()
    message = urllib.request.Request(url + path, data=data, method=method, headers={"Content-Type":"application/json"})
    try:
        response = opener.open(message,timeout=30)
    except urllib.error.HTTPError as error:
        detail = error.read(4096).decode(errors="replace")
        raise RuntimeError(f"HTTP {error.code}: {detail}") from error
    with response:
        data = response.read(8 * 1024 * 1024 + 1)
        if len(data) > 8 * 1024 * 1024: raise RuntimeError("API response exceeded limit")
        return json.loads(data) if data else None


def memory(pid):
    values = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        name, _, amount = line.partition(":")
        if name in ("Rss","Pss","Private_Clean","Private_Dirty","Anonymous"):
            values[name + "_kib"] = int(amount.split()[0])
    return values


def hm_sample(url,index,pid=None):
    row = {"index":index,"success":False,"cleanup_success":False}
    sandbox = None
    marker = "hm-engine-" + uuid.uuid4().hex
    try:
        started = time.perf_counter()
        value = request(url,"POST","/v2/sandboxes",{"templateID":"base","timeout":300,"allowInternetAccess":False})
        sandbox = value.get("sandboxID")
        if not isinstance(sandbox,str) or not sandbox: raise RuntimeError("create returned no known sandbox ID")
        value = request(url,"POST",f"/sandboxes/{sandbox}/exec",{"cmd":f"printf '%s' '{marker}'","timeout_secs":10})
        row["ready_ms"] = (time.perf_counter() - started) * 1000
        if value.get("exit_code") != 0 or value.get("stdout") != marker or value.get("timed_out"):
            raise RuntimeError("guest output/status mismatch")
        info = request(url,"GET",f"/sandboxes/{sandbox}")
        if info.get("cpuCount") != 1 or info.get("memoryMB") != 1024:
            raise RuntimeError("guest resources mismatch")
        if pid is not None: row["node_memory_ready"] = memory(pid)
        row["success"] = True
    except Exception as error:
        row["error"] = str(error)
    finally:
        if sandbox is not None:
            try:
                request(url,"DELETE",f"/sandboxes/{sandbox}")
                row["cleanup_success"] = True
            except Exception as error:
                row["cleanup_error"] = str(error)
            else:
                if pid is not None:
                    try: row["node_memory_after_delete"] = memory(pid)
                    except Exception as error:
                        row["diagnostic_error"] = str(error)
                        row["success"] = False
    return row


def stop(process):
    if process.poll() is None:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait(timeout=5)
    return process.poll() is not None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("hyperMachine","firecracker","kernel","initrd"):
        parser.add_argument("--" + ("hypermachine" if name == "hyperMachine" else name), dest=name, required=True, type=Path)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--pairs", type=int, default=20)
    parser.add_argument("--timeout", type=int, default=30)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64": parser.error("requires Linux x86_64")
    if not 1 <= args.pairs <= 1000 or not 1 <= args.timeout <= 300: parser.error("invalid pair count/timeout")
    paths = {name:getattr(args,name).resolve() for name in ("hyperMachine","firecracker","kernel","initrd")}
    paths["harness"] = Path(__file__).resolve()
    paths["firecracker_harness"] = Path(fc.__file__).resolve()
    identities = {name:digest(path) for name,path in paths.items()}
    records = []
    preflight = None
    setup_error = None
    cleanup_error = None
    baseline_memory = None
    node_log_tail = None
    with tempfile.TemporaryDirectory(prefix="hm-native-",dir="/var/tmp") as directory:
        directory = Path(directory)
        port, proxy = free_port(), free_port()
        while proxy == port: proxy = free_port()
        url = f"http://127.0.0.1:{port}"
        command = [str(paths["hyperMachine"]),"--port",str(port),"--proxy-port",str(proxy),
            "--memory-mb","1024","--cpu-cores","1","--capacity","128","--no-template",
            "--volume-dir",str(directory/"volumes"),"--snapshot-store",str(directory/"snapshots")]
        process = None
        try:
            with (directory/"node.log").open("wb") as log:
                process = subprocess.Popen(command,env={"PATH":"/usr/local/bin:/usr/bin:/bin",
                    "HV2_KERNEL":str(paths["kernel"]),"HV2_INITRD":str(paths["initrd"]),"RUST_LOG":"warn"},
                    stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
                deadline = time.perf_counter() + 30
                while time.perf_counter() < deadline:
                    if process.poll() is not None: raise RuntimeError("isolated HyperMachine node exited during startup")
                    try:
                        preflight = request(url,"GET","/templates")
                        break
                    except OSError: time.sleep(.01)
                if preflight is None: raise RuntimeError("node did not become ready")
                base = next((item for item in preflight if "base" in item.get("aliases",[])),None)
                if not base or base.get("snapshot") is not False or base.get("cpuCount") != 1 or base.get("memoryMB") != 1024:
                    raise RuntimeError("node must report cold base template at 1 vCPU/1024 MiB")
                baseline_memory = memory(process.pid)
                for index in range(args.pairs):
                    # Alternate order to reduce drift bias; concurrency is one.
                    order = ("hypermachine","firecracker") if index % 2 == 0 else ("firecracker","hypermachine")
                    for engine in order:
                        row = hm_sample(url,index,process.pid) if engine == "hypermachine" else fc.sample(args,index)
                        row["engine"] = engine
                        row["pair"] = index
                        records.append(row)
                if request(url,"GET","/sandboxes") != []:
                    cleanup_error = "isolated node has remaining sandbox records"
                if any(not row["success"] for row in records):
                    with (directory/"node.log").open("rb") as source:
                        source.seek(max(0,(directory/"node.log").stat().st_size-8000))
                        node_log_tail = source.read(8000).decode(errors="replace")
        except Exception as error:
            setup_error = str(error)
            logfile = directory/"node.log"
            if logfile.exists():
                with logfile.open("rb") as source:
                    source.seek(max(0,logfile.stat().st_size-4000))
                    setup_error += ": " + source.read(4000).decode(errors="replace")
        finally:
            if process is not None:
                try:
                    if not stop(process): cleanup_error = "isolated node did not stop"
                except Exception as error: cleanup_error = str(error)
    try:
        unchanged = all(digest(path) == identities[name] for name,path in paths.items())
    except OSError:
        unchanged = False
    passed = not setup_error and not cleanup_error and unchanged and len(records) == args.pairs*2 and all(row["success"] and row["cleanup_success"] for row in records)
    print(json.dumps({"schema_version":1,"lifecycle":"native-cold-create-to-command","concurrency":1,
        "pairs":args.pairs,"order":"alternating AB/BA","environment":args.environment,"host":platform.platform(),
        "artifact_sha256":identities,"artifacts_unchanged":unchanged,"cpu_count":1,"memory_mb":1024,
        "common_boot_args":fc.BOOT_ARGS,"hypermachine_template_preflight":preflight,
        "setup_error":setup_error,"cleanup_error":cleanup_error,"samples":records,
        "node_memory_baseline":baseline_memory,"node_log_tail":node_log_tail,
        "ready_ms":{engine:fc.summary([row["ready_ms"] for row in records if row["engine"]==engine and row["success"] and row["cleanup_success"]]) for engine in ("hypermachine","firecracker")},
        "limitations":["Shared nested-KVM host; no CPU pinning or dedicated hardware","Different native control paths: persistent HyperMachine HTTP node versus Firecracker process and Unix API","Cold lifecycle only; no snapshot/SDK/platform comparison","Engine-generated device kernel arguments differ"],"success":passed},indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
