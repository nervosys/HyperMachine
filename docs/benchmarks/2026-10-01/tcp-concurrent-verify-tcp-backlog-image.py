import gzip,hashlib,json
from pathlib import Path
def entries(path):
 raw=gzip.decompress(path.read_bytes());offset=0;result={}
 while True:
  header=raw[offset:offset+110];assert header[:6]==b'070701'
  fields=[int(header[6+8*i:14+8*i],16) for i in range(13)];offset+=110
  name=raw[offset:offset+fields[11]-1].decode();offset=(offset+fields[11]+3)//4*4
  data=raw[offset:offset+fields[6]];offset=(offset+fields[6]+3)//4*4
  if name=='TRAILER!!!':break
  assert name not in result
  result[name]=([fields[i] for i in [1,2,3,4,5,7,8,9,10]],data)
 return result
source=Path('/var/tmp/hm-tcp/guest-tcp.cpio.gz');output=Path('/var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz')
a,b=entries(source),entries(output);assert set(a)==set(b)
changed=[n for n in a if a[n][1]!=b[n][1]];metadata=[n for n in a if a[n][0]!=b[n][0]]
assert changed==['bin/hv2-guest-agentd'],changed;assert not metadata,metadata
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
r={'success':True,'source_sha256':sha(source),'output_sha256':sha(output),'changed_contents':changed,'changed_metadata':metadata,'entry_sets_equal':True,'entries':len(a),'verifier_sha256':sha(Path(__file__))}
Path('/var/tmp/hm-tcp-backlog/image-verification.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r))
