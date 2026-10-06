import hashlib, json, re, statistics, sys
from pathlib import Path

path = Path(sys.argv[1])
r = json.loads(path.read_text())
assert r['success'] and r['diagnostic_only'] and r['artifacts_unchanged']
assert r['concurrency'] == 100 and r['pairs'] == 2
assert r['cohort_exit_code'] == 0 and r['remaining_sandbox_count'] == 0 and not r['cleanup_errors']
assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
rows = [row for batch in r['batches'] for row in batch['samples']]
assert len(rows) == 400 and all(row['success'] and row['cleanup_success'] for row in rows)
hm = {row['sandbox_id']:row for row in rows if row['engine'] == 'hypermachine'}
assert len(hm) == 200 and set(hm) == set(r['cold_readiness_stages_ms']) == set(r['startup_stages_ms'])
assert r['stage_ids_match_passed_requests'] and r['cold_ids_match_passed_requests']
assert r['dispatch_ids_match_passed_requests'] and set(hm) == set(r['dispatch_stages_ms'])
assert all(set(row) == {'dispatch_queue_ms','wrapper_queue_ms','thread_start_ms','owner_setup_ms','first_backend_ms'} for row in r['dispatch_stages_ms'].values())
observations = r['guest_observations']
assert len(observations) == 20 and {(o['pair'],o['index']) for o in observations} == {(p,i) for p in range(2) for i in range(0,100,10)}
samples = []
def uptime(probe):
    response = probe['response']
    assert response['exit_code'] == 0 and not response.get('timed_out') and not response.get('truncated')
    matches = re.findall(r'UPTIME\n([0-9.]+)\s',response['stdout'])
    assert len(matches) == 1
    return float(matches[0])
for observation in observations:
    repeat = observation['repeat_clock_probe']
    first_uptime, second_uptime = uptime(observation), uptime(repeat)
    begin,end = observation['host_start_perf_seconds'],observation['host_end_perf_seconds']
    later_begin,later_end = repeat['host_start_perf_seconds'],repeat['host_end_perf_seconds']
    assert begin <= end <= later_begin <= later_end
    guest_delta = second_uptime-first_uptime
    low,high = later_begin-end,later_end-begin
    # /proc/uptime prints centiseconds; allow two quantization bins.
    compatible = low-.02 <= guest_delta <= high+.02
    midpoint = (low+high)/2
    row = hm[observation['sandbox_id']]
    native_start = row['batch_start_perf_seconds']+row['start_offset_ms']/1000
    # Conditional on stable clocks, epoch lies in this probe-bound interval.
    epoch_low,epoch_high = begin-first_uptime-.01,end-first_uptime+.01
    samples.append(dict(sandbox_id=observation['sandbox_id'],guest_delta_seconds=guest_delta,
        host_delta_low_seconds=low,host_delta_high_seconds=high,
        elapsed_clocks_compatible=compatible,guest_host_midpoint_ratio=guest_delta/midpoint,
        conditional_guest_epoch_after_native_start_low_ms=(epoch_low-native_start)*1000,
        conditional_guest_epoch_after_native_start_high_ms=(epoch_high-native_start)*1000))
def stats(values):
    return dict(n=len(values),mean=statistics.mean(values),median=statistics.median(values),min=min(values),max=max(values))
summary = dict(diagnostic_only=True,passing_attempts=400,matched_ids=200,guest_samples=20,
    dispatch_ms={field:stats([row[field] for row in r['dispatch_stages_ms'].values()]) for field in ['dispatch_queue_ms','wrapper_queue_ms','thread_start_ms','owner_setup_ms','first_backend_ms']},
    elapsed_clocks_compatible_count=sum(sample['elapsed_clocks_compatible'] for sample in samples),samples=samples,
    measurements={field:stats([sample[field] for sample in samples]) for field in ['guest_host_midpoint_ratio','conditional_guest_epoch_after_native_start_low_ms','conditional_guest_epoch_after_native_start_high_ms']},
    raw_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    limitation='Two uptime observations test only their interval, not earlier boot-time clock stability. Guest clock epoch is not VM entry. Conditional epoch bounds cannot establish dispatch delay or transport root cause. Shared host, sampling and post-readiness commands remain diagnostic limitations.')
path.with_name('first-backend-diagnostic-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps({key:value for key,value in summary.items() if key != 'samples'},indent=2))
