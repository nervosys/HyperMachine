#!/usr/bin/env python3
"""Counterbalanced static GNU threshold cold-boot comparison with FC controls.

Sets only MALLOC_MMAP_THRESHOLD_=131072 on candidate owned HyperMachine daemons.
No preload, runtime probe, explicit trim, guest-memory or snapshot format change.
"""
import argparse
import contextlib
import io
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time


def require(value, message):
    if not value:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("daemon", "firecracker", "kernel", "initrd", "daemon-context", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=2)
    parser.add_argument("--concurrency", type=int, default=8)
    args = parser.parse_args()
    require(platform.system() == "Linux" and platform.libc_ver()[0] == "glibc", "requires Linux GNU libc")
    require(1 <= args.pairs <= 8 and 1 <= args.concurrency <= 100 and not args.output.exists(), "invalid or existing profile")
    require(not any(os.environ.get(name) for name in ("LD_PRELOAD", "LD_DEBUG", "GLIBC_TUNABLES"))
            and not any(name.startswith("MALLOC_") for name in os.environ),
            "parent allocator environment is contaminated")
    spec = importlib.util.spec_from_file_location("mmap_prepared", Path(__file__).with_name("bench-local-engines-concurrent.py"))
    warm = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(warm)
    paths = {name: getattr(args, name).resolve(strict=True) for name in ("daemon", "firecracker", "kernel", "initrd", "daemon_context")}
    paths.update(driver=Path(__file__).resolve(), coordinator=Path(warm.__file__).resolve(),
                 engines=Path(warm.engines.__file__).resolve(), firecracker_harness=Path(warm.fc.__file__).resolve())
    hashes = {name: warm.engines.digest(path) for name, path in paths.items()}
    context = json.loads(paths["daemon_context"].read_bytes())
    require(context["baseline_sha256"] == hashes["daemon"] and context["build_exit_code"] == 0,
            "accepted daemon binding differs")
    affinity = sorted(os.sched_getaffinity(0))[:8]
    require(len(affinity) == 8, "requires eight available host CPUs")
    os.sched_setaffinity(0, affinity)
    os.umask(0o077)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"success": False, "same_binary": True, "runtime_change_adopted": False,
        "managed_competitor_win_established": False, "intervention": "static_glibc_mmap_threshold_131072",
        "candidate_environment": {"MALLOC_MMAP_THRESHOLD_": "131072"}, "cpu_affinity": affinity,
        "pairs": args.pairs, "concurrency": args.concurrency, "libc": list(platform.libc_ver()),
        "artifact_sha256": hashes, "runs": [],
        "order": "fresh-daemon baseline/candidate AB/BA; two internal cold HM/FC AB/BA pairs",
        "limitations": ["Shared WSL nested KVM and uncontrolled host background load",
            "Native cold create-to-command path; no dropped-cache or managed-platform comparison",
            "PSS excludes kernel memory and unmapped page cache; no fleet-density result",
            "Same executable; environment affects GNU allocation/reclamation policy, not proven allocation ownership",
            "No runtime logging/probe/preload/explicit trim; startup verification excluded from cold create-to-command"]}
    original_argv = sys.argv
    try:
        for pair in range(args.pairs):
            for variant in (("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")):
                path = args.output.parent / f"{pair}-{variant}.json"
                require(not path.exists(), "preserve nested evidence")
                row = {"pair": pair, "variant": variant, "success": False, "activations": []}
                owned_nodes = []
                original_launch = warm.subprocess.Popen
                def launch(command, *positional, **kwargs):
                    owned = Path(command[0]).resolve() == paths["daemon"]
                    if owned:
                        require(not row["activations"], "unexpected extra daemon launch")
                        environment = dict(kwargs["env"])
                        require(not any(key.startswith("MALLOC_") or key in ("GLIBC_TUNABLES", "LD_PRELOAD", "LD_DEBUG")
                                        for key in environment), "baseline daemon allocator environment contaminated")
                        if variant == "candidate":
                            environment.update(report["candidate_environment"])
                        kwargs["env"] = environment
                    process = original_launch(command, *positional, **kwargs)
                    if owned:
                        owned_nodes.append(process)
                        # This runs before source preparation, outside all guest timers.
                        deadline = time.monotonic() + 3
                        observations = 0
                        while True:
                            require(process.poll() is None, "owned daemon exited before activation verification")
                            observations += 1
                            try:
                                executable_matches = warm.engines.digest(Path(f"/proc/{process.pid}/exe")) == hashes["daemon"]
                                raw = Path(f"/proc/{process.pid}/environ").read_bytes() if executable_matches else b""
                                require(len(raw) <= 65536, "owned daemon environment exceeds limit")
                                actual = dict(item.decode().split("=", 1) for item in raw.split(b"\0") if item)
                                if executable_matches and actual == environment:
                                    break
                            except OSError:
                                pass
                            require(time.monotonic() < deadline, "running daemon executable/environment did not match before timeout")
                            time.sleep(0.01)
                        row["activations"].append({"pid": process.pid, "environment": actual,
                                                   "executable_sha256": hashes["daemon"],
                                                   "preparation_observations": observations})
                    return process
                warm.subprocess.Popen = launch
                sys.argv = ["bench-local-engines-concurrent.py", "--pairs", "2", "--concurrency", str(args.concurrency),
                            "--hypermachine", str(paths["daemon"]), "--environment", "same-binary-glibc-threshold-cold",
                            "--memory-idle-seconds", "5", "--cold-start-concurrency", "16"]
                for name in ("firecracker", "kernel", "initrd"):
                    sys.argv += ["--" + name, str(paths[name])]
                try:
                    captured = io.StringIO()
                    with contextlib.redirect_stdout(captured):
                        code = warm.main()
                    path.write_text(captured.getvalue())
                    if path.exists():
                        row["cold_report"] = json.loads(path.read_bytes())
                    row["success"] = code == 0 and row.get("cold_report", {}).get("success") is True and len(row["activations"]) == 1
                except Exception as error:
                    row["error"] = str(error)
                finally:
                    warm.subprocess.Popen = original_launch
                    for process in owned_nodes:
                        if process.poll() is None:
                            warm.engines.stop(process)
                    row["owned_daemon_exit_codes"] = [process.poll() for process in owned_nodes]
                report["runs"].append(row)
                args.output.write_text(json.dumps(report, indent=2) + "\n")
                print(json.dumps({"pair": pair, "variant": variant, "success": row["success"]}), flush=True)
    finally:
        sys.argv = original_argv
    report["artifacts_unchanged"] = all(warm.engines.digest(path) == hashes[name] for name, path in paths.items())
    report["success"] = report["artifacts_unchanged"] and len(report["runs"]) == args.pairs * 2 and all(row["success"] for row in report["runs"])
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
