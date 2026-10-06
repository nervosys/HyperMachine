from pathlib import Path
import os, subprocess, shutil, hashlib, tempfile, socket, time, json, urllib.request, urllib.error, uuid, http.client, http.server, threading
iso=Path('/var/tmp/hm-egress-log-mA2CCL')
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
# Candidate remains isolated until runtime acceptance; source identity is catalogued.
protected={'crates/hv2-core/src/backends/kvm.rs':'05bb3d41799f72970df19dc3bced8991c758664bf73de95de2c15a0902079a43','crates/hv2-core/src/boot/linux.rs':'f24a7aeb7f1a493188bad3f83df6d2f0ba5df5959ed7d185f13224872b5712a1','crates/hv2-core/src/boot/source.rs':'4f269436b7bee48bebb2bcef87eb9d4cd7048ad8add634a7e4949764d8a3c23c'}
for rel,digest in protected.items(): assert hashlib.sha256((iso/rel).read_bytes()).hexdigest()==digest
out=Path(tempfile.mkdtemp(prefix='hm-pci-fastboot-network-v1-',dir='/var/tmp'))
binary=Path('/var/tmp/hm-pci-fastboot-release-node-v1');assert binary.exists()
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
api=port();proxy=port();assert api!=proxy
credential='owned-pci-'+uuid.uuid4().hex
env=dict(os.environ,HV2_KERNEL='/var/tmp/hm-competitive/bzImage-known-uart-irq',HV2_INITRD='/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz',HV2_CLUSTER_TOKEN=credential,RUST_LOG='info')
host_ip=json.loads(subprocess.check_output(['ip','-j','-4','addr','show','dev','eth0']))[0]['addr_info'][0]['local']
received=[]
class OwnedHTTP(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  payload=self.path.removeprefix('/').encode();received.append(self.path)
  self.send_response(200);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
 def log_message(self,*args):pass
owned=http.server.ThreadingHTTPServer((host_ip,0),OwnedHTTP)
threading.Thread(target=owned.serve_forever,daemon=True).start()
owned_port=owned.server_address[1]
args=[str(binary),'--guest-transport','pci','--port',str(api),'--proxy-port',str(proxy),'--capacity','8','--network','--egress-default','deny','--tenant-reserved-cidr',host_ip+'/32','--require-template','--snapshot-store',str(out/'store')]
log=(out/'daemon.txt').open('w');process=subprocess.Popen(args,cwd=iso,env=env,stdout=log,stderr=subprocess.STDOUT)
checks=[];ids=[];failure=None
print('Owned PCI API fixture',out,'daemon pid',process.pid,flush=True)
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
def nic_http(sandbox):
 marker='nic-'+uuid.uuid4().hex
 exact(sandbox,'wget -q -T 10 -O - http://'+host_ip+':'+str(owned_port)+'/'+marker,marker)
 assert '/'+marker in received
 record('exact NIC HTTP '+sandbox)

def guest_http(sandbox,expected):
 connection=http.client.HTTPConnection('127.0.0.1',proxy,timeout=15)
 try:
  connection.request('GET','/pci-api-marker',headers={'Host':'8080-'+sandbox+'.localhost'})
  response=connection.getresponse();body=response.read()
  assert response.status==200 and body==expected.encode(),(response.status,body[:200])
 finally:connection.close()
 record('exact guest HTTP '+sandbox)

try:
 deadline=time.monotonic()+90
 while True:
  if process.poll() is not None:raise RuntimeError('daemon exited during startup')
  try:
   status,templates=call('GET','/templates');assert status==200;break
  except (OSError,urllib.error.URLError):
   if time.monotonic()>deadline:raise
   time.sleep(0.2)
 record('authenticated node startup with required PCI template')
 status,parent=call('POST','/v2/sandboxes',{'templateID':'base','timeout':300,'network':{'allowOut':[host_ip+'/32']}});assert status==201
 parent=parent['sandboxID'];ids.append(parent);record('API create')
 marker='pci-api-'+uuid.uuid4().hex
 exact(parent,"printf '%s' '"+marker+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",marker)
 exact(parent,"busybox httpd -p 8080 -h /tmp; printf '%s' 'http-server-ready'",'http-server-ready')
 guest_http(parent,marker);nic_http(parent)
 status,children=call('POST','/sandboxes/'+parent+'/fork',{'count':2,'timeout':300});assert status==201 and len(children)==2
 assert all('sandbox' in child for child in children),children
 children=[child['sandbox']['sandboxID'] for child in children];ids.extend(children);record('API fork two children')
 for child in children:
  exact(child,'cat /tmp/pci-api-marker',marker);guest_http(child,marker);nic_http(child)
 first=marker+'-first';second=marker+'-second'
 exact(children[0],"printf '%s' '"+first+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",first)
 exact(children[1],"printf '%s' '"+second+"' > /tmp/pci-api-marker; cat /tmp/pci-api-marker",second)
 exact(parent,'cat /tmp/pci-api-marker',marker)
 guest_http(parent,marker);guest_http(children[0],first);guest_http(children[1],second)
 record('API sibling and parent write isolation')
 for cycle in range(3):
  status,_=call('POST','/sandboxes/'+parent+'/pause',{});assert 200<=status<300
  exact(children[0],'cat /tmp/pci-api-marker',first)
  status,_=call('POST','/sandboxes/'+parent+'/resume',{});assert 200<=status<300
  exact(parent,'cat /tmp/pci-api-marker',marker);guest_http(parent,marker);guest_http(children[0],first);nic_http(parent);record('API pause resume cycle '+str(cycle))
 status,grandchildren=call('POST','/sandboxes/'+children[0]+'/fork',{'count':1,'timeout':300});assert status==201 and len(grandchildren)==1
 assert 'sandbox' in grandchildren[0],grandchildren
 grandchild=grandchildren[0]['sandbox']['sandboxID'];ids.append(grandchild)
 exact(grandchild,'cat /tmp/pci-api-marker',first);exact(children[1],'cat /tmp/pci-api-marker',second)
 guest_http(grandchild,first);guest_http(children[1],second);nic_http(grandchild);record('API second-generation fork isolation')
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
 log.close();owned.shutdown();owned.server_close()
 (out/'report.json').write_text(json.dumps({'passed':failure is None,'checks':checks,'failure':failure,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'output':str(out),'daemon_exit':process.returncode,'owned_nic_http_requests':len(received)},indent=2)+'\n')
 print('Fixture terminal',json.dumps({'passed':failure is None,'checks':len(checks),'output':str(out),'failure':failure}),flush=True)
if failure:raise SystemExit(1)
