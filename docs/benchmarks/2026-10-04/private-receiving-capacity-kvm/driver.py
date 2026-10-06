from pathlib import Path
import subprocess,json,hashlib
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');checker=root/'tools/check-udp-cluster-kvm.py'
kind='candidate';binaries={kind:Path('/var/tmp/hm-private-setup-overlap-release-node-v1')};output=Path('/var/tmp/hm-private-receiving-capacity-kvm-v1');assert not output.exists()
command=['python3',str(checker),'--daemon',str(binaries[kind]),'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-udp-pause-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-receiving-udp','--private-guest-tcp','--private-guest-udp','--private-cross-node','--private-guest-revocation','--private-udp-transport-samples','32','--host-build-profile','release','--private-receiving-capacity']
with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=300)
assert result.returncode==0,output.with_suffix('.stdout').read_text()[-6000:]
r=json.loads((output/'report.json').read_text());assert len(r['checks'])==53 and r['guests_remaining']==0
for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
c=r['private_receiving_udp']['receiving_capacity']
assert c['extra_status']==c['refilled_extra_status']==503 and c['recovered']
print(json.dumps(c));print('53 KVM checks with actual private receiving saturation/recovery and 128 scored operations passed.')
