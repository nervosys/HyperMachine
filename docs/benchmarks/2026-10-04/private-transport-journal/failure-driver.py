from pathlib import Path
import sys,runpy,subprocess,json,hashlib
output=Path('/var/tmp/hm-private-transport-journal-failure-v1');assert not output.exists()
checker=Path('/var/tmp/hm-private-transport-journal-injected-checker-v1.py')
command=['python3',str(checker),'--daemon','/var/tmp/hm-private-transport-parallel-dev-node-v1','--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-guest-tcp','--private-cross-node','--private-guest-revocation','--private-transport-samples','32']
processes=[];original=subprocess.Popen
class TrackedPopen(original):
 def __init__(self,*args,**kwargs):
  super().__init__(*args,**kwargs);processes.append(self)
subprocess.Popen=TrackedPopen;sys.argv=command[1:];failure=None
try:runpy.run_path(str(checker),run_name='__main__')
except ValueError as error:failure=str(error)
finally:subprocess.Popen=original
assert failure=='matched private transport benchmark includes failed scored requests',failure
rows=[json.loads(line) for line in (output/'private-transport-rows.jsonl').read_text().splitlines()]
b=json.loads((output/'private-transport-benchmark.json').read_text());assert rows==b['rows'] and len(rows)==136
bad=[row for row in rows if not row['success']];assert len(bad)==1 and bad[0]['pair']==0 and bad[0]['path']=='private' and bad[0]['payload_bytes']==64 and bad[0]['failure_type']=='TimeoutError'
assert sum(s['failed'] for s in b['summary'])==1 and sum(s['successful'] for s in b['summary'])==127
assert not (output/'report.json').exists()
assert processes and all(p.poll() is not None for p in processes)
result={'expected_failure_observed':True,'journal_rows':len(rows),'failed_scored_operations':1,'successful_scored_operations':127,'false_success_report_absent':True,'all_tracked_child_processes_reaped':True,'tracked_process_count':len(processes),'production_checker_sha256':hashlib.sha256(Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine/tools/check-udp-cluster-kvm.py').read_bytes()).hexdigest(),'injected_checker_sha256':hashlib.sha256(checker.read_bytes()).hexdigest()}
(output/'failure-verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
