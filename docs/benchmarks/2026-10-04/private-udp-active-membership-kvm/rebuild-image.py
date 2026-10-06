from pathlib import Path
import subprocess,json,hashlib
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
subprocess.run(['python3',str(root/'tools/build-private-udp-client-image.py'),'--base','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--client','/var/tmp/hm-private-udp-live-client-v2','--output','/var/tmp/hm-private-udp-live-guest-v4.cpio.gz','--report','/var/tmp/hm-private-udp-live-guest-image-v4.json'],check=True)
a=json.loads(Path('/var/tmp/hm-private-udp-live-guest-image-v3.json').read_text());b=json.loads(Path('/var/tmp/hm-private-udp-live-guest-image-v4.json').read_text());assert a==b
assert hashlib.sha256(Path('/var/tmp/hm-private-udp-live-guest-v4.cpio.gz').read_bytes()).hexdigest()==a['image_sha256']
