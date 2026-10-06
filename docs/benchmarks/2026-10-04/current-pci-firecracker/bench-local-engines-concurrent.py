#!/usr/bin/env python3
"""Matched native cold-start bursts; retain each batch until readiness is measured."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import threading
import time
import uuid

spec = importlib.util.spec_from_file_location("engines", Path(__file__).with_name("bench-local-engines.py"))
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
fc = engines.fc


def daemon_environment(args):
    """Explicit child environment; allocator experiment never reaches other engines."""
    environment = {"PATH":"/usr/local/bin:/usr/bin:/bin", "HV2_KERNEL":str(args.kernel),
                   "HV2_INITRD":str(args.initrd), "RUST_LOG":getattr(args, "daemon_log_filter", "warn")}
    arena_max = getattr(args, "daemon_allocator_arena_max", None)
    if arena_max is not None:
        environment["MALLOC_ARENA_MAX"] = str(arena_max)
    mmap_threshold = getattr(args, "daemon_allocator_mmap_threshold", None)
    if mmap_threshold is not None:
        environment["MALLOC_MMAP_THRESHOLD_"] = str(mmap_threshold)
    return environment


def hm_attempt(args, row, batch_start, ready):
    sandbox = None
    phase = "create"
    started = None
    try:
        started = time.perf_counter()
        row["start_offset_ms"] = (started - batch_start) * 1000
        marker = "hm-concurrent-" + uuid.uuid4().hex
        value = engines.request(args.url, "POST", "/v2/sandboxes",
            {"templateID":"base", "timeout":300, "allowInternetAccess":False})
        sandbox = value.get("sandboxID")
        if not isinstance(sandbox, str) or not sandbox:
            raise RuntimeError("create returned no known sandbox ID")
        created = time.perf_counter()
        row["create_ms"] = (created - started) * 1000
        phase = "exec"
        value = engines.request(args.url, "POST", f"/sandboxes/{sandbox}/exec",
            {"cmd":f"printf '%s' '{marker}'", "timeout_secs":10})
        row["ready_ms"] = (time.perf_counter() - started) * 1000
        row["exec_ms"] = row["ready_ms"] - row["create_ms"]
        if value.get("exit_code") != 0 or value.get("stdout") != marker or value.get("timed_out") or value.get("truncated"):
            raise RuntimeError("guest output/status mismatch")
        phase = "resources"
        info = engines.request(args.url, "GET", f"/sandboxes/{sandbox}")
        if info.get("cpuCount") != 1 or info.get("memoryMB") != 1024:
            raise RuntimeError("guest resources mismatch")
        row["success"] = True
    except Exception as error:
        row["error"] = str(error)
        row["failure_phase"] = phase
        row["failure_elapsed_ms"] = (time.perf_counter() - started) * 1000 if started is not None else None
    finally:
        ready(row, args.node_pid)
        if sandbox is not None:
            try:
                engines.request(args.url, "DELETE", f"/sandboxes/{sandbox}")
                row["cleanup_success"] = True
            except Exception as error: row["cleanup_error"] = str(error)


def fc_attempt(args, row, batch_start, ready):
    process = None
    directory = None
    log = None
    started = None
    phase = "prepare"
    try:
        directory = tempfile.TemporaryDirectory(prefix="hm-fc-burst-", dir="/var/tmp")
        path = Path(directory.name)
        socket, vsock = path/"api.sock", path/"vsock.sock"
        log = (path/"console.log").open("wb")
        started = time.perf_counter()
        row["start_offset_ms"] = (started - batch_start) * 1000
        marker = "hm-concurrent-" + uuid.uuid4().hex
        deadline = started + args.timeout
        phase = "api-setup"
        process = subprocess.Popen([str(args.firecracker), "--api-sock", str(socket)],
            stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        fc.wait_api(socket, deadline, process)
        fc.api(socket, "PUT", "/machine-config", {"vcpu_count":1, "mem_size_mib":1024})
        fc.api(socket, "PUT", "/boot-source", {"kernel_image_path":str(args.kernel),
            "initrd_path":str(args.initrd), "boot_args":fc.BOOT_ARGS})
        fc.api(socket, "PUT", "/vsock", {"guest_cid":3, "uds_path":str(vsock)})
        fc.api(socket, "PUT", "/actions", {"action_type":"InstanceStart"})
        configured = time.perf_counter()
        row["api_setup_ms"] = (configured - started) * 1000
        phase = "agent-connect"
        with fc.guest(vsock, min(deadline, time.perf_counter()+fc.GUEST_READY_TIMEOUT_SECONDS), process) as stream:
            connected = time.perf_counter()
            row["agent_connect_ms"] = (connected - configured) * 1000
            phase = "exec"
            value = fc.rpc(stream, 2, {"kind":"exec", "program":"/bin/sh",
                "args":["-c", f"printf '%s' '{marker}'"], "timeout_ms":10000})
        row["ready_ms"] = (time.perf_counter() - started) * 1000
        row["exec_ms"] = row["ready_ms"] - row["api_setup_ms"] - row["agent_connect_ms"]
        if value.get("kind") != "exited" or value.get("exit_code") != 0 or value.get("stdout") != marker or value.get("timed_out") or value.get("truncated"):
            raise RuntimeError("guest output/status mismatch")
        phase = "resources"
        config = fc.api(socket, "GET", "/machine-config")
        if config.get("vcpu_count") != 1 or config.get("mem_size_mib") != 1024:
            raise RuntimeError("guest resources mismatch")
        row["success"] = True
    except Exception as error:
        row["error"] = str(error)
        row["failure_phase"] = phase
        row["failure_elapsed_ms"] = (time.perf_counter() - started) * 1000 if started is not None else None
        if directory is not None:
            logfile = Path(directory.name)/"console.log"
            if logfile.exists(): row["console_tail"] = logfile.read_bytes()[-4000:].decode(errors="replace")
    finally:
        ready(row, process.pid if process is not None else None)
        try:
            row["cleanup_success"] = process is None or engines.stop(process)
        except Exception as error: row["cleanup_error"] = str(error)
        if log is not None: log.close()
        if directory is not None: directory.cleanup()


def batch(args, engine, index, attempt=None, memory_reader=None, memory_baseline=None, idle_wait=None):
    """Barrier-release arrivals and retain all owned guests through measurement."""
    attempt = attempt or (hm_attempt if engine == "hypermachine" else fc_attempt)
    memory_reader = memory_reader or engines.memory
    idle_seconds = getattr(args, "memory_idle_seconds", 0)
    idle_wait = idle_wait or time.sleep
    count = args.concurrency
    timing = {}
    rows = [{"index":slot, "pair":index, "engine":engine,
        "success":False, "cleanup_success":False} for slot in range(count)]
    pids = set()
    lock = threading.Lock()
    start = threading.Barrier(count+1, action=lambda: timing.update(start=time.perf_counter()))
    held = threading.Barrier(count+1)
    release = threading.Event()
    report = {"engine":engine, "pair":index, "concurrency":count, "samples":rows, "error":None}

    def worker(row):
        arrived = False
        def ready(value, pid):
            nonlocal arrived
            if arrived: return
            arrived = True
            value["validated_offset_ms"] = (time.perf_counter() - timing["start"]) * 1000
            with lock:
                if pid is not None: pids.add(pid)
            try:
                held.wait(timeout=120)
                if not release.wait(timeout=120): raise TimeoutError("batch measurement did not release guests")
            except Exception as error:
                value["success"] = False
                value["hold_error"] = str(error)
        try:
            start.wait(timeout=30)
            attempt(args, row, timing["start"], ready)
        except Exception as error:
            row["success"] = False
            row["error"] = str(error)
        finally:
            if not arrived and "start" in timing: ready(row, None)

    with ThreadPoolExecutor(max_workers=count) as pool:
        futures = [pool.submit(worker, row) for row in rows]
        try:
            start.wait(timeout=30)
            held.wait(timeout=120)
            report["readiness_wall_ms"] = max(row["validated_offset_ms"] for row in rows)
            offsets = [row["start_offset_ms"] for row in rows if "start_offset_ms" in row]
            report["launch_spread_ms"] = max(offsets)-min(offsets) if offsets else None
            report["passing_attempts"] = sum(row["success"] for row in rows)
            report["passing_attempts_per_second"] = report["passing_attempts"]/(report["readiness_wall_ms"]/1000)
            report["held_process_count"] = len(pids)
            readings = [memory_reader(pid) for pid in sorted(pids)]
            report["held_process_memory_kib"] = {key:sum(item[key] for item in readings) for key in readings[0]} if readings else {}
            report["all_guests_validated_while_held"] = all(row["success"] for row in rows)
            if idle_seconds:
                idle_started = time.perf_counter()
                idle_wait(idle_seconds)
                measurement_started = time.perf_counter()
                idle_readings = [memory_reader(pid) for pid in sorted(pids)]
                if not idle_readings:
                    raise RuntimeError("no owned process available for idle memory measurement")
                idle_memory = {key:sum(item[key] for item in idle_readings) for key in idle_readings[0]}
                baseline = memory_baseline if engine == "hypermachine" else {key:0 for key in idle_memory}
                if baseline is None or set(baseline) != set(idle_memory):
                    raise RuntimeError("idle memory measurement requires a matching empty-process baseline")
                report["idle_process_memory_kib"] = idle_memory
                report["empty_process_memory_baseline_kib"] = baseline
                report["incremental_idle_process_memory_kib"] = {key:idle_memory[key]-baseline[key] for key in idle_memory}
                report["memory_idle_requested_seconds"] = idle_seconds
                report["memory_idle_actual_seconds"] = measurement_started-idle_started
                report["idle_memory_read_duration_ms"] = (time.perf_counter()-measurement_started)*1000
                report["guest_idle_at_measurement_start_ms"] = {
                    "min":min((measurement_started-timing["start"])*1000-row["validated_offset_ms"] for row in rows),
                    "max":max((measurement_started-timing["start"])*1000-row["validated_offset_ms"] for row in rows)}
        except Exception as error: report["error"] = str(error)
        finally:
            release.set()
            if report["error"]:
                start.abort()
                held.abort()
        for future in futures: future.result()
    report["total_wall_ms_including_cleanup"] = (time.perf_counter()-timing["start"])*1000 if "start" in timing else None
    report["success"] = not report["error"] and all(row["success"] and row["cleanup_success"] for row in rows)
    return report


def arrival_run(args, engine, index, attempt=None):
    """Fixed-rate arrivals; worker limits queue work instead of hiding demand."""
    attempt = attempt or (hm_attempt if engine == "hypermachine" else fc_attempt)
    started = time.perf_counter()
    rows = [{"index":slot, "pair":index, "engine":engine, "success":False,
             "cleanup_success":False, "scheduled_offset_ms":slot / args.arrival_rate * 1000}
            for slot in range(args.arrival_samples)]
    def worker(row):
        row["worker_start_offset_ms"] = (time.perf_counter() - started) * 1000
        row["client_queue_ms"] = row["worker_start_offset_ms"] - row["submitted_offset_ms"]
        def ready(value, pid):
            value["validated_offset_ms"] = (time.perf_counter() - started) * 1000
            value["scheduled_to_validation_ms"] = value["validated_offset_ms"] - value["scheduled_offset_ms"]
        try:
            attempt(args, row, started, ready)
        except Exception as error:
            row.update(success=False, error=str(error))
        finally:
            if "validated_offset_ms" not in row:
                ready(row, None)
            row["cleanup_completed_offset_ms"] = (time.perf_counter() - started) * 1000
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = []
        for row in rows:
            delay = started + row["scheduled_offset_ms"] / 1000 - time.perf_counter()
            if delay > 0:
                time.sleep(delay)
            row["submitted_offset_ms"] = (time.perf_counter() - started) * 1000
            row["submission_lag_ms"] = row["submitted_offset_ms"] - row["scheduled_offset_ms"]
            futures.append(pool.submit(worker, row))
        for future in futures:
            future.result()
    elapsed = time.perf_counter() - started
    passing = sum(row["success"] and row["cleanup_success"] for row in rows)
    return {"engine":engine, "pair":index, "concurrency":args.concurrency,
            "offered_arrivals_per_second":args.arrival_rate, "attempts":len(rows),
            "passing_attempts":passing, "completed_lifecycles_per_second":passing / elapsed,
            "total_wall_ms_including_cleanup":elapsed * 1000, "samples":rows,
            "success":passing == len(rows), "error":None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("hypermachine", "firecracker", "kernel", "initrd"):
        parser.add_argument("--"+name, required=True, type=Path)
    parser.add_argument("--environment", required=True)
    parser.add_argument("--pairs", type=int, default=10)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--arrival-rate", type=float,
        help="opt-in fixed arrivals/second with client queueing; replaces burst holds")
    parser.add_argument("--arrival-samples", type=int, default=100)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--memory-idle-seconds", type=float, default=0,
        help="opt-in fixed idle hold and per-batch empty-node baseline, between 0 and 30 seconds")
    parser.add_argument("--daemon-guest-transport", choices=("mmio", "pci"), default="mmio",
        help="explicit owned HyperMachine agent transport; Firecracker configuration is unchanged")
    parser.add_argument("--daemon-allocator-arena-max", type=int,
        help="opt-in glibc arena limit for the owned HyperMachine daemon only")
    parser.add_argument("--daemon-allocator-mmap-threshold", type=int,
        help="opt-in static glibc mmap threshold in bytes for the owned HyperMachine daemon only")
    parser.add_argument("--daemon-log-filter", default="warn",
        help="explicit diagnostic tracing filter; changes timing, default warn for comparisons")
    parser.add_argument("--cold-start-concurrency", type=int,
        help="opt-in HyperMachine cold boot budget (1..1024); readiness includes queue time")
    args = parser.parse_args()
    if args.cold_start_concurrency is not None and not 1 <= args.cold_start_concurrency <= 1024:
        parser.error("cold-start-concurrency requires 1..1024")
    if args.daemon_allocator_mmap_threshold is not None and not 4096 <= args.daemon_allocator_mmap_threshold <= 33554432:
        parser.error("daemon-allocator-mmap-threshold requires 4096..33554432 bytes")
    if not args.daemon_log_filter.strip() or len(args.daemon_log_filter) > 512:
        parser.error("daemon log filter must be nonempty and <=512 characters")
    if args.arrival_rate is not None and (not math.isfinite(args.arrival_rate) or
            not 0 < args.arrival_rate <= 1000 or not 1 <= args.arrival_samples <= 10000 or
            args.arrival_samples / args.arrival_rate > 60 or args.memory_idle_seconds):
        parser.error("arrival runs require finite rate 0..1000, samples 1..10000, <=60-second schedule and no idle hold")
    if platform.system() != "Linux" or platform.machine() != "x86_64": parser.error("requires Linux x86_64")
    if not 1 <= args.pairs <= 1000 or not 1 <= args.concurrency <= 100 or not 1 <= args.timeout <= 60 or not 0 <= args.memory_idle_seconds <= 30 or (args.daemon_allocator_arena_max is not None and not 1 <= args.daemon_allocator_arena_max <= 128):
        parser.error("invalid pairs, concurrency or timeout")
    paths = {name:getattr(args, name).resolve() for name in ("hypermachine", "firecracker", "kernel", "initrd")}
    for name, path in paths.items(): setattr(args, name, path)
    paths.update(harness=Path(__file__).resolve(), shared_harness=Path(engines.__file__).resolve(),
        firecracker_harness=Path(fc.__file__).resolve())
    identities = {name:engines.digest(path) for name, path in paths.items()}
    report = {"schema_version":1, "lifecycle":"native-cold-create-to-command-bursts", "concurrency":args.concurrency,
        "pairs":args.pairs, "order":"alternating engine-batch AB/BA", "environment":args.environment,
        "host":platform.platform(), "artifact_sha256":identities, "cpu_count":1, "memory_mb":1024,
        "daemon_guest_transport":args.daemon_guest_transport,
        "daemon_allocator_arena_max":args.daemon_allocator_arena_max,
        "daemon_allocator_mmap_threshold":args.daemon_allocator_mmap_threshold,
        "daemon_log_filter":args.daemon_log_filter,
        "cold_start_concurrency":args.cold_start_concurrency,
        "queue_included_in_ready_ms":True,
        "driver_cpu_affinity":sorted(os.sched_getaffinity(0)), "guest_readiness_timeout_s":{
            "hypermachine":15, "firecracker":fc.GUEST_READY_TIMEOUT_SECONDS},
        "firecracker_total_startup_timeout_s":args.timeout, "common_boot_args":fc.BOOT_ARGS,
        "batches":[], "setup_error":None, "cleanup_errors":[], "success":False,
        "limitations":["Shared nested-KVM host; no dedicated hardware",
            "Persistent HyperMachine HTTP daemon versus fresh Firecracker processes and Unix APIs",
            "Barrier-released arrivals have measured scheduling spread; not sustained arrivals",
            "All guests held until last validation; earlier guests have longer idle time",
            "Aggregate process PSS includes guest/VMM memory but excludes kernel allocations; no long idle/density test",
            "Cold native engines only; no managed SDK, snapshot or fleet comparison"]}
    process = None
    if args.memory_idle_seconds:
        report["memory_idle_seconds"] = args.memory_idle_seconds
        report["memory_method"] = "HM held daemon minus same-batch empty daemon PSS; FC summed fresh VMM PSS minus zero-process baseline"
        report["limitations"].append("Fixed idle hold starts after all readiness checks; individual guest idle ages and sequential read duration are recorded; PSS excludes kernel memory and does not prove density")
    if args.arrival_rate is not None:
        report["lifecycle"] = "native-cold-create-to-command-fixed-rate-arrivals"
        report["arrival_rate"] = args.arrival_rate
        report["arrival_samples"] = args.arrival_samples
        report["limitations"] = [item for item in report["limitations"]
            if not item.startswith(("Barrier-released", "All guests held"))]
        report["limitations"].append("Fixed-rate offered arrivals include measured submission lag and client worker queueing; guests cleaned immediately after validation; bounded one-node runs do not prove sustainable capacity")
    if args.daemon_log_filter != "warn":
        report["limitations"].append("Diagnostic daemon tracing is enabled and changes timing; this run is not a scored performance comparison")
    with tempfile.TemporaryDirectory(prefix="hm-concurrent-", dir="/var/tmp") as directory:
        directory = Path(directory)
        port, proxy = engines.free_port(), engines.free_port()
        while proxy == port: proxy = engines.free_port()
        args.url = f"http://127.0.0.1:{port}"
        try:
            with (directory/"node.log").open("wb") as log:
                daemon_argv = [str(args.hypermachine), "--port", str(port), "--proxy-port", str(proxy),
                    "--memory-mb", "1024", "--cpu-cores", "1", "--capacity", "128", "--no-template",
                    "--volume-dir", str(directory/"volumes"), "--snapshot-store", str(directory/"snapshots")]
                if args.daemon_guest_transport != "mmio":
                    daemon_argv.extend(["--guest-transport", args.daemon_guest_transport])
                if args.cold_start_concurrency is not None:
                    daemon_argv.extend(["--cold-start-concurrency", str(args.cold_start_concurrency)])
                report["daemon_argv"] = daemon_argv
                process = subprocess.Popen(daemon_argv,
                    env=daemon_environment(args), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
                args.node_pid = process.pid
                deadline = time.monotonic()+30
                while True:
                    if process.poll() is not None: raise RuntimeError("isolated node exited before readiness")
                    try: templates = engines.request(args.url, "GET", "/templates"); break
                    except OSError:
                        if time.monotonic() >= deadline: raise
                        time.sleep(.01)
                base = next((item for item in templates if "base" in item.get("aliases", [])), None)
                if not base or base.get("snapshot") is not False or base.get("cpuCount") != 1 or base.get("memoryMB") != 1024:
                    raise RuntimeError("cold base template must use 1 vCPU/1024 MiB")
                report["hypermachine_template_preflight"] = base
                report["node_memory_baseline_kib"] = engines.memory(process.pid)
                for index in range(args.pairs):
                    order = ("hypermachine", "firecracker") if index % 2 == 0 else ("firecracker", "hypermachine")
                    for engine in order:
                        baseline = None
                        if args.memory_idle_seconds and engine == "hypermachine":
                            if engines.request(args.url, "GET", "/sandboxes") != []:
                                raise RuntimeError("idle-memory baseline requires an empty isolated node")
                            time.sleep(args.memory_idle_seconds)
                            baseline = engines.memory(process.pid)
                        item = (arrival_run(args, engine, index) if args.arrival_rate is not None
                                else batch(args, engine, index, memory_baseline=baseline))
                        report["batches"].append(item)
                        if args.memory_idle_seconds and engine == "hypermachine":
                            if engines.request(args.url, "GET", "/sandboxes") != []:
                                raise RuntimeError("idle-memory batch cleanup left sandbox records")
                            time.sleep(args.memory_idle_seconds)
                            item["empty_node_memory_after_cleanup_kib"] = engines.memory(process.pid)
        except Exception as error: report["setup_error"] = str(error)
        finally:
            if process is not None and process.poll() is None:
                try:
                    remaining = engines.request(args.url, "GET", "/sandboxes")
                    report["remaining_sandbox_count"] = len(remaining)
                    if remaining:
                        report["cleanup_errors"].append("node retained sandbox records")
                        for item in remaining: engines.request(args.url, "DELETE", f"/sandboxes/{item['sandboxID']}")
                except Exception as error: report["cleanup_errors"].append(str(error))
            if process is not None:
                try:
                    if not engines.stop(process): report["cleanup_errors"].append("node did not stop")
                except Exception as error: report["cleanup_errors"].append(str(error))
                report["daemon_exit_code"] = process.returncode
            if (directory/"node.log").exists():
                logfile = directory/"node.log"
                report["node_log_tail"] = logfile.read_bytes()[-8000:].decode(errors="replace")
                if args.daemon_log_filter != "warn":
                    with logfile.open("rb") as stream:
                        diagnostic = stream.read((1 << 20) + 1)
                    report["node_diagnostic_log_truncated"] = len(diagnostic) > 1 << 20
                    report["node_diagnostic_log"] = diagnostic[:1 << 20].decode(errors="replace")
    report["artifacts_unchanged"] = all(engines.digest(path) == identities[name] for name, path in paths.items())
    report["ready_ms"] = {engine:fc.summary([row["ready_ms"] for item in report["batches"] if item["engine"] == engine
        for row in item["samples"] if row["success"] and row["cleanup_success"]]) for engine in ("hypermachine", "firecracker")}
    if args.arrival_rate is not None:
        report["scheduled_ready_ms"] = {engine:fc.summary([row["scheduled_to_validation_ms"]
            for item in report["batches"] if item["engine"] == engine for row in item["samples"]
            if row["success"] and row["cleanup_success"]]) for engine in ("hypermachine", "firecracker")}
    report["success"] = not report["setup_error"] and not report["cleanup_errors"] and report["artifacts_unchanged"] \
        and len(report["batches"]) == 2*args.pairs and all(item["success"] for item in report["batches"])
    print(json.dumps(report, indent=2))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
