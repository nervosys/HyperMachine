"""Verify matched cold readiness diagnostics; run with the raw JSON path."""
import hashlib
import json
from pathlib import Path
import statistics
import sys

path = Path(sys.argv[1])
report = json.loads(path.read_text())
assert report['success'] and report['diagnostic_only'] and report['artifacts_unchanged']
assert report['concurrency'] == 100 and report['pairs'] == 2
assert report['driver_cpu_affinity'] == list(range(8))
assert report['stage_ids_match_passed_requests'] and report['cold_ids_match_passed_requests']
assert report['remaining_sandbox_count'] == 0 and not report['cleanup_errors']
assert report['cohort_exit_code'] == 0
assert report['controlled_cpu_load']['all_alive_through_cohort']
assert report['controlled_cpu_load']['workers_cleaned_up']
assert not report['guest_boot_collected']
rows = [row for batch in report['batches'] for row in batch['samples']]
assert len(rows) == 400 and all(row['success'] and row['cleanup_success'] for row in rows)
ids = {row['sandbox_id'] for row in rows if row['engine'] == 'hypermachine'}
stages = report['cold_readiness_stages_ms']
assert len(ids) == 200 and ids == set(stages) == set(report['startup_stages_ms'])
assert all(row['succeeded'] and row['phase'] == 'ping' for row in stages.values())
fields = ['blocking_queue_ms', 'connect_ms', 'ping_ms']
assert all(all(row[field] >= 0 for field in fields) for row in stages.values())

def stats(values):
    values = sorted(values)
    return dict(n=len(values), mean=statistics.mean(values), median=statistics.median(values),
        min=values[0], max=values[-1])

summary = dict(diagnostic_only=True, passing_attempts=len(rows), matched_ids=len(ids),
    raw_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    stages_ms={field:stats([row[field] for row in stages.values()]) for field in fields},
    limitation='Connection timing includes waiting for guest boot, driver and listener readiness; it does not isolate transport overhead. Two correlated batches per engine on shared nested hardware do not establish a performance win.')
output = path.with_name('cold-readiness-summary.json')
output.write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
