from pathlib import Path
import subprocess,json,hashlib
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
output=Path('/var/tmp/hm-private-receiving-kvm-v2');assert not output.exists()
command=['python3',str(root/'tools/check-udp-cluster-kvm.py'),'--daemon','/var/tmp/hm-private-receiving-node-v2','--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp']
with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=240)
print('Fixture exit:',result.returncode)
assert result.returncode==0,output.with_suffix('.stdout').read_text()[-5000:]
r=json.loads((output/'report.json').read_text());assert r['guests_remaining']==0
for p,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==h,p
assert r['private_receiving_tcp']['payload_target_real_kvm'] and not r['private_receiving_tcp']['source_gateway_verified']
print(json.dumps({'checks':len(r['checks']),'private_receiving_tcp':r['private_receiving_tcp'],'daemon_reaped':r['daemon_reaped'],'control_and_redis_reaped':r['control_and_redis_reaped'],'guests_remaining':r['guests_remaining']},indent=2))
