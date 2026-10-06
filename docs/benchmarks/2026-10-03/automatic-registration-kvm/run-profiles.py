from pathlib import Path
import hashlib,json,subprocess
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
r=json.loads(Path('/var/tmp/hm-native-port-migration-kvm-v1-ipv4/report.json').read_text())
for name,h in r['inputs_sha256'].items():
 if name.endswith('check-udp-cluster-kvm.py'):continue
 assert hashlib.sha256(Path(name).read_bytes()).hexdigest()==h,name
for stage,family in (('SET','ipv4'),('SET','ipv6'),('XADD','ipv4'),('XADD','ipv6')):
 output=Path('/var/tmp/hm-automatic-registration-kvm-v1-'+stage.lower()+'-'+family);assert not output.exists()
 command=['python3',str(root/'tools/check-udp-cluster-kvm.py'),'--daemon','/var/tmp/hm-registration-worker-node-v1','--control-plane','/var/tmp/hm-discovery-schema-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--owner-port-cli','--resume-publication-fault','--resume-publication-fault-command',stage,'--recovery-cli','--creation-publication-fault','--pending-discovery','--publisher-race','--automatic-registration-recovery','--peer-count','8' if family=='ipv6' else '2']
 if family=='ipv6':command.append('--local-ipv6')
 print('Starting',stage,family,flush=True)
 with output.with_suffix('.stdout').open('x') as log:subprocess.run(command,check=True,stdout=log,stderr=subprocess.STDOUT)
 print('Passed',stage,family,flush=True)
