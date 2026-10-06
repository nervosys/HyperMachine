from pathlib import Path
import subprocess,os,shutil,hashlib,json
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');iso=Path('/var/tmp/hm-egress-log-mA2CCL');client=Path('/var/tmp/hm-private-udp-pause-client-v1');assert not client.exists()
context=json.loads((root/'docs/benchmarks/2026-10-04/private-udp-ipv4-fragmentation/source-context.json').read_text())
for name,h in context['permitted_root_isolated_sha256'].items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==h,name
 assert hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
for name,h in context['accepted_isolated_core_sha256'].items():
 assert hashlib.sha256((iso/name).read_bytes()).hexdigest()==h,name
print('Verified accepted source catalog before build',flush=True)
subprocess.run(['gcc','-static','-O2','-Wall','-Wextra','-Werror',str(root/'tools/guest-image/private-udp-client.c'),'-o',str(client)],check=True)
subprocess.run(['python3',str(root/'tools/build-private-udp-client-image.py'),'--base','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--client',str(client),'--output','/var/tmp/hm-private-udp-pause-guest-v1.cpio.gz','--report','/var/tmp/hm-private-udp-pause-guest-image-v1.json'],check=True)
