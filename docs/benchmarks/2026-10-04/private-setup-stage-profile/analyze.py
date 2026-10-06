from pathlib import Path
import re,json,statistics
run=Path('/var/tmp/hm-private-setup-profile-kvm-v1');r=json.loads((run/'report.json').read_text());b=r['private_udp_transport_benchmark']
metrics=['preauthorization_us','authorization_before_us','guest_open_us','loopback_pair_us','authorization_after_us','setup_us']
rows=[]
for line in (run/'daemon.log').read_text().splitlines():
 line=re.sub(r'\x1b\[[0-9;]*m','',line)
 if 'owned_private_setup_profile' not in line:continue
 fields=dict(re.findall(r'(\w+)=([^\s]+)',line));label=fields['profile_label'];match=re.fullmatch(r'udp-(\d+)-(-?\d+)-(private|standard)',label);assert match,label
 size,pair=int(match[1]),int(match[2]);kind=match[3]
 assert fields['udp']=='true' and fields['private_route']==str(kind=='private').lower()
 row={'label':label,'payload_bytes':size,'pair':pair,'path':kind,'warmup':pair<0,**{key:int(fields[key]) for key in metrics}}
 row['authorization_total_us']=row['authorization_before_us']+row['authorization_after_us']
 row['other_us']=row['setup_us']-sum(row[key] for key in metrics if key!='setup_us');assert row['other_us']>=0
 rows.append(row)
assert len(rows)==136 and len({row['label'] for row in rows})==136
key=lambda row:(row['payload_bytes'],row['pair'],row['path'])
assert set(map(key,rows))==set(map(key,b['rows']))
summary=[];paired=[];all_metrics=metrics+['authorization_total_us','other_us']
for size in [64,65507]:
 for kind in ['private','standard']:
  selected=[row for row in rows if row['payload_bytes']==size and row['path']==kind and not row['warmup']];assert len(selected)==32
  summary.append({'payload_bytes':size,'path':kind,'samples':32,'p50_us':{metric:statistics.median(row[metric] for row in selected) for metric in all_metrics}})
 deltas=[]
 for pair in range(32):
  selected={row['path']:row for row in rows if row['payload_bytes']==size and row['pair']==pair}
  deltas.append({metric:selected['private'][metric]-selected['standard'][metric] for metric in all_metrics})
 paired.append({'payload_bytes':size,'pairs':32,'private_minus_standard_p50_us':{metric:statistics.median(row[metric] for row in deltas) for metric in all_metrics}})
report={'scope':'diagnostic-only release build, tagged owned host-to-target UDP setup; instrumentation/logging can perturb timings; no production benchmark claim','matched_profile_rows':136,'scored_profile_rows':128,'metrics':all_metrics,'summary':summary,'paired_differences':paired,'rows':rows}
Path('/var/tmp/hm-private-setup-profile-analysis-v1.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'summary':summary,'paired_differences':paired},indent=2))
