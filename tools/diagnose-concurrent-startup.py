#!/usr/bin/env python3
"""Use the concurrent benchmark CLI to collect matched HM startup-stage diagnostics.

This wrapper leaves scored harness files unchanged. It enables daemon-only
debug tracing, retains the owned log inode, and associates API IDs with rows.
Its timings are diagnostic and must not be used as competitor benchmark wins.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import threading
import time

spec = importlib.util.spec_from_file_location("burst", Path(__file__).with_name("bench-local-engines-concurrent.py"))
burst = importlib.util.module_from_spec(spec)
spec.loader.exec_module(burst)


def stages(log):
    log = re.sub(r"\x1b\[[0-9;]*m", "", log)
    duration = r"([0-9.]+)(ns|µs|ms|s)"
    pattern = re.compile(r"(sbx-[A-Za-z0-9]+) up in " + duration
        + r": build " + duration + r", launch " + duration
        + r", agent answering " + duration + r",\s+network and envd " + duration)
    scale = {"ns":.000001,"µs":.001,"ms":1,"s":1000}
    result = {}
    for match in pattern.finditer(log):
        values = match.groups()
        if values[0] in result: raise RuntimeError("duplicate sandbox startup log")
        result[values[0]] = {name:float(values[1+2*i])*scale[values[2+2*i]]
            for i,name in enumerate(["total_ms","build_ms","launch_ms","agent_ms","network_envd_ms"])}
    return result


def cold_stages(log):
    result = {}
    for line in re.sub(r"\x1b\[[0-9;]*m", "", log).splitlines():
        if "cold guest readiness stages" not in line: continue
        identity = re.search(r'\bvm="?(sbx-[A-Za-z0-9]+)"?', line)
        phase = re.search(r'\bphase="?(connect|ping)"?', line)
        succeeded = re.search(r'\bsucceeded=(true|false)\b', line)
        if not identity or not phase or not succeeded:
            raise RuntimeError("incomplete cold readiness identity")
        name = identity[1]
        if name in result: raise RuntimeError("duplicate cold readiness log")
        row = {"phase":phase[1], "succeeded":succeeded[1] == "true"}
        for field in ["blocking_queue_ms", "connect_ms"] + (["ping_ms"] if phase[1] == "ping" else []):
            value = re.search(r"\b" + field + r"=([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)\b", line)
            if not value: raise RuntimeError("incomplete cold readiness duration")
            row[field] = float(value[1])
        result[name] = row
    return result


def dispatch_stages(log):
    result = {}
    for line in re.sub(r"\x1b\[[0-9;]*m", "", log).splitlines():
        if "VM background dispatch" in line:
            fields = ["dispatch_queue_ms"]
        elif "vCPU owner thread entry" in line:
            fields = ["wrapper_queue_ms", "thread_start_ms"]
            if not re.search(r'\bvcpu_id=0\b', line):
                raise RuntimeError("dispatch diagnostic requires exactly one vCPU")
        else: continue
        identity = re.search(r'\bvm="?(sbx-[A-Za-z0-9]+)"?', line)
        if not identity: raise RuntimeError("incomplete dispatch identity")
        row = result.setdefault(identity[1], {})
        for field in fields:
            value = re.search(r"\b" + field + r"=([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)\b", line)
            if not value: raise RuntimeError("incomplete dispatch duration")
            if field in row: raise RuntimeError("duplicate dispatch stage")
            row[field] = float(value[1])
    return result


def main():
    original_popen = burst.subprocess.Popen
    original_request = burst.engines.request
    original_hm = burst.hm_attempt
    original_memory = burst.engines.memory
    collect_guest = "--collect-guest-boot" in sys.argv
    if collect_guest: sys.argv.remove("--collect-guest-boot")
    collect_cold = "--collect-cold-readiness" in sys.argv
    if collect_cold: sys.argv.remove("--collect-cold-readiness")
    collect_dispatch = "--collect-dispatch" in sys.argv
    if collect_dispatch:
        sys.argv.remove("--collect-dispatch")
        collect_cold = True
    log_filter = "hv2_sandboxd=debug" + (",hv2_agent::cold_readiness=debug" if collect_cold else "")
    if collect_dispatch: log_filter += ",hv2_core::cold_dispatch=debug"
    local = threading.local()
    active = {}
    lock = threading.Lock()
    node = {}
    observations = []
    with tempfile.TemporaryDirectory(prefix="hm-startup-diagnostic-", dir="/var/tmp") as directory:
        retained = Path(directory)/"node.log"
        def launch(command, *args, **kwargs):
            if Path(command[0]).name == "hv2-sandboxd" and "--no-template" in command:
                kwargs["env"] = {**kwargs["env"], "RUST_LOG":log_filter}
                os.link(kwargs["stdout"].name, retained)
            process = original_popen(command, *args, **kwargs)
            if Path(command[0]).name == "hv2-sandboxd" and "--no-template" in command:
                node.update(pid=process.pid, url="http://127.0.0.1:"+command[command.index("--port")+1])
            return process
        def request(url, method, path, body=None):
            value = original_request(url, method, path, body)
            if method == "POST" and path == "/v2/sandboxes": local.sandbox = value.get("sandboxID")
            if method == "DELETE" and path.startswith("/sandboxes/"):
                with lock: active.pop(path.rsplit("/",1)[1], None)
            return value
        def attempt(args, row, start, ready):
            local.sandbox = None
            def identified(value, pid):
                value["batch_start_perf_seconds"] = start
                if local.sandbox is not None:
                    value["sandbox_id"] = local.sandbox
                    with lock: active[local.sandbox] = {"pair":value["pair"], "index":value["index"]}
                ready(value, pid)
            return original_hm(args, row, start, identified)
        def memory(pid):
            # The batch driver calls this after its readiness barrier, while
            # every guest remains held. Probes cannot extend measured boot.
            if collect_guest and pid == node.get("pid"):
                with lock: chosen = sorted(active.items(), key=lambda item:item[1]["index"])
                batch_observations = []
                for sandbox, identity in chosen:
                    if identity["index"] % 10: continue
                    probe_started = time.perf_counter()
                    response = original_request(node["url"], "POST", f"/sandboxes/{sandbox}/exec",
                        {"cmd":"printf 'UPTIME\\n'; cat /proc/uptime; printf 'DMESG\\n'; dmesg", "timeout_secs":10})
                    probe_finished = time.perf_counter()
                    if response.get("exit_code") != 0 or response.get("timed_out") or response.get("truncated"):
                        raise RuntimeError("guest boot diagnostic failed")
                    observations.append({"sandbox_id":sandbox, **identity, "response":response,
                        "host_start_perf_seconds":probe_started, "host_end_perf_seconds":probe_finished})
                    batch_observations.append(observations[-1])
                # A second bounded observation checks elapsed-clock consistency
                # before interpreting uptime as a host/guest clock alignment.
                for observation in batch_observations:
                    probe_started = time.perf_counter()
                    response = original_request(node["url"], "POST",
                        f"/sandboxes/{observation['sandbox_id']}/exec",
                        {"cmd":"printf 'UPTIME\\n'; cat /proc/uptime", "timeout_secs":10})
                    probe_finished = time.perf_counter()
                    if response.get("exit_code") != 0 or response.get("timed_out") or response.get("truncated"):
                        raise RuntimeError("guest clock diagnostic failed")
                    observation["repeat_clock_probe"] = {"response":response,
                        "host_start_perf_seconds":probe_started, "host_end_perf_seconds":probe_finished}
            return original_memory(pid)
        burst.subprocess.Popen = launch
        burst.engines.request = request
        burst.hm_attempt = attempt
        burst.engines.memory = memory
        output = io.StringIO()
        try:
            with contextlib.redirect_stdout(output): code = burst.main()
        finally:
            burst.subprocess.Popen = original_popen
            burst.engines.request = original_request
            burst.hm_attempt = original_hm
            burst.engines.memory = original_memory
        report = json.loads(output.getvalue())
        report["diagnostic_only"] = True
        report["hypermachine_log_filter"] = log_filter
        report["diagnostic_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        report["guest_boot_collected"] = collect_guest
        report["guest_observations"] = observations
        report["guest_probe_scope"] = "Every tenth arrival index, after all guests validate while held; memory readings follow these extra commands"
        if retained.stat().st_size > 4*1024*1024: raise RuntimeError("diagnostic log exceeds 4 MiB limit")
        log = retained.read_text(errors="replace")
        report["startup_stages_ms"] = stages(log)
        if collect_cold:
            report["cold_readiness_log"] = log
            report["cold_readiness_stages_ms"] = cold_stages(log)
        if collect_dispatch: report["dispatch_stages_ms"] = dispatch_stages(log)
        rows = [row for batch in report["batches"] if batch["engine"] == "hypermachine" for row in batch["samples"]]
        report["stage_ids_match_passed_requests"] = set(report["startup_stages_ms"]) == {
            row["sandbox_id"] for row in rows if row["success"]}
        report["success"] = report["success"] and report["stage_ids_match_passed_requests"]
        if collect_cold:
            report["cold_ids_match_passed_requests"] = {
                name for name, row in report["cold_readiness_stages_ms"].items() if row["succeeded"]
            } == {row["sandbox_id"] for row in rows if row["success"]}
            report["success"] = report["success"] and report["cold_ids_match_passed_requests"]
        if collect_dispatch:
            report["dispatch_ids_match_passed_requests"] = set(report["dispatch_stages_ms"]) == {
                row["sandbox_id"] for row in rows if row["success"]} and all(
                    set(stage) == {"dispatch_queue_ms", "wrapper_queue_ms", "thread_start_ms"}
                    for stage in report["dispatch_stages_ms"].values())
            report["success"] = report["success"] and report["dispatch_ids_match_passed_requests"]
        print(json.dumps(report, indent=2))
        return code if report["success"] else 1


if __name__ == "__main__": raise SystemExit(main())
