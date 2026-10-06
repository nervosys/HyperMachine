from pathlib import Path
import json,hashlib,importlib.util
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
spec=importlib.util.spec_from_file_location('analysis',root/'analysis.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
c=json.loads((root/'context.json').read_text());terminal=json.loads((root/'terminal.json').read_text());source=json.loads((root/'source-context.json').read_text())
assert c['profiles']==[1,8] and c['affinity']==list(range(8)) and c['daemon_guest_transport']=='pci'
assert c['inputs_sha256']['hypermachine']==source['candidate_binary_sha256']
assert len(terminal)==2 and [t['concurrency'] for t in terminal]==[1,8] and all(t['exit_code']==0 for t in terminal)
for concurrency in c['profiles']:
 directory=root/('c'+str(concurrency));r=json.loads((directory/'report.json').read_text());s=m.analyze(directory/'report.json');assert s==json.loads((directory/'summary.json').read_text()) and s['profile_success']
 assert s['concurrency']==concurrency and s['pairs']==8
 assert all(e['passed']==e['attempted']==8*concurrency and e['memory_batches']==8 and not e['failures'] for e in s['engines'].values())
 assert r['daemon_guest_transport']=='pci' and r['daemon_argv'][r['daemon_argv'].index('--guest-transport')+1]=='pci'
 assert r['daemon_allocator_arena_max'] is None and r['daemon_allocator_mmap_threshold'] is None and r['daemon_log_filter']=='warn'
 for name,digest in c['inputs_sha256'].items():assert r['artifact_sha256'][name]==digest
 for name,field in [('bench-local-engines-concurrent.py','harness'),('bench-local-engines.py','shared_harness'),('bench-firecracker-local.py','firecracker_harness')]:assert hashlib.sha256((root/name).read_bytes()).hexdigest()==r['artifact_sha256'][field]
 for b in r['batches']:assert b['success'] and b['all_guests_validated_while_held'] and b['memory_idle_actual_seconds']>=5 and b['guest_idle_at_measurement_start_ms']['min']>=5000
 proof=json.loads((root/'boot-argument-proof.json').read_text());assert proof['passed'] and proof['binary_sha256']==c['inputs_sha256']['hypermachine'] and proof['boot_cmdline'].split()==r['common_boot_args'].split()
print('Verified C1/C8 PCI comparisons: 144 exact attempts, frozen inputs, boot-argument match, quantiles, memory, actual idle holds and cleanup.')
