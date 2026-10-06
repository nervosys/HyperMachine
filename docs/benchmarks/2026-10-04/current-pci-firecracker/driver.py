from pathlib import Path
import os,json,hashlib,tempfile,subprocess,shutil,time
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');identity=json.loads((root/'docs/benchmarks/2026-10-04/pci-fastboot/source-context.json').read_text())
paths={'hypermachine':Path('/var/tmp/hm-pci-fastboot-release-node-v1'),'firecracker':Path('/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64'),'kernel':Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),'initrd':Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')}
expected={'hypermachine':identity['candidate_binary_sha256'],'firecracker':'99ad0f5cd0514a88aad0e9ae8cfdb3cc3b4ab9d190e1194602406c786b5de7a5','kernel':'afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd','initrd':'1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'}
for name,path in paths.items():assert hashlib.sha256(path.read_bytes()).hexdigest()==expected[name]
assert hashlib.sha256((root/'crates/hv2-sandboxd/src/main.rs').read_bytes()).hexdigest()==identity['main_candidate_sha256']
os.sched_setaffinity(0,set(range(8)));out=Path(tempfile.mkdtemp(prefix='hm-pci-current-firecracker-v1-',dir='/var/tmp'));print(out,flush=True)
for name in ['bench-local-engines-concurrent.py','bench-local-engines.py','bench-firecracker-local.py']:shutil.copyfile(root/'tools'/name,out/name)
shutil.copyfile(root/'docs/benchmarks/2026-10-04/current-release-firecracker-c1/analysis.py',out/'analysis.py');shutil.copyfile(root/'docs/benchmarks/2026-10-04/pci-fastboot/source-context.json',out/'source-context.json');shutil.copyfile(__file__,out/'driver.py')
context={'inputs_sha256':expected,'affinity':list(range(8)),'profiles':[1,8],'pairs':8,'memory_idle_seconds':5,'daemon_guest_transport':'pci'};(out/'context.json').write_text(json.dumps(context,indent=2)+'\n');terminal=[]
for concurrency in context['profiles']:
 directory=out/('c'+str(concurrency));directory.mkdir()
 argv=['python3',str(out/'bench-local-engines-concurrent.py'),'--environment','owned WSL KVM current PCI release; matched kernel/output-drain guest, eight CPU affinity, no templates or admission budget','--daemon-guest-transport','pci','--concurrency',str(concurrency),'--pairs','8','--memory-idle-seconds','5']
 for name,path in paths.items():argv.extend(['--'+name,str(path)])
 started=time.time()
 with (directory/'report.json').open('w') as stdout,(directory/'stderr.txt').open('w') as stderr:result=subprocess.run(argv,stdout=stdout,stderr=stderr)
 terminal.append({'concurrency':concurrency,'argv':argv,'exit_code':result.returncode,'started_unix':started,'ended_unix':time.time()});(out/'terminal.json').write_text(json.dumps(terminal,indent=2)+'\n');print('C'+str(concurrency),'terminal',result.returncode,flush=True)
 if result.returncode:raise SystemExit(result.returncode)
for name,path in paths.items():assert hashlib.sha256(path.read_bytes()).hexdigest()==expected[name]
print('Both profiles completed; frozen inputs unchanged.',flush=True)
