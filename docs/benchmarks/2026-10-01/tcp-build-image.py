import gzip,hashlib,json,os,subprocess,tempfile
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp')
source=Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz')
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
original=digest(source);assert original=='1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'
command=['gcc','-static','-O2','-Wall','-Wextra','-Werror',str(root/'tools/guest-image/tcp-fixture.c'),'-o',str(out/'tcp-fixture')]
result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT);(out/'fixture-build.txt').write_bytes(result.stdout);assert result.returncode==0,result.stdout
archive=gzip.decompress(source.read_bytes())
names=subprocess.check_output(['cpio','-t','--quiet'],input=archive).decode().splitlines()
assert all(not Path(n).is_absolute() and '..' not in Path(n).parts for n in names)
with tempfile.TemporaryDirectory(prefix='hm-tcp-image-',dir='/var/tmp') as directory:
 subprocess.run(['cpio','-id','--quiet','--no-absolute-filenames'],input=archive,cwd=directory,check=True)
 folder=Path(directory);init_sha=digest(folder/'init')
 for name in ['hv2-guest-agentd','tcp-fixture']:
  (folder/'bin'/name).write_bytes((out/name).read_bytes());(folder/'bin'/name).chmod(0o755)
 # Keep the exact existing init; the fixture is launched explicitly by exec.
 assert digest(folder/'init')==init_sha
 packed=subprocess.check_output(['bash','-c','find . -print0 | LC_ALL=C sort -z | xargs -0 touch -h -d @0; find . -print0 | LC_ALL=C sort -z | cpio --null -o -H newc -R 0:0 --reproducible --quiet'],cwd=directory)
 (out/'guest-tcp.cpio.gz').write_bytes(gzip.compress(packed,compresslevel=9,mtime=0))
 report={'source_initrd_sha256':original,'source_initrd_unchanged':digest(source)==original,'init_sha256':init_sha,'agent_sha256':digest(out/'hv2-guest-agentd'),'fixture_sha256':digest(out/'tcp-fixture'),'fixture_source_sha256':digest(root/'tools/guest-image/tcp-fixture.c'),'output_sha256':digest(out/'guest-tcp.cpio.gz'),'command':command,'compiler':subprocess.check_output(['gcc','--version'],text=True).splitlines()[0],'builder_sha256':digest(Path(__file__))}
 (out/'image-build.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
