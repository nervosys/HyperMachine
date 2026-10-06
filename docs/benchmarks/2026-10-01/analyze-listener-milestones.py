import hashlib, json, re, statistics, sys
from pathlib import Path

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
rows = [row for batch in report['batches'] for row in batch['samples']]
assert len(rows) == 400 and all(row['success'] and row['cleanup_success'] for row in rows)
ids = {row['sandbox_id'] for row in rows if row['engine'] == 'hypermachine'}
assert len(ids) == 200 and ids == set(report['cold_readiness_stages_ms']) == set(report['startup_stages_ms'])
observations = report['guest_observations']
assert report['guest_boot_collected'] and len(observations) == 20
assert {(row['pair'],row['index']) for row in observations} == {(pair,index) for pair in range(2) for index in range(0,100,10)}
samples = []
for observation in observations:
    assert observation['sandbox_id'] in ids
    response = observation['response']
    assert response['exit_code'] == 0 and not response.get('timed_out') and not response.get('truncated')
    log = response['stdout']
    def timestamp(pattern):
        matches = re.findall(r'\[\s*([0-9.]+)\]\s*' + pattern, log)
        assert len(matches) == 1, (pattern, matches)
        return float(matches[0])*1000
    handoff = timestamp(r'Run /init as init process')
    mounts = timestamp(r'HV2_BOOT core_mounts_ready')
    launch = timestamp(r'HV2_BOOT agent_launch')
    listening = timestamp(r'agent_listening')
    accepted = timestamp(r'agent_first_accept')
    assert handoff <= mounts <= launch <= listening <= accepted
    samples.append(dict(sandbox_id=observation['sandbox_id'],handoff_ms=handoff,
        mounts_ms=mounts,agent_launch_ms=launch,init_to_mounts_ms=mounts-handoff,
        mounts_to_launch_ms=launch-mounts,init_to_agent_launch_ms=launch-handoff,launch_to_listen_ms=listening-launch,listen_to_accept_ms=accepted-listening))
def stats(values):
    return dict(n=len(values),mean=statistics.mean(values),median=statistics.median(values),min=min(values),max=max(values))
summary = dict(diagnostic_only=True,passing_attempts=400,matched_ids=200,guest_samples=20,
    raw_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),samples=samples,
    guest_ms={field:stats([row[field] for row in samples]) for field in samples[0] if field != 'sandbox_id'},
    cold_ms={field:stats([row[field] for row in report['cold_readiness_stages_ms'].values()]) for field in ['blocking_queue_ms','connect_ms','ping_ms']},
    limitation='Guest milestone deltas share a guest clock; host connection durations have a different time origin. Listener-to-first-accept is guest elapsed time and includes host scheduling/connection delivery; it does not isolate transport overhead. Post-readiness probes affect memory/cleanup. Shared nested host and two batch pairs limit conclusions.')
path.with_name('listener-milestones-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps({key:value for key,value in summary.items() if key != 'samples'},indent=2))
