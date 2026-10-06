import hashlib, json, re, sys
from pathlib import Path

path = Path(sys.argv[1])
r = json.loads(path.read_text())
assert r['diagnostic_only'] and r['artifacts_unchanged'] and not r['setup_error']
assert r['concurrency'] == 100 and r['pairs'] == 10
assert r['driver_cpu_affinity'] == list(range(8))
assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
assert r['remaining_sandbox_count'] == 0 and not r['cleanup_errors']
rows = [row for batch in r['batches'] for row in batch['samples']]
assert len(rows) == 2000
assert r['success'] == all(row['success'] and row['cleanup_success'] for row in rows)
assert r['cohort_exit_code'] == (0 if r['success'] else 1)
assert all(row['cleanup_success'] for row in rows if row['success'])
assert r['artifact_sha256']['hypermachine'] == '0341fe7e22479a1a276fec9fe0ee16750ca48eb664528c5a79e3f63a0b86e96b'
assert r['artifact_sha256']['kernel'] == 'afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd'
assert r['artifact_sha256']['initrd'] == '1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'
failures = []
for row in rows:
    if row['success']: continue
    item = dict(row)
    match = re.search(r'\bTSC=(0x[0-9a-f]+|unavailable) TSC_DEADLINE=(0x[0-9a-f]+|unavailable)',row.get('error',''))
    item['timer_fields_present'] = match is not None
    if match:
        item['captured_tsc'] = None if match[1] == 'unavailable' else int(match[1],16)
        item['captured_deadline'] = None if match[2] == 'unavailable' else int(match[2],16)
    failures.append(item)
counts = {engine:dict(attempts=sum(row['engine']==engine for row in rows),
    passed=sum(row['engine']==engine and row['success'] for row in rows)) for engine in ['hypermachine','firecracker']}
assert all(value['attempts'] == 1000 for value in counts.values())
summary = dict(diagnostic_only=True,counts=counts,failures=failures,
    failures_with_timer_fields=sum(row['timer_fields_present'] for row in failures),
    raw_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    limitation='Failure-only formatter change; no runtime timer fix. MSRs are sequential owner-safe reads, not atomic. A cohort without failures yields no failed-boot timer state and cannot establish reliability resolution. Shared nested host; diagnostic evidence excluded from scored comparisons.')
path.with_name('timer-diagnostic-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps({key:value for key,value in summary.items() if key != 'failures'},indent=2))
