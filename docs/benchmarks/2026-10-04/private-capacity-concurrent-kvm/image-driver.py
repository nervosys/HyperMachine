from pathlib import Path
import hashlib,gzip,tempfile,subprocess,shutil,os,json
base=Path('/var/tmp/hm-private-udp-pause-guest-v1.cpio.gz');echo=Path('/var/tmp/hm-private-capacity-buffered-echo-v1');out=Path('/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz');assert not out.exists()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();assert sha(base)=='c7dae4197f4939ec344de2245d94fed38f98c36e8564ecf2a61d51caab634883'
assert 'INTERP' not in subprocess.check_output(['readelf','-l',str(echo)],text=True)
with tempfile.TemporaryDirectory(prefix='hm-capacity-buffer-image-') as temp:
 root=Path(temp);subprocess.run(['cpio','-id','--no-absolute-filenames','--quiet'],cwd=root,input=gzip.decompress(base.read_bytes()),capture_output=True,check=True)
 target=root/'bin/hm-udp-echo';assert target.is_file() and not target.is_symlink();old=sha(target);shutil.copyfile(echo,target);target.chmod(0o755)
 names=sorted(['.']+[str(p.relative_to(root)) for p in root.rglob('*')])
 for name in names:os.utime(root/name,(0,0),follow_symlinks=False)
 packed=subprocess.run(['cpio','--null','-o','-H','newc','-R','0:0','--reproducible','--quiet'],cwd=root,input=b'\0'.join(n.encode() for n in names)+b'\0',capture_output=True,check=True).stdout
 with out.open('xb') as f:f.write(gzip.compress(packed,compresslevel=9,mtime=0))
report={'base_sha256':sha(base),'old_echo_sha256':old,'echo_sha256':sha(echo),'image_sha256':sha(out),'only_replaced_path':'bin/hm-udp-echo'};Path('/var/tmp/hm-private-capacity-buffered-image-v1.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
