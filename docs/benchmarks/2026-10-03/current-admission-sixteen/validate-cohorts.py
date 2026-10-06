from pathlib import Path
import json,hashlib,importlib.util
r=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-current-admission16-v1')
p=r/'tools/analyze-cold-budget-memory.py';spec=importlib.util.spec_from_file_location('memory',p);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
paths={'baseline':Path('/var/tmp/hm-current-competitive-release-node-v1'),'candidate':Path('/var/tmp/hm-current-competitive-release-node-v1'),'kernel':Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),'initrd':Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),'harness':r/'tools/bench-cold-start-limit.py','comparison':r/'tools/bench-connection-wait.py','burst':r/'tools/bench-local-engines-concurrent.py','shared':r/'tools/bench-local-engines.py','firecracker':r/'tools/bench-firecracker-local.py'}
h=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
results={}
for cohort in ['initial','repeat']:
    j=json.loads((out/(cohort+'.json')).read_text())
    if j['pairs']!=4 or j['concurrency']!=100 or j['driver_cpu_affinity']!=list(range(8)) or j['baseline_limit']!=0 or j['candidate_limit']!=16:raise ValueError('experiment configuration differs')
    if not all(h(paths[name])==value for name,value in j['artifact_sha256'].items()):raise ValueError('input identity changed')
    if j['artifact_sha256']['baseline']!='6e4b52b1eb43b74d5e4564b2179bf1e6c9676552a0ab8b7ca1350a00eb7b5d10':raise ValueError('not current accepted release')
    q=module.analyze(j)
    if not all(v['attempted']==v['planned']==400 for v in q['variants'].values()):raise ValueError('planned attempts missing')
    if not q['cleanup_verified']:raise ValueError('cleanup failed')
    (out/(cohort+'-analysis.json')).write_text(json.dumps(q,indent=2)+'\n')
    results[cohort]=q
print(json.dumps(results,indent=2))
