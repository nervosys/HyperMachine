from pathlib import Path
import os, subprocess, shutil, hashlib, tempfile, socket, time, json, urllib.request, urllib.error, uuid
iso=Path('/var/tmp/hm-egress-log-mA2CCL')
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
assert (root/'crates/hv2-sandboxd/src/main.rs').read_bytes()==(iso/'crates/hv2-sandboxd/src/main.rs').read_bytes()
protected={'crates/hv2-core/src/backends/kvm.rs':'05bb3d41799f72970df19dc3bced8991c758664bf73de95de2c15a0902079a43','crates/hv2-core/src/boot/linux.rs':'f24a7aeb7f1a493188bad3f83df6d2f0ba5df5959ed7d185f13224872b5712a1','crates/hv2-core/src/boot/source.rs':'4f269436b7bee48bebb2bcef87eb9d4cd7048ad8add634a7e4949764d8a3c23c'}
for rel,digest in protected.items(): assert hashlib.sha256((iso/rel).read_bytes()).hexdigest()==digest
out=Path(tempfile.mkdtemp(prefix='hm-mmio-daemon-api-v1-',dir='/var/tmp'))
binary=Path('/var/tmp/hm-pci-daemon-api-node-v1');assert binary.exists()
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
api=port();proxy=port();assert api!=proxy
credential='owned-pci-'+uuid.uuid4().hex
env=dict(os.environ,HV2_KERNEL='/var/tmp/hm-competitive/bzImage-known-uart-irq',HV2_INITRD='/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz',HV2_CLUSTER_TOKEN=credential,RUST_LOG='info')
args=[str(binary),'--port',str(api),'--proxy-port',str(proxy),'--capacity','8','--require-template','--snapshot-store',str(out/'store')]
log=(out/'daemon.txt').open('w');process=subprocess.Popen(args,cwd=iso,env=env,stdout=log,stderr=subprocess.STDOUT)
checks=[];ids=[];failure=None
print('Owned default MMIO API fixture',out,'daemon pid',process.pid,flush=True)
def call(method,path,payload=None):
 body=None if payload is None else json.dumps(payload).encode()
 request=urllib.request.Request('http://127.0.0.1:'+str(api)+path,data=body,method=method,headers={'content-type':'application/json','x-hv2-cluster-token':credential})
 with urllib.request.urlopen(request,timeout=30) as response:
  raw=response.read();return response.status,json.loads(raw) if raw else None
def record(name):checks.append(name);print('PASS',name,flush=True)
def exact(sandbox,command,expected):
 status,result=call('POST','/sandboxes/'+sandbox+'/exec',{'cmd':command,'args':[],'timeout_secs':15})
 assert status==200 and result['exit_code']==0 and result['stdout']==expected and result['stderr']=='' and result['timed_out']==False,(status,result)
 record('exact command '+sandbox)
try:
 deadline=time.monotonic()+90
 while True:
  if process.poll() is not None:raise RuntimeError('daemon exited during startup')
  try:
   status,templates=call('GET','/templates');assert status==200;break
  except (OSError,urllib.error.URLError):
   if time.monotonic()>deadline:raise
   time.sleep(0.2)
 record('authenticated node startup with required default MMIO template')
 status,parent=call('POST','/v2/sandboxes',{'templateID':'base','timeout':300});assert status==201
 parent=parent['sandboxID'];ids.append(parent);record('API create')
 marker='pci-api-'+uuid.uuid4().hex
 exact(parent,"printf '%s' '"+marker+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",marker)
 status,children=call('POST','/sandboxes/'+parent+'/fork',{'count':2,'timeout':300});assert status==201 and len(children)==2
 assert all('sandbox' in child for child in children),children
 children=[child['sandbox']['sandboxID'] for child in children];ids.extend(children);record('API fork two children')
 for child in children:exact(child,'cat /tmp/pci-api-marker',marker)
 first=marker+'-first';second=marker+'-second'
 exact(children[0],"printf '%s' '"+first+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",first)
 exact(children[1],"printf '%s' '"+second+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",second)
 exact(parent,'cat /tmp/pci-api-marker',marker);record('API sibling and parent write isolation')
 for cycle in range(3):
  status,_=call('POST','/sandboxes/'+parent+'/pause',{});assert 200<=status<300
  exact(children[0],'cat /tmp/pci-api-marker',first)
  status,_=call('POST','/sandboxes/'+parent+'/resume',{});assert 200<=status<300
  exact(parent,'cat /tmp/pci-api-marker',marker);record('API pause resume cycle '+str(cycle))
 status,grandchildren=call('POST','/sandboxes/'+children[0]+'/fork',{'count':1,'timeout':300});assert status==201 and len(grandchildren)==1
 assert 'sandbox' in grandchildren[0],grandchildren
 grandchild=grandchildren[0]['sandbox']['sandboxID'];ids.append(grandchild)
 exact(grandchild,'cat /tmp/pci-api-marker',first);exact(children[1],'cat /tmp/pci-api-marker',second);record('API second-generation fork isolation')
except Exception as error:failure=str(error);print('FAILED',failure,flush=True)
finally:
 for sandbox in reversed(ids):
  try:
   status,_=call('DELETE','/sandboxes/'+sandbox);assert 200<=status<300;record('API deletion '+sandbox)
  except Exception as error:failure=(failure or '')+' cleanup: '+str(error)
 try:
  status,remaining=call('GET','/sandboxes');assert status==200 and remaining==[];record('empty node inventory')
 except Exception as error:failure=(failure or '')+' inventory: '+str(error)
 process.terminate()
 try:process.wait(timeout=15)
 except subprocess.TimeoutExpired:process.kill();process.wait();failure=(failure or '')+' daemon needed forced kill'
 log.close()
 (out/'report.json').write_text(json.dumps({'passed':failure is None,'checks':checks,'failure':failure,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'output':str(out),'daemon_exit':process.returncode},indent=2)+'\n')
 print('Fixture terminal',json.dumps({'passed':failure is None,'checks':len(checks),'output':str(out),'failure':failure}),flush=True)
if failure:raise SystemExit(1)
