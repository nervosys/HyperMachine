import hashlib,json,math,shutil,statistics
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
source=Path('/var/tmp/hm-refusal-retry');out=root/'docs/benchmarks/2026-10-01'
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
entries=json.loads((source/'commands-executable-matrix.json').read_bytes());failed=json.loads((source/'failed-setup-matrix.json').read_bytes())
assert len(entries)==16 and len(failed)==16
reports={};hashes={}
for entry in entries+failed:
 path=source/(entry['name']+'.json');r=json.loads(path.read_bytes())
 assert r['artifacts_unchanged'] and not r['cleanup_errors']
 target=out/('refusal-'+path.name);shutil.copyfile(path,target);hashes[target.name]=digest(target)
 if entry in failed:
  assert entry['exit_code']==1 and r['attempts']==0 and 'Permission denied' in r['error']
  continue
 assert entry['exit_code']==0 and r['success'] and r['remaining_sandbox_count']==0
 assert all(p['success'] for p in r['preparation']) and len(r['preparation'])==r['concurrency']
 assert r['passed']==r['attempts']==r['rounds']*r['concurrency'] and all(v['success'] for v in r['rows'])
 assert r['artifact_sha256']['coordinator']==digest(out/'refusal-command-coordinator.py')
 profile=r['profile'];c=r['concurrency'];block=int(entry['name'][-1]);reports[(profile,c,block)]=r
 for batch in r['command_batches']:
  starts=[row['started_at_seconds'] for row in r['rows'] if row['round']==batch['round']]
  assert len(starts)==c and math.isclose(batch['start_spread_ms'],(max(starts)-min(starts))*1000,abs_tol=1e-9)
rows=[]
for c in [1,8,50,100]:
 for block in [0,1]:
  pair={p:reports[(p,c,block)] for p in ['baseline','candidate']}
  values={p:sorted(v['latency_ms'] for v in pair[p]['rows']) for p in pair}
  row={'concurrency':c,'block':block,'attempts_per_profile':len(values['baseline']),
       'mean_ms':{p:statistics.mean(v) for p,v in values.items()},
       'p99_ms':{p:v[math.ceil(.99*len(v))-1] for p,v in values.items()}}
  row['candidate_minus_baseline_mean_ms']=row['mean_ms']['candidate']-row['mean_ms']['baseline'];rows.append(row)
summary={'attempts':sum(r['attempts'] for r in reports.values()),'passed':sum(r['passed'] for r in reports.values()),'preparations':sum(len(r['preparation']) for r in reports.values()),'setup_failure_cohorts':len(failed),'rows':rows,'raw_sha256':hashes,'adopted':False}
assert summary['attempts']==15600 and summary['passed']==15600 and summary['preparations']==636
for profile in ['baseline','candidate']:
 meta=json.loads((source/f'{profile}-build-metadata.json').read_bytes())
 assert meta['build_exit_code']==0 and meta['canonical_scored_binary_restored']
 assert digest(source/f'hv2-sandboxd-{profile}')==meta[f'{profile}_binary_sha256']
 if profile=='candidate':
  assert digest(out/'refusal-retry-candidate.rs')==meta['source_sha256']['crates/hv2-agent/src/guest_agent.rs']
  raw=(source/'candidate-build-output.txt').read_bytes();meta['build_output']=raw.decode();meta['build_output_sha256']=hashlib.sha256(raw).hexdigest()
  (out/'refusal-retry-candidate-build.json').write_text(json.dumps(meta,indent=2)+'\n')
 for key,value in meta['source_sha256'].items():
  if profile=='baseline':assert digest(root/key)==value
for original,target in [('commands-executable-matrix.json','refusal-command-results-matrix.json'),('failed-setup-matrix.json','refusal-command-failed-setup-matrix.json'),('failed-setup-matrix-source.py','refusal-command-failed-setup-matrix.py')]:shutil.copyfile(source/original,out/target)
(out/'refusal-retry-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
text='''# Refused-agent connection backoff candidate (2026-10-01)

The candidate retries initial refusals after 1, 2 and 4 ms, then returns to the existing 5 ms cadence; each delay is clipped to the remaining deadline. Normal progress polling stays unchanged. Its hypothesis was that reconnecting immediately after a completed command could benefit from a shorter initial refusal delay. This is not a change to the agent protocol or a shared connection pool.

**Rejected and reverted.** All 15,600 measured commands and 636 guest preparations passed, but candidate mean latency was higher in seven of eight pairs. Both pairs were slower at concurrency 1, 50 and 100; concurrency 8 was mixed. No consistent improvement, comparative win or reliability fix is established. Production retains the prior fixed 5 ms refusal sleep.

Each cohort starts a fresh owned node with a snapshot-backed base template, identical kernel/initrd, one vCPU and 1024 MiB per guest, eight pinned host CPUs and no extra busy worker. Every guest passes command and resource preparation. Commands use the same HTTP exec API and fresh guest-agent connections in both builds. Rounds synchronize clients and retain observed start spreads. Each concurrency has two pairs, baseline/candidate then candidate/baseline, with 100 rounds at C1/C8 and 20 at C50/C100. Repeated commands within a guest and rounds within a cohort are correlated; the sample count is not an independent reliability estimate. All nodes verified no remaining sandbox records and unchanged artifacts.

| Concurrency | Pair | Commands per profile | Mean baseline / candidate (ms) | Candidate mean change (ms) | P99 baseline / candidate (ms) |
|---|---|---|---|---|---|
'''
for row in rows:
 m=row['mean_ms'];p=row['p99_ms']
 text+=f"| {row['concurrency']} | {row['block']} | {row['attempts_per_profile']} | {m['baseline']:.2f} / {m['candidate']:.2f} | {row['candidate_minus_baseline_mean_ms']:+.2f} | {p['baseline']:.2f} / {p['candidate']:.2f} |\n"
text+='''
The first 16 cohort launches failed before any guest attempt because the build archive copy lacked executable permissions. Their nonzero setup reports and original matrix are retained separately, with zero command attempts. After applying executable bits to the two owned binaries, the corrected sweep used distinct filenames; no failed report was overwritten or counted as a passing attempt. Executable bytes were unchanged by the permission fix.

The current-source baseline and candidate used the same release flags, Cargo lock and compiler. Build metadata and output hashes, exact candidate source/tests, coordinators, both matrices and all raw cohorts are retained. The canonical original scored binary was restored after builds and the main-source timestamp invalidated to force later Cargo relinking. Candidate protocol/deadline checks passed on Windows/Linux (15 tests), and strict Linux Clippy passed before reversion. Restored-production checks are recorded separately. Shared nested hardware, two pairs and no direct refusal counters limit causal attribution; this rejects the default change for lack of a consistent measured benefit.
'''
(out/'refusal-retry.md').write_text(text)
print(json.dumps({'attempts':summary['attempts'],'passed':summary['passed'],'slower_pairs':sum(row['candidate_minus_baseline_mean_ms']>0 for row in rows)}))