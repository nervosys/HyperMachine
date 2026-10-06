from pathlib import Path
import json,math
p=Path(__file__).parent;a=json.loads((p/'analysis.json').read_text());m=json.loads((p/'metrics.json').read_text())
for path in ['private','standard','all']:
 for kind in ['baseline','candidate']:
  rows=[b for c in a['cohorts'] if c['kind']==kind for b in c['concurrent_comparison']['blocks'] if path=='all' or b['path']==path];x=m['concurrent'][path][kind];volume=sum(b['echoed_payload_bytes'] for b in rows)/1048576
  assert x['blocks']==len(rows) and x['scored_datagrams']==len(rows)*3200 and x['payload_mib']==volume
  assert math.isclose(x['aggregate_payload_mib_s'],volume/sum(b['elapsed_seconds_including_worker_start'] for b in rows),rel_tol=1e-12)
  values=[b['payload_mib_per_second'] for b in rows];assert x['block_payload_mib_s_range']==[min(values),max(values)]
  for role,y in x['resources'].items():
   cpu=[b['resource_deltas'][role]['cpu_ms'] for b in rows];pss=[b['resource_deltas'][role][key] for b in rows for key in ['pss_before_mib','pss_after_mib']]
   assert y['total_cpu_ms']==sum(cpu) and y['block_cpu_ms_range']==[min(cpu),max(cpu)] and y['pss_observation_mib_range']==[min(pss),max(pss)]
   assert math.isclose(y['cpu_ms_per_payload_mib'],sum(cpu)/volume,rel_tol=1e-12)
 x=m['concurrent'][path];b=x['baseline'];c=x['candidate']
 assert math.isclose(x['observed_target_cpu_reduction_percent'],100*(1-c['resources']['target_daemon']['cpu_ms_per_payload_mib']/b['resources']['target_daemon']['cpu_ms_per_payload_mib']),rel_tol=1e-12)
 assert math.isclose(x['observed_aggregate_throughput_increase_percent'],100*(c['aggregate_payload_mib_s']/b['aggregate_payload_mib_s']-1),rel_tol=1e-12)
for x in m['latency']:
 for kind in ['baseline','candidate']:
  rows=[s for c in a['cohorts'] if c['kind']==kind for s in c['summary'] if s['path']==x['path'] and s['payload_bytes']==x['payload_bytes']]
  assert len(rows)==2
  for key,limits in x[kind].items():assert limits==[min(s[key] for s in rows),max(s[key] for s in rows)]
print('Weighted throughput, CPU per MiB/reductions, all-role CPU/PSS ranges and cohort latency ranges independently verified.')
