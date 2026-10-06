from pathlib import Path
import os,subprocess,socket,time,json,urllib.request,urllib.error,uuid,tempfile,hashlib,math,platform
binary=Path('/var/tmp/hm-transport-release-node-v1');assert binary.exists()
iso=Path('/var/tmp/hm-egress-log-mA2CCL');out=Path(tempfile.mkdtemp(prefix='hm-transport-release-abba-v1-',dir='/var/tmp'))
affinity=sorted(os.sched_getaffinity(0))[:8];assert len(affinity)==8
context={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'affinity':affinity,'kernel_release':platform.release(),'hz':os.sysconf('SC_CLK_TCK'),'order':['mmio','pci','pci','mmio'],'samples_per_cohort':16,'warmups_per_cohort':2}
(out/'context.json').write_text(json.dumps(context,indent=2)+'\n')
print('Matched release ABBA',out,flush=True)
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
def resource(pid):
 stat=Path('/proc/'+str(pid)+'/stat').read_text().split(') ',1)[1].split()
 pss=next(int(line.split()[1]) for line in Path('/proc/'+str(pid)+'/smaps_rollup').read_text().splitlines() if line.startswith('Pss:'))
 return {'pid':pid,'start_ticks':int(stat[19]),'cpu_ticks':int(stat[11])+int(stat[12]),'pss_kib':pss}
cohorts=[]
for index,transport in enumerate(context['order']):
 directory=out/('cohort-'+str(index+1)+'-'+transport);directory.mkdir()
 api=port();proxy=port();assert api!=proxy;credential='owned-benchmark-'+uuid.uuid4().hex
 env=dict(os.environ,HV2_KERNEL='/var/tmp/hm-competitive/bzImage-known-uart-irq',HV2_INITRD='/var/tmp/hm-private-capacity-buffered-guest-v1.cpio.gz',HV2_CLUSTER_TOKEN=credential,RUST_LOG='info')
 log=(directory/'daemon.txt').open('w')
 process=subprocess.Popen([str(binary),'--guest-transport',transport,'--port',str(api),'--proxy-port',str(proxy),'--capacity','2','--require-template','--snapshot-store',str(directory/'store')],cwd=iso,env=env,stdout=log,stderr=subprocess.STDOUT,preexec_fn=lambda:os.sched_setaffinity(0,affinity))
 ids=[];samples=[];failure=None
 def call(method,path,payload=None):
  req=urllib.request.Request('http://127.0.0.1:'+str(api)+path,data=None if payload is None else json.dumps(payload).encode(),method=method,headers={'content-type':'application/json','x-hv2-cluster-token':credential})
  with urllib.request.urlopen(req,timeout=30) as response:
   raw=response.read();return response.status,json.loads(raw) if raw else None
 try:
  deadline=time.monotonic()+90
  while True:
   assert process.poll() is None,'daemon terminated during readiness'
   try:
    status,_=call('GET','/templates')
    if status==200:break
   except (OSError,urllib.error.URLError):pass
   assert time.monotonic()<deadline,'readiness deadline';time.sleep(.2)
  for sample_index in range(18):
   marker='exact-release-'+uuid.uuid4().hex
   before=resource(process.pid);started=time.perf_counter_ns()
   status,descriptor=call('POST','/v2/sandboxes',{'templateID':'base','timeout':300});created=time.perf_counter_ns()
   assert status==201;sid=descriptor['sandboxID'];ids.append(sid)
   status,result=call('POST','/sandboxes/'+sid+'/exec',{'cmd':"printf '%s' '"+marker+"'",'args':[],'timeout_secs':15});answered=time.perf_counter_ns()
   assert status==200 and result['exit_code']==0 and result['stdout']==marker and result['stderr']=='' and result['timed_out']==False,(status,result)
   held=resource(process.pid);assert before['start_ticks']==held['start_ticks']
   status,_=call('DELETE','/sandboxes/'+sid);assert 200<=status<300;ids.remove(sid)
   status,inventory=call('GET','/sandboxes');assert status==200 and inventory==[]
   samples.append({'sample':sample_index,'warmup':sample_index<2,'create_ms':(created-started)/1e6,'command_ms':(answered-created)/1e6,'total_ms':(answered-started)/1e6,'before':before,'held':held,'exact_command':True,'deleted':True,'empty_inventory':True})
  print('Cohort',index+1,transport,'18 exact/create/delete gates passed',flush=True)
 except Exception as error:failure=str(error)
 finally:
  for sid in ids:
   try:call('DELETE','/sandboxes/'+sid)
   except Exception as error:failure=(failure or '')+' cleanup '+str(error)
  process.terminate()
  try:process.wait(timeout=15)
  except subprocess.TimeoutExpired:process.kill();process.wait();failure=(failure or '')+' forced daemon kill'
  log.close()
  report={'transport':transport,'passed':failure is None,'failure':failure,'samples':samples,'daemon_exit':process.returncode}
  (directory/'report.json').write_text(json.dumps(report,indent=2)+'\n');cohorts.append(report)
 if failure:
  print('FAILED',failure,'output',out,flush=True);raise SystemExit(1)
def quantile(values,p):return sorted(values)[math.ceil(len(values)*p)-1]
metrics={}
for transport in ['mmio','pci']:
 values=[sample for cohort in cohorts if cohort['transport']==transport for sample in cohort['samples'] if not sample['warmup']]
 metrics[transport]={'samples':len(values),'latencies':{field:{'p50':quantile([v[field] for v in values],.5),'p95':quantile([v[field] for v in values],.95)} for field in ['create_ms','command_ms','total_ms']},'cpu_ms_per_operation':sum((v['held']['cpu_ticks']-v['before']['cpu_ticks'])*1000/context['hz'] for v in values)/len(values),'held_pss_kib_p50':quantile([v['held']['pss_kib'] for v in values],.5)}
(out/'metrics.json').write_text(json.dumps(metrics,indent=2)+'\n')
print('ABBA complete',out,json.dumps(metrics),flush=True)
