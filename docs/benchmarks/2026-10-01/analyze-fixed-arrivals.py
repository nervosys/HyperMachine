import hashlib,json,math
from pathlib import Path
root=Path(__file__).resolve().parents[1] if Path(__file__).parent.name=='target' else Path(__file__).resolve().parents[3]
out=root/'docs/benchmarks/2026-10-01'
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def stats(values):
 values=sorted(values)
 return {'p50':values[math.ceil(.5*len(values))-1],'p99':values[math.ceil(.99*len(values))-1]}
rows=[]
for rate,execution,coordinator in [(5,'fixed-arrivals-execution.json','fixed-arrivals-probe.py'),(25,'fixed-arrivals-25ps-execution.json','fixed-arrivals-25ps-probe.py')]:
 path=out/f'fixed-arrivals-{rate}ps.json'; report=json.loads(path.read_bytes())
 run=json.loads((out/execution).read_bytes())
 assert run['exit_code']==0 and run['coordinator_sha256']==digest(out/coordinator)
 assert report['artifacts_unchanged'] and report['success'] and not report['cleanup_errors'] and report['remaining_sandbox_count']==0
 assert report['artifact_sha256']['harness']==digest(out/'fixed-arrivals-harness.py')
 assert report['driver_cpu_affinity']==run['affinity'] and report['arrival_rate']==rate
 assert [b['engine'] for b in report['batches']]==['hypermachine','firecracker','firecracker','hypermachine']
 for engine in ['hypermachine','firecracker']:
  batches=[b for b in report['batches'] if b['engine']==engine]
  records=[r for b in batches for r in b['samples']]
  assert len(records)==report['arrival_samples']*2
  for r in records:
   assert r['success'] and r['cleanup_success']
   assert r['scheduled_offset_ms']==r['index']/rate*1000
   assert r['client_queue_ms']==r['worker_start_offset_ms']-r['submitted_offset_ms']
   assert r['scheduled_to_validation_ms']==r['validated_offset_ms']-r['scheduled_offset_ms']
   assert r['cleanup_completed_offset_ms']>=r['validated_offset_ms']
  rows.append({'rate':rate,'engine':engine,'attempts':len(records),'passed':len(records),
    'scheduled_command_ready_ms':stats([r['start_offset_ms']+r['ready_ms']-r['scheduled_offset_ms'] for r in records]),
    'scheduled_validation_ms':stats([r['scheduled_to_validation_ms'] for r in records]),
    'client_queue_ms':stats([r['client_queue_ms'] for r in records]),
    'completed_lifecycles_per_second':[b['completed_lifecycles_per_second'] for b in batches],
    'raw_sha256':digest(path)})
(out/'fixed-arrivals-summary.json').write_text(json.dumps({'attempts':sum(r['attempts'] for r in rows),'rows':rows},indent=2)+'\n')
text='''# Matched fixed-rate native arrivals (2026-10-01)

Both engines receive the same fixed-rate schedule with eight client workers, one vCPU and 1024 MiB per guest, identical kernel/initrd, and eight pinned host CPUs on shared nested KVM. Two pairs alternate HM/FC then FC/HM at each rate. HyperMachine uses the event-preserving build identified in [its build evidence](kvm-events.md); this is not a fresh build of the later snapshot-ID compatibility change. Exact executable/image/harness hashes are in each raw report. Firecracker is v1.17.0.

A planned arrival is submitted independently of previous completions. The bounded worker pool queues excess work; queue delay is measured from submission to worker entry, and submission lag from planned arrival to submission. Each successful guest produces its unique command marker and passes resource verification before immediate cleanup. No batch-wide hold is used. Completion rates include cleanup and the final drain, with all attempted arrivals in the denominator. Successful command latency is derived from raw start offset plus command readiness duration minus planned arrival offset; it includes submission lag, queueing and setup after worker entry. The raw report's `scheduled_ready_ms` field refers to the later resource-verification callback, not only the command; both are retained distinctly in the summary.

| Offered arrivals/s | Engine | Passed / attempted | Scheduled command P50 / P99 (ms) | Client queue P50 / P99 (ms) | Completed lifecycles/s by pair |
|---|---|---|---|---|---|
'''
for r in rows:
 a=r['scheduled_command_ready_ms'];q=r['client_queue_ms']
 text+=f"| {r['rate']} | {r['engine']} | {r['passed']} / {r['attempts']} | {a['p50']:.2f} / {a['p99']:.2f} | {q['p50']:.2f} / {q['p99']:.2f} | "+' / '.join(f'{x:.2f}' for x in r['completed_lifecycles_per_second'])+' |\n'
text+='''
All 560 attempts passed, with unchanged artifacts and no sandbox records left after cleanup. At five arrivals/s, both engines keep near the offered rate over the short schedule. At 25 arrivals/s, both accumulate client queues and drain below the offered rate; Firecracker has lower command tails and higher completed lifecycle rate in these pairs. The client limit includes cleanup, so these figures do not establish a server-only maximum, a sustained fleet capacity, an SLA or an across-the-board win. The first-to-last planned arrival intervals are 7.8 seconds at rate 5 and 3.96 seconds at rate 25; longer arrivals, overload failures, recovery across load changes and multi-node runs remain unverified. The memory baseline is informational; this run measures no held guest PSS or density.

The 17 synchronization, queue, failure-accounting and cleanup tests pass on Windows and Linux. Raw per-attempt reports, execution manifests, exact executed harness/coordinators and an analyzer are retained beside this document. Failures would invalidate the cohort and remain in its denominator; these two bounded profiles happened to have none.
'''
(out/'fixed-arrivals.md').write_text(text)
print(json.dumps({'attempts':sum(r['attempts'] for r in rows),'rows':rows}))