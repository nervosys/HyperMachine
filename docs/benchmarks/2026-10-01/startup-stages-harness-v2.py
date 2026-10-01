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


def main():
    original_popen = burst.subprocess.Popen
    original_request = burst.engines.request
    original_hm = burst.hm_attempt
    original_memory = burst.engines.memory
    collect_guest = "--collect-guest-boot" in sys.argv
    if collect_guest: sys.argv.remove("--collect-guest-boot")
    local = threading.local()
    active = {}
    lock = threading.Lock()
    node = {}
    observations = []
    with tempfile.TemporaryDirectory(prefix="hm-startup-diagnostic-", dir="/var/tmp") as directory:
        retained = Path(directory)/"node.log"
        def launch(command, *args, **kwargs):
            if Path(command[0]).name == "hv2-sandboxd" and "--no-template" in command:
                kwargs["env"] = {**kwargs["env"], "RUST_LOG":"hv2_sandboxd=debug"}
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
                for sandbox, identity in chosen:
                    if identity["index"] % 10: continue
                    response = original_request(node["url"], "POST", f"/sandboxes/{sandbox}/exec",
                        {"cmd":"printf 'UPTIME\\n'; cat /proc/uptime; printf 'DMESG\\n'; dmesg", "timeout_secs":10})
                    if response.get("exit_code") != 0 or response.get("timed_out") or response.get("truncated"):
                        raise RuntimeError("guest boot diagnostic failed")
                    observations.append({"sandbox_id":sandbox, **identity, "response":response})
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
        report["hypermachine_log_filter"] = "hv2_sandboxd=debug"
        report["diagnostic_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        report["guest_boot_collected"] = collect_guest
        report["guest_observations"] = observations
        report["guest_probe_scope"] = "Every tenth arrival index, after all guests validate while held; memory readings follow these extra commands"
        if retained.stat().st_size > 4*1024*1024: raise RuntimeError("diagnostic log exceeds 4 MiB limit")
        report["startup_stages_ms"] = stages(retained.read_text(errors="replace"))
        rows = [row for batch in report["batches"] if batch["engine"] == "hypermachine" for row in batch["samples"]]
        report["stage_ids_match_passed_requests"] = set(report["startup_stages_ms"]) == {
            row["sandbox_id"] for row in rows if row["success"]}
        report["success"] = report["success"] and report["stage_ids_match_passed_requests"]
        print(json.dumps(report, indent=2))
        return code if report["success"] else 1


if __name__ == "__main__": raise SystemExit(main())
