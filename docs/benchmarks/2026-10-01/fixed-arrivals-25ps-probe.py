import hashlib,json,os,subprocess
from pathlib import Path
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
cores=sorted(os.sched_getaffinity(0))[:8];os.sched_setaffinity(0,cores)
command=['python3',str(root/'tools/bench-local-engines-concurrent.py'),'--hypermachine','/var/tmp/hm-kvm-events/hv2-sandboxd-events','--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-competitive/guest-output-drain.cpio.gz','--environment','shared-WSL-nested-KVM-eight-host-CPU-affinity-no-extra-load-fixed-rate-arrivals','--pairs','2','--concurrency','8','--arrival-rate','25','--arrival-samples','100']
result=subprocess.run(command,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
out=root/'docs/benchmarks/2026-10-01'
(out/'fixed-arrivals-25ps.json').write_bytes(result.stdout)
(out/'fixed-arrivals-25ps-execution.json').write_text(json.dumps({'command':command,'exit_code':result.returncode,'stderr':result.stderr.decode(),'coordinator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'affinity':cores},indent=2)+'\n')
report=json.loads(result.stdout)
print(json.dumps({'exit_code':result.returncode,'success':report['success'],'setup_error':report['setup_error'],'cleanup_errors':report['cleanup_errors'],'ready_ms':report['ready_ms'],'scheduled_ready_ms':report['scheduled_ready_ms'],'batches':[{'engine':b['engine'],'pair':b['pair'],'passed':b['passing_attempts'],'attempts':b['attempts'],'completed_lifecycles_per_second':b['completed_lifecycles_per_second']} for b in report['batches']]}),flush=True)