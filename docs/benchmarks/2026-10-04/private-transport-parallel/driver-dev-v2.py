from pathlib import Path
import subprocess,json,hashlib
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
output=Path('/var/tmp/hm-private-transport-parallel-dev-kvm-v2');assert not output.exists()
command=['python3',str(root/'tools/check-udp-cluster-kvm.py'),'--daemon','/var/tmp/hm-private-transport-parallel-dev-node-v1','--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-guest-tcp','--private-cross-node','--private-guest-revocation','--private-transport-samples','32']
with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=240)
print('Fixture exit:',result.returncode)
assert result.returncode==0,output.with_suffix('.stdout').read_text()[-5000:]
r=json.loads((output/'report.json').read_text());assert r['guests_remaining']==0
for p,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==h,p
assert r['private_receiving_tcp']['payload_target_real_kvm']
assert r['private_guest_gateway']['source_gateway_verified'] and r['private_guest_gateway']['guest_dns_verified']
assert not r['private_guest_gateway']['same_node']
assert r['private_guest_gateway']['long_lived_guest_revocation_verified'] and r['private_guest_gateway']['revoked_guest_address_reconnect_refused']
assert not r['private_receiving_tcp']['same_node']
assert r['private_guest_gateway']['cross_network_dns_refused'] and r['private_guest_gateway']['source_guest_resume_verified']
assert r['private_guest_gateway']['source_node']!=r['private_guest_gateway']['destination_node']
print(json.dumps({'checks':len(r['checks']),'private_receiving_tcp':r['private_receiving_tcp'],'private_guest_gateway':r['private_guest_gateway'],'private_transport_summary':r['private_transport_benchmark']['summary'],'daemon_reaped':r['daemon_reaped'],'control_and_redis_reaped':r['control_and_redis_reaped'],'guests_remaining':r['guests_remaining']},indent=2))
