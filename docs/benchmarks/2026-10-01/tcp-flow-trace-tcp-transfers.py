import importlib.util,json,os,sys,time
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine');out=Path('/var/tmp/hm-tcp-fixture-diagnostic')
spec=importlib.util.spec_from_file_location('bench',root/'tools/bench-tcp-local.py');bench=importlib.util.module_from_spec(spec);spec.loader.exec_module(bench)
os.sched_setaffinity(0,sorted(os.sched_getaffinity(0))[:8])
original_transfer=bench.transfer;original_popen=bench.subprocess.Popen
def popen(*args,**kwargs):
 if 'env' in kwargs:kwargs['env']=dict(kwargs['env'],RUST_LOG='warn,hv2_core::devices::virtio_vsock=debug')
 return original_popen(*args,**kwargs)
bench.subprocess.Popen=popen
def transfer(connect,payload):
 started=time.perf_counter();events=[]
 class Observed:
  def __init__(self,stream):self.stream=stream
  def sendall(self,data):
   events.append({'event':'send-start','ms':(time.perf_counter()-started)*1000,'bytes':len(data)})
   self.stream.sendall(data)
   events.append({'event':'send-finished','ms':(time.perf_counter()-started)*1000})
  def recv(self,size):
   data=self.stream.recv(size)
   events.append({'event':'received','ms':(time.perf_counter()-started)*1000,'bytes':len(data)})
   return data
  def __getattr__(self,name):return getattr(self.stream,name)
 row=original_transfer(lambda:Observed(connect()),payload);row['trace_events']=events;return row
bench.transfer=transfer
sys.argv=['trace','--hypermachine','/var/tmp/hm-tcp-bench/hv2-sandboxd-release','--firecracker','/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd',str(out/'guest-tcp.cpio.gz'),'--output',str(out/'trace.json'),'--pairs','2','--rounds','1','--fixture-nodelay','--environment','instrumented diagnostic, no performance score']
raise SystemExit(bench.main())
