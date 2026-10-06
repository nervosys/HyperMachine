from pathlib import Path
import os,json,hashlib,tempfile,subprocess,shutil,time
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');base=root/'docs/benchmarks/2026-10-04/current-pci-firecracker';prior=json.loads((base/'context.json').read_text());identity=json.loads((base/'source-context.json').read_text())
assert hashlib.sha256((root/'crates/hv2-sandboxd/src/main.rs').read_bytes()).hexdigest()==identity['main_candidate_sha256']
argv=json.loads((base/'terminal.json').read_text())[1]['argv'];argv[1]=str(root/'tools/bench-local-engines-concurrent.py');argv[argv.index('--pairs')+1]='4';argv[argv.index('--environment')+1]='owned WSL KVM PCI C8 glibc/jemalloc/jemalloc/glibc; background purge and one-second decay candidate; matched fixed inputs'
for name,digest in prior['inputs_sha256'].items():assert hashlib.sha256(Path(argv[argv.index('--'+name)+1]).read_bytes()).hexdigest()==digest
out=Path(tempfile.mkdtemp(prefix='hm-jemalloc-pci-c8-abba-v1-',dir='/var/tmp'));os.sched_setaffinity(0,set(range(8)));library=out/'libjemalloc.so.2';source=Path('/lib/x86_64-linux-gnu/libjemalloc.so.2');assert hashlib.sha256(source.read_bytes()).hexdigest()=='51952ffe97354b56197c9b765582023d435cfbe1930b0c03b778bdb5faa1fff5';shutil.copyfile(source,library)
context={'inputs_sha256':prior['inputs_sha256'],'affinity':list(range(8)),'order':[False,True,True,False],'argv_base':argv,'jemalloc_library_path':str(library),'jemalloc_library_sha256':hashlib.sha256(library.read_bytes()).hexdigest(),'jemalloc_package':'libjemalloc2:amd64 5.3.0-3','jemalloc_conf':'abort_conf:true,background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000'}
(out/'context.json').write_text(json.dumps(context,indent=2)+'\n');print(out,flush=True)
for name in ['bench-local-engines-concurrent.py','bench-local-engines.py','bench-firecracker-local.py']:shutil.copyfile(root/'tools'/name,out/name)
shutil.copyfile(base/'analysis.py',out/'analysis.py');shutil.copyfile(base/'source-context.json',out/'source-context.json');shutil.copyfile(__file__,out/'driver.py')
terminal=[]
for index,jemalloc in enumerate(context['order'],1):
 command=argv+(['--daemon-jemalloc-library',str(library)] if jemalloc else []);started=time.time()
 with (out/f'cohort-{index}.json').open('w') as stdout,(out/f'cohort-{index}.stderr.txt').open('w') as stderr:result=subprocess.run(command,stdout=stdout,stderr=stderr)
 terminal.append({'cohort':index,'jemalloc':jemalloc,'argv':command,'exit_code':result.returncode,'started_unix':started,'ended_unix':time.time()});(out/'terminal.json').write_text(json.dumps(terminal,indent=2)+'\n');print('cohort',index,'terminal',result.returncode,flush=True)
 if result.returncode:raise SystemExit(result.returncode)
assert hashlib.sha256(library.read_bytes()).hexdigest()==context['jemalloc_library_sha256']
print('ABBA complete; frozen library unchanged.',flush=True)
