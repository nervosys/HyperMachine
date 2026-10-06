from pathlib import Path
import json
p=Path(__file__).parent
for label in ['normal','failure']:
 rows=[json.loads(x) for x in (p/(label+'-private-transport-rows.jsonl')).read_text().splitlines()]
 b=json.loads((p/(label+'-private-transport-benchmark.json')).read_text())
 assert rows==b['rows'] and len(rows)==136
 scored=[x for x in rows if not x['warmup']]
 assert len(scored)==128
 for s in b['summary']:
  group=[x for x in scored if x['path']==s['path'] and x['payload_bytes']==s['payload_bytes']]
  assert len(group)==s['planned']==32 and sum(x['success'] for x in group)==s['successful']
  assert sum(not x['success'] for x in group)==s['failed']
 bad=[x for x in rows if not x['success']]
 if label=='normal':assert not bad
 else:assert len(bad)==1 and bad[0]['failure_type']=='TimeoutError' and bad[0]['pair']==0 and bad[0]['path']=='private'
r=json.loads((p/'report.json').read_text());assert r['private_transport_benchmark']==json.loads((p/'normal-private-transport-benchmark.json').read_text())
assert len(r['checks'])==30 and r['guests_remaining']==0 and r['daemon_reaped'] and r['secondary_node_reaped'] and r['control_and_redis_reaped']
f=json.loads((p/'failure-verification.json').read_text());assert f['false_success_report_absent'] and f['all_tracked_child_processes_reaped'] and f['failed_scored_operations']==1
needle="                                                native_tcp_echo(client,benchmark_payload)"
s=(p/'checker.py').read_text();assert s.count(needle)==1
assert (p/'injected-checker.py').read_text()==s.replace(needle,"                                                if pair==0 and kind=='private' and payload_bytes==64:raise TimeoutError('owned fixture injected sample failure')\n"+needle)
print('Normal/failure journals, summary totals, exact isolated injection and cleanup evidence verified.')
