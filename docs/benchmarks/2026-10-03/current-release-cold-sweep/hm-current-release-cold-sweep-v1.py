from pathlib import Path
import os,json,subprocess,hashlib,time
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-current-release-cold-sweep-v1');out.mkdir(exist_ok=False)
os.sched_setaffinity(0,set(range(8)))
inputs={'hypermachine':'/var/tmp/hm-current-competitive-release-node-v1','firecracker':'/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','kernel':'/var/tmp/hm-competitive/bzImage-known-uart-irq','initrd':'/var/tmp/hm-competitive/guest-output-drain.cpio.gz'}
expected={'firecracker':'99ad0f5cd0514a88aad0e9ae8cfdb3cc3b4ab9d190e1194602406c786b5de7a5','kernel':'afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd','initrd':'1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'}
for name,value in expected.items():assert hashlib.sha256(Path(inputs[name]).read_bytes()).hexdigest()==value,name
for concurrency,pairs in [(1,4),(8,4),(50,2),(100,2)]:
 command=['python3',str(root/'tools/bench-local-engines-concurrent.py'),'--environment','owned WSL KVM, optimized current isolated daemon, same 8-CPU affinity, one vCPU and 1024 MiB per guest, cold-start budget disabled','--concurrency',str(concurrency),'--pairs',str(pairs),'--memory-idle-seconds','5']
 for name,path in inputs.items():command+=['--'+name,path]
 print('Starting C'+str(concurrency),flush=True)
 with (out/('c'+str(concurrency)+'.json')).open('x') as stdout, (out/('c'+str(concurrency)+'.stderr')).open('x') as stderr:
  result=subprocess.run(command,stdout=stdout,stderr=stderr)
 (out/('c'+str(concurrency)+'-exit.json')).write_text(json.dumps({'exit_code':result.returncode,'argv':command,'driver_affinity':sorted(os.sched_getaffinity(0))},indent=2)+'\n')
 print('Terminal C'+str(concurrency)+' exit '+str(result.returncode),flush=True)
