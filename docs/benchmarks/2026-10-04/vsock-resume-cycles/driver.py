from pathlib import Path
import subprocess,json
checker=Path('/var/tmp/hm-vsock-resume-cycles-checker-v1.py')
for kind,binary in [('baseline','/var/tmp/hm-private-setup-overlap-release-node-v1'),('candidate','/var/tmp/hm-vsock-connection-progress-release-node-v1')]:
 output=Path(f'/var/tmp/hm-vsock-resume-cycles-{kind}-v1');assert not output.exists()
 command=['python3',str(checker),'--daemon',binary,'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--resume-cycle-checks','8']
 print('Starting focused repeated resume',kind,flush=True)
 with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=300)
 print(kind,'exit',result.returncode,flush=True)
 if result.returncode==0:
  r=json.loads((output/'report.json').read_text());assert r['guests_remaining']==0
  for key in ['daemon_reaped','control_and_redis_reaped','cli_reaped']:assert r[key]
  rows=json.loads((output/'resume-cycles.json').read_text());assert len(rows)==7 and all(row['exact_udp'] for row in rows);print(kind,'8 main-target resumes passed; full cleanup.',flush=True)
 else:print(output.with_suffix('.stdout').read_text()[-1500:],flush=True)
