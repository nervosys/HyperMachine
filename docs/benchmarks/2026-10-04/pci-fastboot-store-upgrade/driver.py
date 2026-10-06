from pathlib import Path
import os,json,hashlib,tempfile,subprocess,socket,time,urllib.request,urllib.error,uuid,shutil
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
identity=json.loads((root/'docs/benchmarks/2026-10-04/pci-fastboot/source-context.json').read_text())
binaries={'before':Path('/var/tmp/hm-transport-release-node-v1'),'after':Path('/var/tmp/hm-pci-fastboot-release-node-v1')}
for phase,key in [('before','baseline_binary_sha256'),('after','candidate_binary_sha256')]:assert hashlib.sha256(binaries[phase].read_bytes()).hexdigest()==identity[key]
assert hashlib.sha256((root/'crates/hv2-sandboxd/src/main.rs').read_bytes()).hexdigest()==identity['main_candidate_sha256']
out=Path(tempfile.mkdtemp(prefix='hm-pci-fastboot-store-upgrade-v1-',dir='/var/tmp'));print(out,flush=True)
context={'binary_sha256':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in binaries.items()},'profiles':['pci','mmio'],'cpu_affinity':list(range(8)),'inputs_sha256':{p:hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in ['/var/tmp/hm-competitive/bzImage-known-uart-irq','/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz']}}
(out/'context.json').write_text(json.dumps(context,indent=2)+'\n')
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
for mode in context['profiles']:
 directory=out/mode;directory.mkdir();store=directory/'store';credential='owned-upgrade-'+uuid.uuid4().hex;process=None;log=None;checks=[];ids=[];failure=None;exits=[];cache=[];snapshots=[]
 def check(name):checks.append(name);print('PASS',mode,name,flush=True)
 def call(method,path,payload=None):
  req=urllib.request.Request(url+path,data=None if payload is None else json.dumps(payload).encode(),method=method,headers={'content-type':'application/json','x-hv2-cluster-token':credential})
  with urllib.request.urlopen(req,timeout=30) as r:
   b=r.read();return r.status,json.loads(b) if b else None
 def exact(sid,command,expected):
  status,r=call('POST','/sandboxes/'+sid+'/exec',{'cmd':command,'args':[],'timeout_secs':15});assert status==200 and r['exit_code']==0 and r['stdout']==expected and r['stderr']=='' and not r['timed_out'];check('exact saved guest command')
 def stop():
  global process,log
  if process is not None:
   process.terminate();process.wait(timeout=15);exits.append(process.returncode);assert process.returncode==0;process=None;log.close();log=None
 def start(phase):
  global process,log,url
  api=port();proxy=port();assert api!=proxy;url='http://127.0.0.1:'+str(api)
  env={'PATH':'/usr/local/bin:/usr/bin:/bin','HV2_KERNEL':'/var/tmp/hm-competitive/bzImage-known-uart-irq','HV2_INITRD':'/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz','HV2_CLUSTER_TOKEN':credential,'RUST_LOG':'info'}
  log=(directory/(phase+'-daemon.txt')).open('w');process=subprocess.Popen([str(binaries[phase]),'--guest-transport',mode,'--port',str(api),'--proxy-port',str(proxy),'--capacity','8','--require-template','--snapshot-store',str(store)],env=env,stdout=log,stderr=subprocess.STDOUT,preexec_fn=lambda:os.sched_setaffinity(0,set(range(8))))
  deadline=time.monotonic()+90
  while True:
   assert process.poll() is None
   try:
    status,templates=call('GET','/templates')
    if status==200:break
   except (OSError,urllib.error.URLError):pass
   assert time.monotonic()<deadline;time.sleep(.1)
  assert any('base' in t.get('aliases',[]) and t['snapshot'] for t in templates);check(phase+' required-template startup')
 try:
  start('before');status,parent=call('POST','/v2/sandboxes',{'templateID':'base','timeout':300});assert status==201
  original=parent;sid=parent['sandboxID'];ids.append(sid);marker='upgrade-'+uuid.uuid4().hex
  exact(sid,"printf '%s' '"+marker+"' > /tmp/upgrade-marker; cat /tmp/upgrade-marker",marker)
  old_keys=sorted(p.name for p in (store/'templates').iterdir() if p.is_dir());assert len(old_keys)==1;cache.append(old_keys)
  status,_=call('POST','/sandboxes/'+sid+'/pause',{});assert 200<=status<300;check('old release guest paused into store')
  snapshots=[{'path':str(p.relative_to(store)),'size':p.stat().st_size} for p in (store/'paused').rglob('*') if p.is_file()];assert snapshots
  stop();check('old release clean shutdown preserves paused guest')
  start('after');new_keys=sorted(p.name for p in (store/'templates').iterdir() if p.is_dir());cache.append(new_keys)
  if mode=='mmio':assert new_keys==old_keys;check('existing MMIO template key reused')
  else:assert len(new_keys)==2 and set(old_keys)<set(new_keys);check('new PCI template key coexists with old base')
  status,resumed=call('POST','/sandboxes/'+sid+'/resume',{});assert 200<=status<300;assert resumed['sandboxID']==sid and resumed['envdAccessToken']==original['envdAccessToken'];check('same guest identity and access token after upgrade')
  exact(sid,'cat /tmp/upgrade-marker',marker)
  status,children=call('POST','/sandboxes/'+sid+'/fork',{'count':1,'timeout':300});assert status==201 and len(children)==1;child=children[0]['sandbox']['sandboxID'];ids.append(child);exact(child,'cat /tmp/upgrade-marker',marker)
  child_marker=marker+'-child';exact(child,"printf '%s' '"+child_marker+"' > /tmp/upgrade-marker; cat /tmp/upgrade-marker",child_marker);exact(sid,'cat /tmp/upgrade-marker',marker);check('upgraded guest fork isolation')
  for cycle in range(2):
   status,_=call('POST','/sandboxes/'+sid+'/pause',{});assert 200<=status<300;exact(child,'cat /tmp/upgrade-marker',child_marker)
   status,_=call('POST','/sandboxes/'+sid+'/resume',{});assert 200<=status<300;exact(sid,'cat /tmp/upgrade-marker',marker);check('candidate disk resume cycle '+str(cycle))
 except Exception as e:failure=str(e);print('FAILED',mode,failure,flush=True)
 finally:
  if process is not None:
   for sid in reversed(ids):
    try:status,_=call('DELETE','/sandboxes/'+sid);assert 200<=status<300;check('guest deleted')
    except Exception as e:failure=(failure or '')+' cleanup '+str(e)
   try:status,inventory=call('GET','/sandboxes');assert status==200 and inventory==[];check('empty inventory')
   except Exception as e:failure=(failure or '')+' inventory '+str(e)
   try:stop()
   except Exception as e:failure=(failure or '')+' shutdown '+str(e)
  report={'mode':mode,'passed':failure is None,'failure':failure,'checks':checks,'daemon_exits':exits,'cache_keys':cache,'paused_files_before_upgrade':snapshots}
  if failure is None:
   assert store.resolve().parent==directory.resolve() and directory.resolve().parent==out.resolve();shutil.rmtree(store);report['owned_store_removed']=not store.exists()
  (directory/'report.json').write_text(json.dumps(report,indent=2)+'\n')
 if failure:raise SystemExit(1)
print('Both upgrade profiles completed with clean shutdown and owned-store removal.',flush=True)
