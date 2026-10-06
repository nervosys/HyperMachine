from pathlib import Path
import hashlib,json
p=Path(__file__).parent;first=p.parent/'mmio-irq-order';inputs=json.loads((first/'report.json').read_text())['inputs_sha256']
for n in [2,3]:
 d=p/('cohort-'+str(n));r=json.loads((d/'report.json').read_text());rows=json.loads((d/'resume-cycles.json').read_text())
 assert r['inputs_sha256']==inputs and len(r['checks'])==21 and r['guests_remaining']==0
 for key in ['daemon_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 assert len(rows)==19 and [x['cycle'] for x in rows]==list(range(1,20))
 assert all(x['pause_completed'] and x['resume_completed'] and x['exact_udp'] and x['elapsed_seconds']>0 for x in rows)
assert hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()==inputs['/var/tmp/hm-vsock-resume-cycles-checker-v1.py']
print('Two independent original-deadline 20-resume repeats verified; 42 checks and cleanup; same pinned inputs as first cohort. No permanent-elimination or performance claim.')
