from pathlib import Path
import subprocess,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');checker=root/'tools/check-udp-cluster-kvm.py'
binaries={'baseline':Path('/var/tmp/hm-private-udp-comparison-release-node-v1'),'candidate':Path('/var/tmp/hm-private-route-live-snapshot-release-node-v1')}
hashes={kind:hashlib.sha256(binary.read_bytes()).hexdigest() for kind,binary in binaries.items()}
assert hashes['baseline']=='0250dab6fd392c871aa65e041be5b604c690da43a4c296a81e267471b2d9ccc9'
checker_hash=hashlib.sha256(checker.read_bytes()).hexdigest();cohorts=[]
for index,kind in enumerate(['baseline','candidate','candidate','baseline'],1):
 output=Path(f'/var/tmp/hm-private-route-live-snapshot-release-abba-{index}');assert not output.exists()
 assert hashlib.sha256(checker.read_bytes()).hexdigest()==checker_hash
 assert hashlib.sha256(binaries[kind].read_bytes()).hexdigest()==hashes[kind]
 command=['python3',str(checker),'--daemon',str(binaries[kind]),'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-udp-pause-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-receiving-udp','--private-guest-tcp','--private-guest-udp','--private-cross-node','--private-guest-revocation','--private-udp-transport-samples','32','--host-build-profile','release']
 print('Starting',index,kind,flush=True)
 with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=240)
 assert result.returncode==0,output.with_suffix('.stdout').read_text()[-5000:]
 r=json.loads((output/'report.json').read_text());assert len(r['checks'])==51 and r['guests_remaining']==0
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
 for path,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(path).read_bytes()).hexdigest()==h,path
 b=r['private_udp_transport_benchmark'];assert b['all_scored_success'] and b['host_build_profile']=='release'
 assert len(b['rows'])==136 and len([row for row in b['rows'] if not row['warmup']])==128
 cohort={'index':index,'kind':kind,'daemon_sha256':hashes[kind],'checks':len(r['checks']),'scored_operations':128,'paired_differences':b['paired_differences'],'summary':b['summary']}
 cohorts.append(cohort);print(json.dumps(cohort),flush=True)
for kind,binary in binaries.items():assert hashlib.sha256(binary.read_bytes()).hexdigest()==hashes[kind]
report={'order':['baseline','candidate','candidate','baseline'],'cohorts':cohorts,'scored_operations':512,'all_scored_success':True,'checker_sha256':checker_hash,'daemon_sha256':hashes}
Path('/var/tmp/hm-private-route-live-snapshot-release-abba-analysis-v1.json').write_text(json.dumps(report,indent=2)+'\n')
print('ABBA complete: 512/512 scored operations, 204 functional checks, complete cleanup.',flush=True)
