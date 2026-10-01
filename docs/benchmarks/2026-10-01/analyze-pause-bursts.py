import hashlib, json, math, shutil
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
source=Path('/var/tmp/hm-pause-bursts')
out=root/'docs/benchmarks/2026-10-01'
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def stats(values):
 values=sorted(values)
 return {key:values[math.ceil(frac*len(values))-1] for key,frac in [('p50',.5),('p99',.99)]}
rows=[]; failures=[]
for c,n in [(1,100),(8,104),(50,100),(100,200)]:
 path=source/f'pause-resume-sweep-c{c}.json'
 report=json.loads(path.read_bytes()); sdk=report['sdk']; records=sdk['samples']
 assert len(records)==n and sdk['concurrency']==c
 assert sdk['synchronized_pause_batches'] and sdk['synchronized_operation_batches']
 assert report['artifacts_unchanged'] and not report['cleanup_errors'] and report['remaining_sandbox_count']==0
 assert all(report['controlled_cpu_load'][k] for k in ['all_alive_through_cohort','workers_cleaned_up'])
 for key,p in [('sdk_harness',root/'tools/bench-e2b-sdk.py'),('coordinator',root/'target/pause-burst-coordinator.py')]:
  assert report['artifact_sha256'][key]==digest(p)
 good=[r for r in records if r['success']]
 for r in good:
  assert r['cpu_count']==1 and r['memory_mb']==1024
  assert r['pause_start_offset_ms']<=r['operation_start_offset_ms']
 for batch in sdk['pause_batches']:
  offsets=[r['pause_start_offset_ms'] for r in records if r['batch_index']==batch['batch_index'] and r.get('pause_start_offset_ms') is not None]
  assert batch['pauses_started']==len(offsets)
  assert batch['pause_start_spread_ms']==(max(offsets)-min(offsets) if offsets else None)
 bad=[r for r in records if not r['success']]
 assert sdk['failed_samples']==len(bad) and sdk['successful_samples']==len(good)
 assert report['sdk_exit_code']==(1 if bad else 0)
 failures.extend({'concurrency':c,**r} for r in bad)
 rows.append({'concurrency':c,'attempts':n,'passed':len(good),'pause_ms':stats([r['pause_ms'] for r in good]),'resume_ready_ms':stats([r['ready_ms'] for r in good]),'max_pause_start_spread_ms':max(b['pause_start_spread_ms'] for b in sdk['pause_batches']),'raw_sha256':digest(path)})
 shutil.copyfile(path,out/path.name)
for name,src in [('pause-burst-coordinator.py',root/'target/pause-burst-coordinator.py'),('pause-burst-matrix.py',root/'target/pause-burst-matrix.py'),('pause-burst-sdk-harness.py',root/'tools/bench-e2b-sdk.py'),('analyze-pause-bursts.py',Path(__file__))]:
 shutil.copyfile(src,out/name)
summary={'attempts':sum(r['attempts'] for r in rows),'passed':sum(r['passed'] for r in rows),'rows':rows,'failures':failures}
(out/'pause-bursts-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
text='''# Synchronized pause and resume bursts (2026-10-01)

All live-process memory, filesystem and boot-ID probes are prepared before a separate pause barrier. Each pause call is timed through its SDK response, then paused state is verified. A second barrier waits for the whole batch before timing resume through the first successful state-verifying command. Successful samples also verify guest resources and cleanup; pause latency below is conditional on the whole lifecycle succeeding. Start spread is the observed client spread, not a guarantee of simultaneous server arrival.

The runtime is the event-preserving KVM build from [the matched repair experiment](kvm-events.md), with identical kernel/initrd and SDK dependencies. Each concurrency starts a fresh owned node on shared nested KVM, eight host CPUs and one pinned busy worker; guests use one vCPU and 1024 MiB. This is a single-runtime sweep, with no matched competitor or parent comparison. High-concurrency cohorts contain only two batches. No speed win or reliability SLA is established.

| Concurrency | Passed / attempted | Pause P50 / P99 (ms) | Resume-to-command P50 / P99 (ms) | Max pause client start spread (ms) |
|---|---|---|---|---|
'''
for r in rows:
 p=r['pause_ms'];v=r['resume_ready_ms']
 text+=f"| {r['concurrency']} | {r['passed']} / {r['attempts']} | {p['p50']:.2f} / {p['p99']:.2f} | {v['p50']:.2f} / {v['p99']:.2f} | {r['max_pause_start_spread_ms']:.2f} |\n"
text+='''
The concurrency-1 cohort retains its nonzero exit and one resume failure (sample 24). Pause completed and paused state was verified, but resume missed the unchanged 15-second guest-agent readiness deadline; SDK-observed failure elapsed time was about 30.56 seconds. Diagnostics show a halted vCPU, no pending LAPIC interrupts and a nonzero TSC deadline. The cause is unproven. Event preservation fixes a demonstrated omission but does not eliminate this readiness failure.

All four cohorts verified unchanged source/image/runtime hashes, no remaining sandbox records, and worker teardown. Raw cohorts, failure diagnostics, exact coordinator/matrix/SDK source, hashes and an executable analyzer are retained beside this report. The separate initial C8 smoke passed 16/16; it is excluded from the sweep totals. The harness's 24 failure-accounting, barrier, state and cleanup tests passed on Windows and Linux.
'''
(out/'pause-bursts.md').write_text(text)
print(json.dumps({'attempts':summary['attempts'],'passed':summary['passed'],'rows':rows}))