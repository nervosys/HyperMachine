from pathlib import Path
import subprocess,json,hashlib
checker=Path('/var/tmp/hm-vsock-resume-cycles-checker-v1.py');binary=Path('/var/tmp/hm-pause-owner-ack-release-node-v1');output=Path('/var/tmp/hm-pause-owner-ack-resume-cycles-v1');assert not output.exists()
command=['python3',str(checker),'--daemon',str(binary),'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--resume-cycle-checks','20']
print('Starting acknowledged-pause release, 20 main-target resumes with unchanged deadlines and no retries',flush=True)
with output.with_suffix('.stdout').open('x') as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=300)
assert result.returncode==0,output.with_suffix('.stdout').read_text()[-2500:]
r=json.loads((output/'report.json').read_text());assert len(r['checks'])==21 and r['guests_remaining']==0
for key in ['daemon_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
rows=json.loads((output/'resume-cycles.json').read_text());assert len(rows)==19 and all(row['pause_completed'] and row['resume_completed'] and row['exact_udp'] for row in rows)
for key,h in r['inputs_sha256'].items():assert hashlib.sha256(Path(key).read_bytes()).hexdigest()==h,key
print('20 total main-target resumes, exact identified UDP sessions, prior closure, 21 KVM checks and full cleanup verified.',flush=True)
