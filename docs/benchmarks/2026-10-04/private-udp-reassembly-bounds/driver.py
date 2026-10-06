from pathlib import Path
import subprocess,os
iso=Path('/var/tmp/hm-egress-log-mA2CCL')
log=Path('/var/tmp/hm-private-udp-fragment-bounds-tests-v1.txt')
with log.open('x') as f:r=subprocess.run(['cargo','test','--locked','-p','hv2-net','--lib'],cwd=iso,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0 and '204 passed; 0 failed' in log.read_text(),log.read_text()[-6000:]
print('204 networking tests passed.')
