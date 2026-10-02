import hashlib,json,re,statistics
from pathlib import Path
root=Path(__file__).resolve().parents[1] if Path(__file__).parent.name=='target' else Path(__file__).resolve().parents[3]
out=root/'docs/benchmarks/2026-10-01'
r=json.loads((out/'cold-readiness-phases.json').read_bytes())
assert r['success'] and r['artifacts_unchanged'] and r['remaining_sandbox_count']==0
assert not r['node_diagnostic_log_truncated'] and r['daemon_log_filter']!='warn'
log=re.sub(r'\x1b\[[0-9;]*m','',r['node_diagnostic_log'])
rows=[]
for line in log.splitlines():
 if 'cold guest readiness stages' not in line:continue
 row={key:float(re.search(key+r'=([0-9.eE+-]+)',line).group(1)) for key in ['blocking_queue_ms','connect_ms','ping_ms']}
 row['sandbox_id']=re.search(r'vm=(sbx-[0-9a-f]+)',line).group(1)
 assert 'succeeded=true' in line
 rows.append(row)
assert len(rows)==16 and len({r['sandbox_id'] for r in rows})==16
summary={'raw_sha256':hashlib.sha256((out/'cold-readiness-phases.json').read_bytes()).hexdigest(),'diagnostic_rows':rows,'phase_mean_ms':{key:statistics.mean(row[key] for row in rows) for key in ['blocking_queue_ms','connect_ms','ping_ms']}}
cleanup=[]
for rate in [5,25]:
 report=json.loads((out/f'fixed-arrivals-{rate}ps.json').read_bytes())
 for engine in ['hypermachine','firecracker']:
  records=[r for b in report['batches'] if b['engine']==engine for r in b['samples']]
  cleanup.append({'rate':rate,'engine':engine,'mean_cleanup_ms':statistics.mean(r['cleanup_completed_offset_ms']-r['validated_offset_ms'] for r in records)})
summary['scored_cleanup_breakdown']=cleanup
(out/'cold-readiness-phases-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'phase_mean_ms':summary['phase_mean_ms'],'scored_cleanup_breakdown':cleanup}))