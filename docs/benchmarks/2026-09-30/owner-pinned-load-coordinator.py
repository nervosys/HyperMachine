import hashlib,json,os,subprocess,sys,time
from pathlib import Path
root=Path("/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine")
output=Path("/var/tmp/hm-competitive/local-engines-owner-pinned-load-100.json")
worker_code="import time\nend=time.monotonic()+7200\nx=1\nwhile time.monotonic()<end:\n for _ in range(10000): x=(x*1664525+1013904223)&0xffffffff\n"
core=min(os.sched_getaffinity(0))
workers=[]
cleanup=True
profile_valid=False
report=None
try:
 for _ in range(1):
  workers.append(subprocess.Popen([sys.executable,"-c",worker_code],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,preexec_fn=lambda: os.sched_setaffinity(0,{core})))
 time.sleep(.2)
 if any(worker.poll() is not None for worker in workers):raise RuntimeError("CPU worker failed to start")
 command=[sys.executable,str(root/"tools/bench-local-engines.py"),"--hypermachine","/var/tmp/hm-competitive-target/release/hv2-sandboxd","--firecracker","/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64","--kernel","/var/tmp/hm-competitive/bzImage","--initrd","/var/tmp/hm-competitive/guest-output-drain.cpio.gz","--pairs","100","--environment","shared-WSL-nested-KVM-one-pinned-CPU-worker-owner-state-diagnostic"]
 result=subprocess.run(command,capture_output=True,text=True,preexec_fn=lambda: os.sched_setaffinity(0,{core}))
 report=json.loads(result.stdout)
 profile_valid=all(worker.poll() is None for worker in workers)
 report["controlled_cpu_load"]={"workers":1,"worker_code":worker_code,"all_alive_through_cohort":profile_valid,"pinning":True,"cpu":core}
finally:
 for worker in workers:
  if worker.poll() is None:
   worker.terminate()
   try:worker.wait(timeout=5)
   except subprocess.TimeoutExpired:worker.kill();worker.wait(timeout=5)
  cleanup=cleanup and worker.poll() is not None
 if report is not None:
  report["controlled_cpu_load"]["workers_cleaned_up"]=cleanup
  report["controlled_cpu_load"]["coordinator_sha256"]=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
  report["success"]=report["success"] and profile_valid and cleanup
  output.write_text(json.dumps(report,indent=2))
  print(json.dumps({"success":report["success"],"cpu_load":report["controlled_cpu_load"],"ready_ms":report["ready_ms"],"failures":[{k:row.get(k) for k in ("pair","engine","error")} for row in report["samples"] if not row["success"]]}))
