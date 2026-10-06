from pathlib import Path
import subprocess,os
iso=Path('/var/tmp/hm-egress-log-mA2CCL');env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target')
for package,expected in [('hv2-net','200 passed; 0 failed'),('hv2-sandboxd','64 passed; 0 failed; 2 ignored')]:
 command=['cargo','test','--locked','-p',package]+(['--lib'] if package=='hv2-net' else ['--bin','hv2-sandboxd'])
 log=Path('/var/tmp/hm-private-udp-ethernet-'+package+'-v4.txt')
 with log.open('x') as f:r=subprocess.run(command,cwd=iso,env=env,stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0 and expected in log.read_text(),log.read_text()[-5000:]
 print(package,'passed',flush=True)
