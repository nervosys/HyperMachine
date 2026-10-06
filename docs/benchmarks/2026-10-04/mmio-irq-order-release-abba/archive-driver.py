from pathlib import Path
import shutil,json,hashlib,subprocess
r=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');p=r/'docs/benchmarks/2026-10-04/mmio-irq-order-release-abba';assert p.exists() and not (p/"analysis.json").exists()
a=json.loads(Path('/var/tmp/hm-mmio-irq-order-release-abba-analysis-v1.json').read_text());assert a['scored_operations']==512 and a['concurrent_scored_datagrams']==51200 and len(a['cohorts'])==4
pre=r/'docs/benchmarks/2026-10-04/mmio-irq-order';base=r/'docs/benchmarks/2026-10-04/private-capacity-thread-kvm'
for f in ['build-driver.py','build.txt','vsock-tests.txt','mmio-tests.txt','vm-tests.txt','agent-tests.txt','daemon-tests.txt']:shutil.copyfile(pre/f,p/f)
(p/'analysis.json').write_text(json.dumps(a,indent=2)+'\n')
for kind,folder in [('baseline',base),('candidate',pre)]:
 shutil.copyfile(folder/'source-context.json',p/f'{kind}-source-context.json')
 for name in ['virtio-vsock.rs','guest-agent.rs']:shutil.copyfile(folder/name,p/f'{kind}-{name}')
for name in ['vm.rs','virtio-mmio.rs']:shutil.copyfile(pre/name,p/('candidate-'+name))
shared=(r/'docs/benchmarks/2026-10-04/private-capacity-resource-kvm/verify-results.py').read_text()
needle="assert r['inputs_sha256']['/var/tmp/hm-private-setup-overlap-release-node-v1']=='aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef'"
assert needle in shared
shared=shared.replace(needle,"daemon=next(path for path in r['inputs_sha256'] if path.endswith('-release-node-v1'));assert r['inputs_sha256'][daemon]==c['daemon_sha256']")
(p/'verify-cohort.py').write_text(shared)
for i,kind in enumerate(a['order'],1):
 src=Path(f'/var/tmp/hm-mmio-irq-order-release-abba-{i}');dest=p/f'cohort-{i}';dest.mkdir();report=json.loads((src/'report.json').read_text());assert len(report['checks'])==55 and report['guests_remaining']==0
 for f,name in [('report.json','report.json'),('private-capacity-comparison.json','comparison.json'),('private-capacity-traffic.json','traffic.json'),('private-udp-transport-rows.jsonl','rows.jsonl'),('private-udp-transport-benchmark.json','benchmark.json'),('daemon.log','daemon.txt')]:shutil.copyfile(src/f,dest/name)
 shutil.copyfile(src.with_suffix('.stdout'),dest/'stdout.txt');shutil.copyfile(r/'tools/check-udp-cluster-kvm.py',dest/'checker.py');shutil.copyfile(r/'crates/hv2-sandboxd/src/forwards.rs',dest/'forwards.rs')
 c=json.loads((p/f'{kind}-source-context.json').read_text());c.update(runtime_inputs_sha256=report['inputs_sha256'],checker_sha256=a['checker_sha256'],daemon_sha256=a['daemon_sha256'][kind]);(dest/'source-context.json').write_text(json.dumps(c,indent=2)+'\n')
 exec(shared,{'__file__':str(dest/'verify-cohort.py')})
print('Archived four matched release cohorts; outer source/analysis verifier, README/manifest/documentation still needed.')
