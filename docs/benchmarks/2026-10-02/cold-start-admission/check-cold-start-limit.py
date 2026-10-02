#!/usr/bin/env python3
"""Verify cold-boot admission bounds and failed-boot permit release on owned VMs."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request


def require(condition, message):
    if not condition: raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1",0))
        return probe.getsockname()[1]


def admissions(log):
    log = re.sub(r"\x1b\[[0-9;]*m", "", log)
    active, maximum, acquired, released = set(), 0, set(), set()
    for line in log.splitlines():
        if "cold boot admitted" not in line and "cold boot admission released" not in line: continue
        match = re.search(r'\bvm="?(sbx-[A-Za-z0-9]+)"?',line)
        require(match is not None, "admission identity missing")
        name = match[1]
        if "cold boot admitted" in line:
            require(name not in acquired, "duplicate admission")
            acquired.add(name); active.add(name)
            maximum = max(maximum,len(active))
        else:
            require(name in active and name not in released, "unmatched release")
            active.remove(name); released.add(name)
    require(not active and acquired == released, "unreleased admission")
    return {"acquired":len(acquired),"released":len(released),"maximum_observed":maximum}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon","kernel","initrd","no-agent-initrd","output"]:
        parser.add_argument("--"+name,type=Path,required=True)
    parser.add_argument("--restore-bypass",action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    paths = {name:getattr(args,name).resolve(strict=True) for name in ["daemon","kernel","initrd","no_agent_initrd"]}
    identities = {name:digest(path) for name,path in paths.items()}
    identities["coordinator"] = digest(Path(__file__))
    report = {"success":False,"purpose":"owned functional verification; no performance comparison",
        "artifact_sha256":identities,"checks":[],"cleanup_errors":[],"processes_stopped":[]}
    credential = secrets.token_urlsafe(32)
    env = {"PATH":"/usr/local/bin:/usr/bin:/bin","RUST_LOG":"warn,hv2_sandboxd::cold_admission=debug",
        "HV2_KERNEL":str(paths["kernel"]),"HV2_CLUSTER_TOKEN":credential}
    try:
        for invalid in ["0","1025","-1","bad"]:
            result = subprocess.run([str(paths["daemon"]),"--cold-start-concurrency",invalid],
                env=dict(env,HV2_INITRD=str(paths["initrd"])),capture_output=True,timeout=10)
            require(result.returncode != 0 and b"requires 1..1024" in result.stderr, "invalid limit accepted")
            (args.output/("invalid-"+invalid+".log")).write_bytes(result.stdout+result.stderr)
            report["checks"].append("invalid limit "+invalid)
        for phase,limit,image in [("bounded",2,paths["initrd"]),("failure-release",1,paths["no_agent_initrd"])]:
            process = None
            guests = set()
            with tempfile.TemporaryDirectory(prefix="hm-cold-admission-",dir="/var/tmp") as temporary:
                temporary = Path(temporary)
                phase_image = image
                if phase == "failure-release" and args.restore_bypass:
                    phase_image = temporary/"base-initrd.cpio.gz"
                api,proxy = port(),port()
                while proxy == api: proxy = port()
                base = f"http://127.0.0.1:{api}"
                log_path = args.output/(phase+".log")

                def request(method,path,body=None):
                    data = None if body is None else json.dumps(body).encode()
                    req = urllib.request.Request(base+path,method=method,data=data,
                        headers={"x-hv2-cluster-token":credential,"content-type":"application/json"})
                    try: response = urllib.request.urlopen(req,timeout=45)
                    except urllib.error.HTTPError as error: response = error
                    with response:
                        raw = response.read()
                        return response.status,json.loads(raw) if raw else None

                def create(template="base",expected=201):
                    status,value = request("POST","/v2/sandboxes",{"templateID":template,"timeout":120})
                    require(status == expected,("create",status,expected))
                    if expected == 201:
                        name = value["sandboxID"]
                        guests.add(name)
                        status,value = request("GET",f"/sandboxes/{name}")
                        require(status == 200,"guest detail failed")
                        require(value["cpuCount"] == 1 and value["memoryMB"] == 1024,"resource mismatch")
                        status,value = request("POST",f"/sandboxes/{name}/exec",{"cmd":"printf admission-ok","timeout_secs":10})
                        require(status == 200 and value["exit_code"] == 0 and value["stdout"] == "admission-ok"
                            and not value.get("timed_out") and not value.get("truncated"),"guest command failed")
                        return name

                try:
                    with log_path.open("wb") as log:
                        command = [str(paths["daemon"]),"--port",str(api),"--proxy-port",str(proxy),
                            "--memory-mb","1024","--cpu-cores","1","--capacity","16",
                            "--cold-start-concurrency",str(limit),"--template","healthy="+str(paths["initrd"]),
                            "--volume-dir",str(temporary/"volumes"),"--snapshot-store",str(temporary/"snapshots")]
                        if phase != "failure-release" or not args.restore_bypass: command.append("--no-template")
                        process = subprocess.Popen(command,
                            env=dict(env,HV2_INITRD=str(phase_image)),stdin=subprocess.DEVNULL,stdout=log,stderr=log)
                        deadline = time.monotonic()+30
                        while True:
                            require(process.poll() is None,"owned daemon exited during startup")
                            try:
                                if request("GET","/templates")[0] == 200: break
                            except OSError: pass
                            require(time.monotonic()<deadline,"readiness timeout")
                            time.sleep(.02)
                        if phase == "bounded":
                            barrier = threading.Barrier(8)
                            def attempt(_):
                                barrier.wait(timeout=10)
                                return create()
                            with ThreadPoolExecutor(max_workers=8) as pool:
                                ids = list(pool.map(attempt,range(8)))
                            require(len(set(ids)) == 8,"duplicate guest IDs")
                        else:
                            if args.restore_bypass:
                                # Base intentionally had no image during startup,
                                # avoiding a long template-build failure. Install
                                # only this fixture's cold image once listening.
                                shutil.copy2(image,phase_image)
                                warm = create("healthy")
                                require(request("POST",f"/sandboxes/{warm}/pause",{})[0] == 204,"pause failed")
                                before = log_path.read_text().count("cold boot admitted")
                                with ThreadPoolExecutor(max_workers=1) as pool:
                                    failure = pool.submit(create,expected=503)
                                    deadline = time.monotonic()+5
                                    while log_path.read_text().count("cold boot admitted") <= before:
                                        require(time.monotonic()<deadline,"fault boot was not admitted")
                                        time.sleep(.02)
                                    resumed_at = time.monotonic()
                                    require(request("POST",f"/sandboxes/{warm}/resume",{"timeout":120})[0] == 201,"resume failed")
                                    require(time.monotonic()-resumed_at < 5,"restore queued behind cold boot")
                                    status,value = request("POST",f"/sandboxes/{warm}/exec",{"cmd":"printf restored-ok","timeout_secs":10})
                                    require(status == 200 and value["stdout"] == "restored-ok" and value["exit_code"] == 0,"restored guest failed")
                                    failure.result()
                                require(len(request("GET","/sandboxes")[1]) == 1,"failed boot retained guest")
                                report["restore_bypass_verified"] = True
                                # Change only the fixture-owned cold image after
                                # the failed VM stopped; the prepared healthy
                                # template remains available for restore checks.
                                report["owned_cold_image_replacement"] = {"before":digest(phase_image),"after":digest(paths["initrd"])}
                                shutil.copy2(paths["initrd"],phase_image)
                            else:
                                create(expected=503)
                                require(request("GET","/sandboxes")[1] == [],"failed boot retained guest")
                            create("base" if args.restore_bypass else "healthy")
                        for name in list(guests):
                            require(request("DELETE",f"/sandboxes/{name}")[0] == 204,"guest delete failed")
                            guests.remove(name)
                        require(request("GET","/sandboxes")[1] == [],"guest inventory not empty")
                finally:
                    if process is not None:
                        if process.poll() is None:
                            for name in list(guests):
                                try: request("DELETE",f"/sandboxes/{name}")
                                except Exception as error: report["cleanup_errors"].append(str(error))
                            process.terminate()
                            try: process.wait(timeout=5)
                            except subprocess.TimeoutExpired:
                                process.kill(); process.wait(timeout=5)
                        report["processes_stopped"].append({"phase":phase,"exit_code":process.returncode})
                raw_log = log_path.read_bytes()
                require(credential.encode() not in raw_log,"credential in fixture log")
                bounds = admissions(raw_log.decode(errors="replace"))
                expected = 8 if phase == "bounded" else 2
                require(bounds["acquired"] == bounds["released"] == expected,"admission count mismatch")
                require(bounds["maximum_observed"] == limit,"admission bound mismatch")
                report[phase] = bounds
                report["checks"].append(phase)
        report["artifacts_unchanged"] = all(digest(path) == identities[name] for name,path in paths.items())
        report["success"] = report["artifacts_unchanged"] and not report["cleanup_errors"] and len(report["processes_stopped"]) == 2
    except Exception as error:
        report["error"] = str(error)
    (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report))
    return 0 if report["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
