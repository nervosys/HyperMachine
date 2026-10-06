import hashlib,json,math,re,statistics
from pathlib import Path
root=Path(__file__).resolve().parents[1] if Path(__file__).parent.name=='target' else Path(__file__).resolve().parents[3]
out=root/'docs/benchmarks/2026-10-01'
raw=out/'matched-readiness-phases.json';report=json.loads(raw.read_bytes())
assert report['success'] and report['artifacts_unchanged'] and report['remaining_sandbox_count']==0 and not report['node_diagnostic_log_truncated']
log=re.sub(r'\x1b\[[0-9;]*m','',report['node_diagnostic_log'])
internal=[{key:float(re.search(key+r'=([0-9.eE+-]+)',line).group(1)) for key in ['blocking_queue_ms','connect_ms','ping_ms']} for line in log.splitlines() if 'cold guest readiness stages' in line]
assert len(internal)==16
summary={'raw_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),'internal_hypermachine_mean_ms':{key:statistics.mean(row[key] for row in internal) for key in internal[0]},'engines':{}}
for batch in report['batches']:
 assert len(batch['samples'])==16 and all(row['success'] and row['cleanup_success'] for row in batch['samples'])
 keys=['create_ms','exec_ms'] if batch['engine']=='hypermachine' else ['api_setup_ms','agent_connect_ms','exec_ms']
 for row in batch['samples']:
  assert math.isclose(sum(row[key] for key in keys),row['ready_ms'],rel_tol=1e-12)
 summary['engines'][batch['engine']]={key:statistics.mean(row[key] for row in batch['samples']) for key in keys}
(out/'matched-readiness-phases-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary))