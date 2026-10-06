from pathlib import Path
import subprocess,os
log=Path('/var/tmp/hm-private-route-live-snapshot-daemon-v1.txt')
with log.open('x') as f:r=subprocess.run(['cargo','test','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'],cwd='/var/tmp/hm-egress-log-mA2CCL',env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0 and '64 passed; 0 failed; 2 ignored' in log.read_text(),log.read_text()[-5000:]
print('64 ordinary daemon tests passed; 2 KVM-only tests ignored.')
