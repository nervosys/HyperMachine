from pathlib import Path
import subprocess,os,shutil,hashlib
iso=Path('/var/tmp/hm-egress-log-mA2CCL');env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target')
for label,command in [('tests',['cargo','test','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd']),('build',['cargo','build','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'])]:
 log=Path('/var/tmp/hm-private-udp-receiving-'+label+'-v1.txt')
 with log.open('x') as f:r=subprocess.run(command,cwd=iso,env=env,stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0,log.read_text()[-5000:]
 if label=='tests':assert '64 passed; 0 failed; 2 ignored' in log.read_text()
 print(label,'passed',flush=True)
binary=Path('/var/tmp/hm-private-udp-receiving-node-v1');assert not binary.exists();shutil.copy2('/var/tmp/hm-object-backup/target/debug/hv2-sandboxd',binary);print('Binary SHA256',hashlib.sha256(binary.read_bytes()).hexdigest())
