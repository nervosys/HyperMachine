#!/usr/bin/env python3
"""Add an owned static private UDP client to the exact accepted fixture image."""
import argparse,gzip,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
BASE_SHA256='a00cd88d52f5f0788b2113e26bf3bc45c66569b6dee21400aaaf5c9b12d29b7d'
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['base','client','output','report']:parser.add_argument('--'+name,type=Path,required=True)
    a=parser.parse_args()
    if digest(a.base)!=BASE_SHA256 or a.output.exists() or a.report.exists():raise ValueError('accepted base and fresh outputs required')
    if len({p.resolve() for p in [a.base,a.client,a.output,a.report]})!=4:raise ValueError('overlapping paths')
    client_hash=digest(a.client)
    if 'INTERP' in subprocess.check_output(['readelf','-l',str(a.client)],text=True):raise ValueError('static client required')
    with tempfile.TemporaryDirectory(prefix='hm-private-udp-image-') as temporary:
        root=Path(temporary)
        subprocess.run(['cpio','-id','--no-absolute-filenames','--quiet'],input=gzip.decompress(a.base.read_bytes()),cwd=root,capture_output=True,check=True)
        target=root/'bin/hm-private-udp-client'
        if target.exists() or target.is_symlink():raise ValueError('client target exists')
        shutil.copyfile(a.client,target);target.chmod(0o755)
        names=sorted(['.']+[str(p.relative_to(root)) for p in root.rglob('*')])
        for name in names:os.utime(root/name,(0,0),follow_symlinks=False)
        packed=subprocess.run(['cpio','--null','-o','-H','newc','-R','0:0','--reproducible','--quiet'],cwd=root,input=b'\0'.join(n.encode() for n in names)+b'\0',capture_output=True,check=True).stdout
        if digest(a.base)!=BASE_SHA256 or digest(a.client)!=client_hash:raise ValueError('input changed')
        with a.output.open('xb') as f:f.write(gzip.compress(packed,compresslevel=9,mtime=0))
    with a.report.open('x') as f:json.dump({'base_sha256':BASE_SHA256,'client_sha256':client_hash,'image_sha256':digest(a.output)},f,indent=2);f.write('\n')
if __name__=='__main__':main()
