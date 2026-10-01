import hashlib, json, subprocess, sys, time
from pathlib import Path
root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
manifest = {'experiment':'counterbalanced-default-vs-arena2-c100','blocks':4,'pairs_per_profile':2,'profiles':[], 'matrix_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
for block in range(4):
    order = ['default','arena2'] if block % 2 == 0 else ['arena2','default']
    for profile in order:
        name = f'allocator-block-{block}-{profile}'
        command = [sys.executable,str(root/'target/allocator-coordinator.py'),'--concurrency','100','--pairs','2','--name',name]
        if profile == 'arena2': command += ['--arena-max','2']
        print('Starting '+name,flush=True)
        start = time.monotonic()
        run = subprocess.run(command)
        path = Path('/var/tmp/hm-competitive')/(name+'.json')
        row = {'block':block,'profile':profile,'exit_code':run.returncode,'duration_seconds':time.monotonic()-start,'file':path.name,'raw_sha256':hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None}
        manifest['profiles'].append(row)
        Path('/var/tmp/hm-competitive/allocator-blocks.json').write_text(json.dumps(manifest,indent=2)+'\n')
        print('Completed '+name+' exit '+str(run.returncode),flush=True)
raise SystemExit(0 if all(row['exit_code']==0 for row in manifest['profiles']) else 1)