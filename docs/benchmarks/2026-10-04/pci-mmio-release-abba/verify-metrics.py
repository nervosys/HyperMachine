from pathlib import Path
import json, math, sys
root=Path(sys.argv[1]);context=json.loads((root/'context.json').read_text());reports=[]
for i,transport in enumerate(['mmio','pci','pci','mmio'],1):
 r=json.loads((root/f'cohort-{i}-{transport}'/'report.json').read_text())
 assert r['passed'] and r['failure'] is None and r['transport']==transport and len(r['samples'])==18
 for j,s in enumerate(r['samples']):
  assert s['sample']==j and s['warmup']==(j<2) and s['exact_command'] and s['deleted'] and s['empty_inventory']
  assert s['before']['pid']==s['held']['pid'] and s['before']['start_ticks']==s['held']['start_ticks']
  assert s['held']['cpu_ticks']>=s['before']['cpu_ticks']
  assert s['create_ms']>0 and s['command_ms']>0 and math.isclose(s['total_ms'],s['create_ms']+s['command_ms'],rel_tol=1e-12)
 reports.append(r)
def q(values,p):return sorted(values)[math.ceil(len(values)*p)-1]
recomputed={}
for transport in ['mmio','pci']:
 values=[s for r in reports if r['transport']==transport for s in r['samples'] if not s['warmup']]
 recomputed[transport]={'samples':len(values),'latencies':{field:{'p50':q([s[field] for s in values],.5),'p95':q([s[field] for s in values],.95)} for field in ['create_ms','command_ms','total_ms']},'cpu_ms_per_operation':sum((s['held']['cpu_ticks']-s['before']['cpu_ticks'])*1000/context['hz'] for s in values)/len(values),'held_pss_kib_p50':q([s['held']['pss_kib'] for s in values],.5)}
assert recomputed==json.loads((root/'metrics.json').read_text())
print('Verified 72 exact/create/delete gates, 64 scored samples, stable process identities and recomputed metrics.')
print(json.dumps(recomputed,indent=2))
