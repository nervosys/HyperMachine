from pathlib import Path
import subprocess,os,shutil,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');iso=Path('/var/tmp/hm-egress-log-mA2CCL');client=Path('/var/tmp/hm-private-udp-client-v1');assert not client.exists()
subprocess.run(['gcc','-static','-O2','-Wall','-Wextra','-Werror',str(root/'tools/guest-image/private-udp-client.c'),'-o',str(client)],check=True)
subprocess.run(['python3',str(root/'tools/build-private-udp-client-image.py'),'--base','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--client',str(client),'--output','/var/tmp/hm-private-udp-guest-v1.cpio.gz','--report','/var/tmp/hm-private-udp-guest-image-v1.json'],check=True)
log=Path('/var/tmp/hm-private-udp-guest-daemon-build-v1.txt')
with log.open('x') as f:r=subprocess.run(['cargo','build','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'],cwd=iso,env=dict(os.environ,CARGO_TARGET_DIR='/var/tmp/hm-object-backup/target'),stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0,log.read_text()[-5000:]
binary=Path('/var/tmp/hm-private-udp-guest-node-v1');assert not binary.exists();shutil.copy2('/var/tmp/hm-object-backup/target/debug/hv2-sandboxd',binary)
print('Frozen daemon',hashlib.sha256(binary.read_bytes()).hexdigest());print(Path('/var/tmp/hm-private-udp-guest-image-v1.json').read_text())
