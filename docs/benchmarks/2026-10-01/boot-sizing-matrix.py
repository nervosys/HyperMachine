import hashlib, json, subprocess, sys, time
from pathlib import Path
root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
out = Path('/var/tmp/hm-boot-sizing')
coordinator = root/'target/boot-sizing-coordinator.py'
manifest = {'baseline_commit':'5c91bef', 'matrix_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'coordinator_sha256':hashlib.sha256(coordinator.read_bytes()).hexdigest(), 'cohorts':[]}
for concurrency, blocks in ((8,2),(100,4)):
    for block in range(blocks):
        profiles = ('baseline','sized') if block % 2 == 0 else ('sized','baseline')
        for profile in profiles:
            name = f'boot-sizing-c{concurrency}-block{block}-{profile}'
            started = time.monotonic()
            result = subprocess.run([sys.executable,str(coordinator),'--concurrency',str(concurrency),'--pairs','2','--name',name,'--hypermachine',str(out/f'hv2-sandboxd-{profile}')],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            path=out/(name+'.json')
            entry={'concurrency':concurrency,'block':block,'profile':profile,'exit_code':result.returncode,'duration_seconds':time.monotonic()-started,'report_sha256':hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None,'stdout':result.stdout,'stderr':result.stderr}
            manifest['cohorts'].append(entry)
            (out/'matrix.json').write_text(json.dumps(manifest,indent=2))
            print(json.dumps({key:entry[key] for key in ('concurrency','block','profile','exit_code','duration_seconds')}),flush=True)