import hashlib, json, subprocess, sys, time
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
folder = Path('/var/tmp/hm-competitive')
coordinator = root/'target/run-kernel-compression.py'
manifest = dict(experiment='matched-native-gzip-lz4-c100', blocks=6, pairs_per_kernel=2,
    coordinator_sha256=hashlib.sha256(coordinator.read_bytes()).hexdigest(),
    driver_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), cohorts=[])
for block in range(6):
    for profile in (['gzip','lz4'] if block % 2 == 0 else ['lz4','gzip']):
        name = f'kernel-compression-block-{block}-{profile}'
        started = time.monotonic()
        result = subprocess.run([sys.executable,str(coordinator),'--kernel-profile',profile,
            '--concurrency','100','--pairs','2','--name',name], capture_output=True,text=True)
        path = folder/(name+'.json')
        item = dict(block=block,profile=profile,file=path.name,exit_code=result.returncode,
            elapsed_seconds=time.monotonic()-started,stdout=result.stdout,stderr=result.stderr)
        if path.exists():
            raw = path.read_bytes()
            item['raw_sha256'] = hashlib.sha256(raw).hexdigest()
            item['success'] = json.loads(raw)['success']
        else: item['success'] = False
        manifest['cohorts'].append(item)
        manifest['success'] = all(row['success'] and row['exit_code'] == 0 for row in manifest['cohorts'])
        (folder/'kernel-compression-blocks.json').write_text(json.dumps(manifest,indent=2))
        print(json.dumps({key:item[key] for key in ['block','profile','exit_code','success','elapsed_seconds']}),flush=True)
manifest['complete'] = len(manifest['cohorts']) == 12
(folder/'kernel-compression-blocks.json').write_text(json.dumps(manifest,indent=2))
raise SystemExit(0 if manifest['success'] and manifest['complete'] else 1)
