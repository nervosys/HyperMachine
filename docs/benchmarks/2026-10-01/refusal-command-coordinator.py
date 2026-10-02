import argparse,concurrent.futures,hashlib,importlib.util,json,os,subprocess,tempfile,threading,time,uuid
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--daemon',type=Path,required=True);parser.add_argument('--concurrency',type=int,required=True)
parser.add_argument('--rounds',type=int,required=True);parser.add_argument('--name',required=True);parser.add_argument('--profile',choices=['baseline','candidate'],required=True)
a=parser.parse_args()
assert 1<=a.concurrency<=100 and 1<=a.rounds<=100
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
out=Path('/var/tmp/hm-refusal-retry');out.mkdir(exist_ok=True)
spec=importlib.util.spec_from_file_location('engines',root/'tools/bench-local-engines.py');engines=importlib.util.module_from_spec(spec);spec.loader.exec_module(engines)
paths={'daemon':a.daemon,'kernel':Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),'initrd':Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),'coordinator':Path(__file__),'request_harness':root/'tools/bench-local-engines.py'}
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cores=sorted(os.sched_getaffinity(0))[:8];os.sched_setaffinity(0,cores)
r={'profile':a.profile,'concurrency':a.concurrency,'rounds':a.rounds,'artifact_sha256':{k:digest(p) for k,p in paths.items()},'host_affinity':cores,'success':False,'error':None,'cleanup_errors':[],'rows':[],'preparation':[],'cpu_count':1,'memory_mb':1024,'load_workers':0,'transport':'native HTTP exec with fresh guest-agent connections'}
process=None;nonce=uuid.uuid4().hex
with tempfile.TemporaryDirectory(prefix='hm-retry-',dir='/var/tmp') as directory:
 directory=Path(directory);port,proxy=engines.free_port(),engines.free_port()
 while port==proxy:proxy=engines.free_port()
 url=f'http://127.0.0.1:{port}'
 def request(method,path,body=None):return engines.request(url,method,path,body)
 def prepare(index):
  record={'index':index,'success':False,'sandbox_id':None};started=time.perf_counter()
  try:
   response=request('POST','/v2/sandboxes',{'templateID':'base','timeout':300,'allowInternetAccess':False})
   record['sandbox_id']=response['sandboxID']
   info=request('GET','/sandboxes/'+record['sandbox_id'])
   assert info['cpuCount']==1 and info['memoryMB']==1024
   marker=f'hm-retry-{nonce}-{index}'
   value=request('POST',f"/sandboxes/{record['sandbox_id']}/exec",{'cmd':f"printf '%s' '{marker}'",'timeout_secs':10})
   assert value['exit_code']==0 and value['stdout']==marker and not value.get('timed_out') and not value.get('truncated')
   record['success']=True
  except Exception as error:record['error']=str(error)
  record['elapsed_ms']=(time.perf_counter()-started)*1000
  return record
 try:
  with (directory/'node.log').open('wb') as log:
   process=subprocess.Popen([str(a.daemon),'--port',str(port),'--proxy-port',str(proxy),'--memory-mb','1024','--cpu-cores','1','--capacity',str(a.concurrency+4),'--volume-dir',str(directory/'volumes'),'--snapshot-store',str(directory/'snapshots')],env={'PATH':'/usr/local/bin:/usr/bin:/bin','RUST_LOG':'warn','HV2_KERNEL':str(paths['kernel']),'HV2_INITRD':str(paths['initrd'])},stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
   deadline=time.monotonic()+30
   while True:
    if process.poll() is not None:raise RuntimeError('daemon exited during setup')
    try:templates=request('GET','/templates');break
    except OSError:
     if time.monotonic()>=deadline:raise
     time.sleep(.01)
   base=next(item for item in templates if 'base' in item.get('aliases',[]));assert base['snapshot'] is True
   r['template']=base
   with concurrent.futures.ThreadPoolExecutor(max_workers=a.concurrency) as pool:r['preparation']=list(pool.map(prepare,range(a.concurrency)))
   clocks={};barrier=threading.Barrier(a.concurrency)
   def commands(prepared):
    rows=[]
    for round_index in range(a.rounds):
     row={'guest_index':prepared['index'],'round':round_index,'success':False}
     barrier.wait(timeout=120)
     start=time.perf_counter();row['started_at_seconds']=start
     try:
      if not prepared['success']:raise RuntimeError('guest preparation failed: '+prepared.get('error','unknown'))
      marker=f"hm-retry-{nonce}-{prepared['index']}-{round_index}"
      value=request('POST',f"/sandboxes/{prepared['sandbox_id']}/exec",{'cmd':f"printf '%s' '{marker}'",'timeout_secs':10})
      row['latency_ms']=(time.perf_counter()-start)*1000
      if value.get('exit_code')!=0 or value.get('stdout')!=marker or value.get('timed_out') or value.get('truncated'):raise RuntimeError('guest command output/status mismatch')
      row['success']=True
     except Exception as error:row.update(error=str(error),failure_elapsed_ms=(time.perf_counter()-start)*1000)
     rows.append(row)
    return rows
   with concurrent.futures.ThreadPoolExecutor(max_workers=a.concurrency) as pool:
    for rows in pool.map(commands,r['preparation']):r['rows'].extend(rows)
   r['command_batches']=[{'round':index,'start_spread_ms':(max(v)-min(v))*1000} for index in range(a.rounds) for v in [[row['started_at_seconds'] for row in r['rows'] if row['round']==index]]]
 except Exception as error:r['error']=str(error)
 finally:
  if process is not None and process.poll() is None:
   for prepared in r['preparation']:
    if prepared['sandbox_id']:
     try:request('DELETE','/sandboxes/'+prepared['sandbox_id'])
     except Exception as error:r['cleanup_errors'].append(str(error))
   try:
    remaining=request('GET','/sandboxes');r['remaining_sandbox_count']=len(remaining)
    if remaining:
     r['cleanup_errors'].append('owned node retained records')
     for record in remaining:request('DELETE','/sandboxes/'+record['sandboxID'])
   except Exception as error:r['cleanup_errors'].append(str(error))
  if process is not None and not engines.stop(process):r['cleanup_errors'].append('daemon did not stop')
  if (directory/'node.log').exists():r['node_log_tail']=(directory/'node.log').read_bytes()[-12000:].decode(errors='replace')
r['artifacts_unchanged']=all(digest(p)==r['artifact_sha256'][key] for key,p in paths.items())
r['attempts']=len(r['rows']);r['passed']=sum(row['success'] for row in r['rows'])
r['success']=not r['error'] and not r['cleanup_errors'] and r['artifacts_unchanged'] and r['remaining_sandbox_count']==0 and r['attempts']==a.concurrency*a.rounds and r['passed']==r['attempts'] and all(p['success'] for p in r['preparation'])
(out/(a.name+'.json')).write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'name':a.name,'success':r['success'],'error':r['error'],'attempts':r['attempts'],'passed':r['passed'],'latency_ms':engines.fc.summary([row['latency_ms'] for row in r['rows'] if row['success']])}),flush=True)
raise SystemExit(0 if r['success'] else 1)