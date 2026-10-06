from pathlib import Path
import hashlib,json,subprocess,sys
root=Path(__file__).resolve().parent
for name,digest in json.loads((root/'manifest.json').read_text()).items():assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
subprocess.run([sys.executable,str(root/'verify-metrics.py'),str(root/'cold')],check=True)
c=json.loads((root/'source-context.json').read_text());context=json.loads((root/'cold/context.json').read_text())
assert context['binary_sha256']=={'baseline':c['baseline_binary_sha256'],'candidate':c['candidate_binary_sha256']}
assert context['affinity']==list(range(8)) and context['operation']=='cold-create-with-exact-command' and context['template_mode']=='disabled'
assert hashlib.sha256((root/'main-before.rs').read_bytes()).hexdigest()==c['main_before_sha256']
assert hashlib.sha256((root/'main-candidate.rs').read_bytes()).hexdigest()==c['main_candidate_sha256']
for directory in (root/'cold').glob('cohort-*'):
 text=(directory/'daemon.txt').read_text();assert text.count('loaded linux boot image')==18 and 'restored from' not in text
 assert json.loads((directory/'report.json').read_text())['daemon_exit']==0
for name,count in [('pci-api',28),('mmio-api',28),('pci-network',56)]:
 r=json.loads((root/name/'report.json').read_text());assert r['passed'] and r['failure'] is None and r['daemon_exit']==0 and len(r['checks'])==count and r['binary_sha256']==c['candidate_binary_sha256']
 if name!='pci-network':
  tokens=r['boot_cmdline'].split();assert all(x in tokens for x in ['8250.nr_uarts=1','i8042.noaux','i8042.nomux','i8042.nopnp','i8042.dumbkbd']) and not any(x in tokens for x in ['pci=off','noapic','nolapic'])
 else:assert sum(x.startswith('exact NIC HTTP') for x in r['checks'])==7 and sum(x.startswith('exact guest HTTP') for x in r['checks'])==14
print('Verified cold ABBA, actual boot arguments, PCI/MMIO lifecycles, NIC/proxy delivery and cleanup.')
