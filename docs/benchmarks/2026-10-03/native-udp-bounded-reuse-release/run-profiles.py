from pathlib import Path
import hashlib,json,subprocess
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
checker=root/'tools/check-udp-cluster-kvm.py'
reference=json.loads(Path('/var/tmp/hm-udp-reuse-release-v1-ipv4-0-baseline/report.json').read_text())
for name,h in reference['inputs_sha256'].items():
 assert hashlib.sha256(Path(name).read_bytes()).hexdigest()==h,name
candidate=Path('/var/tmp/hm-udp-bounded-reuse-release-gateway-v1')
assert candidate.exists()
for family in ('ipv4','ipv6'):
 for index,variant in enumerate(('baseline','candidate','candidate','baseline')):
  output=Path(f'/var/tmp/hm-udp-bounded-reuse-release-v1-{family}-{index}-{variant}')
  assert not output.exists(),output
  gateway=str(candidate) if variant=='candidate' else '/var/tmp/hm-udp-reuse-release-baseline-gateway-v1'
  command=['python3',str(checker),'--daemon','/var/tmp/hm-udp-reuse-release-baseline-node-v1','--control-plane','/var/tmp/hm-udp-reuse-release-baseline-control-v1','--cli','/var/tmp/hm-udp-reuse-release-baseline-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway',gateway,'--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--owner-port-cli','--native-comparison-samples','2000','--native-comparison-bytes','4096','--native-resource-samples','--host-build-profile','release','--peer-count','8' if family=='ipv6' else '2']
  if family=='ipv6':command.append('--local-ipv6')
  print('Starting',family,index,variant,flush=True)
  subprocess.run(command,check=True)
  print('Finished',family,index,variant,flush=True)
