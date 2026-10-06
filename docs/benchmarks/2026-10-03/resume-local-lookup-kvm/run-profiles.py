from pathlib import Path
import hashlib,json,subprocess
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
r=json.loads(Path('/var/tmp/hm-native-port-migration-kvm-v1-ipv4/report.json').read_text())
for name,h in r['inputs_sha256'].items():
 if name.endswith('check-udp-cluster-kvm.py'):continue
 assert hashlib.sha256(Path(name).read_bytes()).hexdigest()==h,name
for mode,family in (('shared','ipv4'),('shared','ipv6'),('local','ipv4'),('local','ipv6')):
 output=Path('/var/tmp/hm-resume-local-kvm-v1-'+mode+'-'+family);assert not output.exists()
 command=['python3',str(root/'tools/check-udp-cluster-kvm.py'),'--daemon','/var/tmp/hm-resume-local-node-v1','--control-plane','/var/tmp/hm-owner-adoption-control-v3','--cli','/var/tmp/hm-owner-adoption-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--owner-port-cli','--owner-adoption','--owner-adoption-cross-node','--native-port-migration','--peer-count','8' if family=='ipv6' else '2']
 if mode=='local':
  command=[arg for arg in command if arg not in ('--owner-adoption','--owner-adoption-cross-node','--native-port-migration')]
  command.append('--in-memory-pauses')
 if family=='ipv6':command.append('--local-ipv6')
 print('Starting',mode,family,flush=True)
 with output.with_suffix('.stdout').open('x') as log:subprocess.run(command,check=True,stdout=log,stderr=subprocess.STDOUT)
 print('Passed',mode,family,flush=True)
