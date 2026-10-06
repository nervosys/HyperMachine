from pathlib import Path
import json,hashlib
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
c=json.loads((root/'context.json').read_text());source=json.loads((root/'source-context.json').read_text())
assert c['cpu_affinity']==list(range(8)) and c['profiles']==['pci','mmio']
assert c['binary_sha256']=={'before':source['baseline_binary_sha256'],'after':source['candidate_binary_sha256']}
for mode in c['profiles']:
 r=json.loads((root/mode/'report.json').read_text());assert r['mode']==mode and r['passed'] and r['failure'] is None and len(r['checks'])==21 and r['daemon_exits']==[0,0] and r['owned_store_removed'] and r['paused_files_before_upgrade']
 assert all(p['size']>0 for p in r['paused_files_before_upgrade'])
 assert sum(x=='exact saved guest command' for x in r['checks'])==9
 assert 'same guest identity and access token after upgrade' in r['checks'] and 'upgraded guest fork isolation' in r['checks'] and 'empty inventory' in r['checks']
 assert all('candidate disk resume cycle '+str(i) in r['checks'] for i in range(2))
 before,after=r['cache_keys'];assert len(before)==1
 if mode=='mmio':assert before==after and 'existing MMIO template key reused' in r['checks']
 else:assert len(after)==2 and set(before)<set(after) and 'new PCI template key coexists with old base' in r['checks']
 before_log=(root/mode/'before-daemon.txt').read_text();after_log=(root/mode/'after-daemon.txt').read_text()
 assert before_log.count('loaded linux boot image')==1
 assert after_log.count('loaded linux boot image')==(1 if mode=='pci' else 0)
 assert before_log.count('restored from')==2 and after_log.count('restored from')==5
print('Verified 42 store-upgrade checks, saved guest state/identity, fork isolation, four candidate resumes, actual cache reuse and cleanup.')
