from pathlib import Path
import json,hashlib
p=Path(__file__).parent;r=json.loads((p/'report.json').read_text());rows=json.loads((p/'resume-cycles.json').read_text());c=json.loads((p/'source-context.json').read_text())
assert len(rows)==19 and [x['cycle'] for x in rows]==list(range(1,20))
assert all(x['pause_completed'] and x['resume_completed'] and x['exact_udp'] and x['elapsed_seconds']>0 for x in rows)
assert len(r['checks'])==21 and r['guests_remaining']==0
for key in ['daemon_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
assert c['runtime_inputs_sha256']==r['inputs_sha256']
assert hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()==r['inputs_sha256']['/var/tmp/hm-vsock-resume-cycles-checker-v1.py']
for file,expected in [('mmio-tests.txt','16 passed; 0 failed'),('vm-tests.txt','47 passed; 0 failed; 2 ignored'),('vsock-tests.txt','26 passed; 0 failed'),('agent-tests.txt','531 passed; 0 failed'),('daemon-tests.txt','64 passed; 0 failed; 2 ignored')]:assert expected in (p/file).read_text()
red=(p/'old-order-tests.txt').read_text();assert 'status lock released before line assertion' in red and 'status lock released before line deassertion' in red and '2 failed' in red
assert len(c['permitted_root_isolated_sha256'])==145
for snap,rel in [('vm.rs','crates/hv2-core/src/vm.rs'),('virtio-vsock.rs','crates/hv2-core/src/devices/virtio_vsock.rs'),('guest-agent.rs','crates/hv2-agent/src/guest_agent.rs'),('virtio-mmio.rs','crates/hv2-core/src/devices/virtio_mmio.rs')]:assert hashlib.sha256((p/snap).read_bytes()).hexdigest()==c['permitted_root_isolated_sha256'][rel]
print('Single original-deadline 20-resume cohort independently verified: 21 checks, exact UDP/prior closure, cleanup, red/green IRQ tests and pinned source snapshots. This does not prove the intermittent stall is eliminated or a CPU win.')
