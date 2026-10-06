from pathlib import Path
import json,hashlib
p=Path(__file__).parent;a=json.loads((p/'analysis.json').read_text());assert a['order']==['baseline','candidate','candidate','baseline'] and a['scored_operations']==512 and a['concurrent_scored_datagrams']==51200
assert a['daemon_sha256']=={'baseline':'aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef','candidate':'aab60303cf468e9266279e6d701a18df2638b262d50bf8f18ba44877a8072f0a'}
source={kind:json.loads((p/f'{kind}-source-context.json').read_text()) for kind in ['baseline','candidate']}
changed=[key for key,h in source['baseline']['permitted_root_isolated_sha256'].items() if source['candidate']['permitted_root_isolated_sha256'][key]!=h]
assert sorted(changed)==['crates/hv2-agent/src/guest_agent.rs','crates/hv2-core/src/devices/virtio_vsock.rs']
assert len(source['candidate']['permitted_root_isolated_sha256'])==145
assert set(source['candidate']['permitted_root_isolated_sha256'])-set(source['baseline']['permitted_root_isolated_sha256'])=={'crates/hv2-core/src/vm.rs','crates/hv2-core/src/devices/virtio_mmio.rs'}
assert source['candidate']['accepted_isolated_core_sha256']==source['baseline']['accepted_isolated_core_sha256']
for snap,rel in [('candidate-vm.rs','crates/hv2-core/src/vm.rs'),('candidate-virtio-mmio.rs','crates/hv2-core/src/devices/virtio_mmio.rs')]:assert hashlib.sha256((p/snap).read_bytes()).hexdigest()==source['candidate']['permitted_root_isolated_sha256'][rel]
for kind in source:
 for name,key in [('virtio-vsock.rs','crates/hv2-core/src/devices/virtio_vsock.rs'),('guest-agent.rs','crates/hv2-agent/src/guest_agent.rs')]:assert hashlib.sha256((p/f'{kind}-{name}').read_bytes()).hexdigest()==source[kind]['permitted_root_isolated_sha256'][key]
common=None
for i,kind in enumerate(a['order'],1):
 q=p/f'cohort-{i}';r=json.loads((q/'report.json').read_text());c=json.loads((q/'source-context.json').read_text())
 exec((p/'verify-cohort.py').read_text(),{'__file__':str(q/'verify-cohort.py')})
 assert c['checker_sha256']==a['checker_sha256'] and c['daemon_sha256']==a['daemon_sha256'][kind]
 daemon=next(path for path in r['inputs_sha256'] if path.endswith('-release-node-v1'))
 other={key:h for key,h in r['inputs_sha256'].items() if key!=daemon}
 if common is None:common=other
 else:assert common==other
 row=a['cohorts'][i-1];assert row['index']==i and row['kind']==kind and row['checks']==55 and row['scored_operations']==128
 assert row['concurrent_comparison']==json.loads((q/'comparison.json').read_text())
 assert row['summary']==r['private_udp_transport_benchmark']['summary'] and row['paired_differences']==r['private_udp_transport_benchmark']['paired_differences']
assert '26 passed; 0 failed' in (p/'vsock-tests.txt').read_text() and '531 passed; 0 failed' in (p/'agent-tests.txt').read_text() and '64 passed; 0 failed; 2 ignored' in (p/'daemon-tests.txt').read_text()
assert 'release' in (p/'build.txt').read_text() and 'optimized' in (p/'build.txt').read_text()
assert '16 passed; 0 failed' in (p/'mmio-tests.txt').read_text() and '47 passed; 0 failed; 2 ignored' in (p/'vm-tests.txt').read_text()
print('Two changed shared catalog entries plus two additional candidate source snapshots; no exact whole-source delta claim. Four release cohorts, shared fixture identities, 220 KVM checks, 512 regression operations, 51200 matched datagrams and all resource/drop/cleanup evidence independently verified.')
