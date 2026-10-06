from pathlib import Path
import subprocess,json,hashlib
checker=Path('/var/tmp/hm-pause-owner-ack-observe-checker-v1.py');out=Path('/var/tmp/hm-pause-owner-ack-observe-v1');assert not out.exists()
cmd=['python3',str(checker),'--daemon','/var/tmp/hm-pause-owner-ack-release-node-v1','--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(out),'--tls','--mtls','--owner-context','--owner-port-api','--resume-cycle-checks','20']
print('DIAGNOSTIC ONLY: API observation 90 seconds; no retry, unchanged daemon readiness budget; not original gate.',flush=True)
with out.with_suffix('.stdout').open('x') as f:r=subprocess.run(cmd,stdout=f,stderr=subprocess.STDOUT,timeout=360)
print('Diagnostic terminal status',r.returncode,flush=True)
print(out.with_suffix('.stdout').read_text()[-2500:],flush=True)
