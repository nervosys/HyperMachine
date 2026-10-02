#!/usr/bin/env python3
"""Collect owned-node mappings, KVM handles and threads around cold guest batches.

Diagnostic only: no comparator, allocator intervention or performance win.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile
import time
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location("bursts", Path(__file__).with_name("bench-local-engines-concurrent.py"))
bursts = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bursts)
engines = bursts.engines


def mappings(text):
    result, current = [], None
    for line in text.splitlines():
        if re.match(r"^[0-9a-f]+-[0-9a-f]+ ", line):
            fields = line.split(maxsplit=5)
            current = {"range":fields[0], "permissions":fields[1], "path":fields[5] if len(fields) > 5 else ""}
            result.append(current)
        elif current is not None:
            name, separator, value = line.partition(":")
            if separator and name in ("Size", "Rss", "Pss", "Private_Dirty", "Anonymous", "AnonHugePages"):
                current[name + "_kib"] = int(value.split()[0])
    return result


def snapshot(pid):
    proc = Path(f"/proc/{pid}")
    text = (proc / "smaps").read_text()
    if len(text) > 16 * 1024 * 1024:
        raise RuntimeError("owned smaps exceeded diagnostic limit")
    items = mappings(text)
    categories = {}
    for item in items:
        path = item["path"]
        category = ("heap" if path == "[heap]" else "anonymous" if not path or path.startswith("[anon")
                    else "anonymous_shared" if path.startswith("/dev/zero") else "stack" if path.startswith("[stack")
                    else "special" if path.startswith("[") else "file")
        totals = categories.setdefault(category, {"mapping_count":0, "Size_kib":0, "Pss_kib":0, "Rss_kib":0})
        totals["mapping_count"] += 1
        for field in ("Size_kib", "Pss_kib", "Rss_kib"):
            totals[field] += item.get(field, 0)
    descriptors = []
    for fd in (proc / "fd").iterdir():
        try:
            descriptors.append(os.readlink(fd))
        except FileNotFoundError:
            pass  # A transient API connection can close during observation.
    return {"process_memory_kib":engines.memory(pid), "mapping_categories":categories,
            "largest_pss_mappings":sorted(items, key=lambda item:item.get("Pss_kib", 0), reverse=True)[:20],
            "large_mapping_count":sum(item.get("Size_kib", 0) >= 1024 * 1024 for item in items),
            "thread_count":len(list((proc / "task").iterdir())), "fd_count":len(descriptors),
            "kvm_vm_handles":sum("kvm-vm" in item for item in descriptors),
            "kvm_vcpu_handles":sum("kvm-vcpu" in item for item in descriptors),
            "limitation":"Sequential procfs reads are not atomic; mapping categories do not prove allocator ownership"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("daemon", "kernel", "initrd", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--batches", type=int, default=3)
    parser.add_argument("--idle-seconds", type=float, default=5)
    args = parser.parse_args()
    if platform.system() != "Linux" or not 1 <= args.concurrency <= 100 or not 1 <= args.batches <= 10 or not 0 < args.idle_seconds <= 30:
        parser.error("requires Linux, concurrency 1..100, batches 1..10 and idle seconds (0,30]")
    paths = {name:getattr(args, name).resolve() for name in ("daemon", "kernel", "initrd")}
    paths.update(diagnostic=Path(__file__).resolve(), burst_harness=Path(bursts.__file__).resolve(), shared_harness=Path(engines.__file__).resolve())
    report = {"diagnostic_only":True, "success":False, "snapshots":[], "batches":[], "cleanup_errors":[],
              "artifact_sha256":{name:engines.digest(path) for name, path in paths.items()},
              "cpu_affinity":sorted(os.sched_getaffinity(0)), "host":platform.platform(),
              "concurrency":args.concurrency, "idle_seconds":args.idle_seconds}
    process = None
    with tempfile.TemporaryDirectory(prefix="hm-retention-", dir="/var/tmp") as scratch:
        scratch = Path(scratch)
        port, proxy = engines.free_port(), engines.free_port()
        while proxy == port:
            proxy = engines.free_port()
        url = f"http://127.0.0.1:{port}"
        try:
            with (scratch / "node.log").open("wb") as log:
                process = subprocess.Popen([str(paths["daemon"]), "--port", str(port), "--proxy-port", str(proxy),
                    "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "128", "--no-template",
                    "--volume-dir", str(scratch / "volumes"), "--snapshot-store", str(scratch / "snapshots")],
                    env={"PATH":"/usr/local/bin:/usr/bin:/bin", "HV2_KERNEL":str(paths["kernel"]),
                         "HV2_INITRD":str(paths["initrd"]), "RUST_LOG":"warn"},
                    stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None:
                        raise RuntimeError("owned daemon exited before readiness")
                    try:
                        templates = engines.request(url, "GET", "/templates")
                        break
                    except OSError:
                        if time.monotonic() >= deadline:
                            raise
                        time.sleep(.02)
                base = next(item for item in templates if "base" in item.get("aliases", []))
                if base.get("snapshot") is not False or base.get("cpuCount") != 1 or base.get("memoryMB") != 1024:
                    raise RuntimeError("requires matched cold 1-vCPU/1024-MiB template")
                time.sleep(args.idle_seconds)
                report["snapshots"].append({"stage":"initial-empty", **snapshot(process.pid)})
                options = SimpleNamespace(concurrency=args.concurrency, node_pid=process.pid, url=url)
                for index in range(args.batches):
                    held = []
                    def read_memory(pid):
                        time.sleep(args.idle_seconds)
                        value = snapshot(pid)
                        held.append({"stage":"held", "batch":index, **value})
                        return value["process_memory_kib"]
                    batch = bursts.batch(options, "hypermachine", index, memory_reader=read_memory)
                    report["batches"].append(batch)
                    report["snapshots"].extend(held)
                    if engines.request(url, "GET", "/sandboxes") != []:
                        raise RuntimeError("batch cleanup left sandbox records")
                    time.sleep(args.idle_seconds)
                    report["snapshots"].append({"stage":"after-cleanup", "batch":index, **snapshot(process.pid)})
                report["success"] = all(batch["success"] for batch in report["batches"])
        except Exception as error:
            report["error"] = str(error)
        finally:
            if process is not None and process.poll() is None:
                try:
                    remaining = engines.request(url, "GET", "/sandboxes")
                    report["remaining_sandbox_count"] = len(remaining)
                    for item in remaining:
                        engines.request(url, "DELETE", f"/sandboxes/{item['sandboxID']}")
                    if remaining:
                        report["cleanup_errors"].append("remaining records required diagnostic cleanup")
                except Exception as error:
                    report["cleanup_errors"].append(str(error))
            if process is not None:
                engines.stop(process)
            report["owned_daemon_reaped"] = process is None or process.poll() is not None
    report["artifacts_unchanged"] = all(engines.digest(path) == report["artifact_sha256"][name] for name, path in paths.items())
    report["success"] = report["success"] and report["owned_daemon_reaped"] and report["artifacts_unchanged"] and not report["cleanup_errors"]
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"success":report["success"], "error":report.get("error"), "cleanup_errors":report["cleanup_errors"]}))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
