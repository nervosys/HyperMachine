from pathlib import Path
import subprocess,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');checker=root/'tools/check-udp-cluster-kvm.py'
binaries={'baseline':Path('/var/tmp/hm-private-setup-overlap-release-node-v1'),'candidate':Path('/var/tmp/hm-vsock-connection-progress-release-node-v1')}
hashes={kind:hashlib.sha256(binary.read_bytes()).hexdigest() for kind,binary in binaries.items()}
assert hashes['baseline']=='aef812594544d708d6c0101c27b3cdc0c93f6c3ded3e408f1eff9e253be955ef'
checker_hash=hashlib.sha256(checker.read_bytes()).hexdigest();cohorts=[]
for index,kind in enumerate(['baseline','candidate','candidate','baseline'],1):
 output=Path(f'/var/tmp/hm-vsock-connection-progress-release-abba-{index}');assert not output.exists()
 assert hashlib.sha256(checker.read_bytes()).hexdigest()==checker_hash
 assert hashlib.sha256(binaries[kind].read_bytes()).hexdigest()==hashes[kind]
 command=['python3',str(checker),'--daemon',str(binaries[kind]),'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-receiving-udp','--private-guest-tcp','--private-guest-udp','--private-cross-node','--private-guest-revocation','--private-udp-transport-samples','32','--host-build-profile','release','--private-receiving-capacity','--private-capacity-comparison']
 print('Starting',index,kind,flush=True)
 with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=240)
 assert result.returncode==0,output.with_suffix('.stdout').read_text()[-5000:]
 r=json.loads((output/'report.json').read_text());assert len(r['checks'])==55 and r['guests_remaining']==0
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 for path,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(path).read_bytes()).hexdigest()==h,path
 b=r['private_udp_transport_benchmark'];assert b['all_scored_success'] and b['host_build_profile']=='release'
 assert len(b['rows'])==136 and len([row for row in b['rows'] if not row['warmup']])==128
 cohort={'index':index,'kind':kind,'daemon_sha256':hashes[kind],'checks':len(r['checks']),'scored_operations':128,'paired_differences':b['paired_differences'],'summary':b['summary']}
 comparison=r['private_receiving_udp']['receiving_capacity']['concurrent_comparison']
 assert len(comparison['blocks'])==4 and all(block['all_success'] for block in comparison['blocks'])
 cohort['concurrent_comparison']=comparison
 cohorts.append(cohort);print(json.dumps({'index':index,'kind':kind,'checks':len(r['checks']),'blocks':[{'path':block['path'],'payload_mib_s':block['payload_mib_per_second'],'target_cpu_ms':block['resource_deltas']['target_daemon']['cpu_ms']} for block in comparison['blocks']]}),flush=True)
for kind,binary in binaries.items():assert hashlib.sha256(binary.read_bytes()).hexdigest()==hashes[kind]
report={'order':['baseline','candidate','candidate','baseline'],'cohorts':cohorts,'scored_operations':512,'concurrent_scored_datagrams':51200,'all_scored_success':True,'checker_sha256':checker_hash,'daemon_sha256':hashes}
Path('/var/tmp/hm-vsock-connection-progress-release-abba-analysis-v1.json').write_text(json.dumps(report,indent=2)+'\n')
print('ABBA complete: 512/512 scored operations, 220 functional checks, complete cleanup.',flush=True)
