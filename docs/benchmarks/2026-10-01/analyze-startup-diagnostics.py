import hashlib, json, re, statistics
from pathlib import Path

folder = Path('/var/tmp/hm-competitive')
files = [('local-engines-startup-stages-c100.json', 'stage_only'),
    ('local-engines-guest-boot-stages-c100.json', 'guest_probe_after_readiness')]
summary = {'diagnostic_only':True, 'cohorts':[], 'raw_sha256':{},
    'analysis_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}

def stats(values):
    values = sorted(values)
    return {'n':len(values),'mean':statistics.mean(values),'median':statistics.median(values),
        'min':values[0],'max':values[-1]}

for file, label in files:
    path = folder/file
    report = json.loads(path.read_text())
    assert report['success'] and report['diagnostic_only'] and report['artifacts_unchanged']
    assert report['concurrency'] == 100 and report['pairs'] == 2 and len(report['batches']) == 4
    assert report['driver_cpu_affinity'] == list(range(8))
    assert report['stage_ids_match_passed_requests'] and not report['cleanup_errors']
    assert report['remaining_sandbox_count'] == 0 and report['cohort_exit_code'] == 0
    assert report['controlled_cpu_load']['all_alive_through_cohort']
    assert report['controlled_cpu_load']['workers_cleaned_up']
    all_rows = [row for batch in report['batches'] for row in batch['samples']]
    assert len(all_rows) == 400 and all(row['success'] and row['cleanup_success'] for row in all_rows)
    rows = {row['sandbox_id']:row for row in all_rows if row['engine'] == 'hypermachine'}
    stages = report['startup_stages_ms']
    assert len(rows) == 200 and set(rows) == set(stages)
    fields = ['total_ms','build_ms','launch_ms','agent_ms','network_envd_ms']
    assert all(set(stage) == set(fields) and all(value >= 0 for value in stage.values()) for stage in stages.values())
    residual = [row['ready_ms']-stages[id]['total_ms'] for id,row in rows.items()]
    item = {'label':label,'passing_attempts':400,'matched_hypermachine_stages':200,
        'stages_ms':{key:stats([stage[key] for stage in stages.values()]) for key in fields},
        'native_ready_minus_bringup_ms':stats(residual)}
    observations = report.get('guest_observations', [])
    if observations:
        assert report['guest_boot_collected'] and len(observations) == 20
        assert {(o['pair'],o['index']) for o in observations} == {(pair,index) for pair in range(2) for index in range(0,100,10)}
        init, differences, checks, entropy_before_init = [], [], [], []
        for observation in observations:
            id = observation['sandbox_id']
            assert id in rows and rows[id]['pair'] == observation['pair'] and rows[id]['index'] == observation['index']
            response = observation['response']
            assert response['exit_code'] == 0 and not response.get('timed_out') and not response.get('truncated')
            text = response['stdout']
            def stamp(message):
                match = re.search(r'\[\s*([0-9.]+)\] '+re.escape(message), text)
                assert match, message
                return float(match.group(1))*1000
            handoff = stamp('Run /init as init process')
            init.append(handoff)
            differences.append(stages[id]['agent_ms']-handoff)
            checks.append(stamp('x86/mm: Checked W+X mappings')-stamp('Freeing unused kernel image'))
            entropy_before_init.append(stamp('random: crng init done') < handoff)
        item['sampled_guest_init_handoff_ms'] = stats(init)
        item['host_agent_wait_minus_guest_init_timestamp_ms'] = stats(differences)
        item['sampled_guest_free_init_to_wx_check_ms'] = stats(checks)
        item['sampled_guests_with_entropy_initialized_before_handoff'] = sum(entropy_before_init)
        item['guest_samples'] = len(observations)
        item['clock_limitation'] = 'Guest dmesg and host elapsed clocks do not independently isolate dispatch, userspace or handshake delay'
    summary['cohorts'].append(item)
    summary['raw_sha256'][file] = hashlib.sha256(path.read_bytes()).hexdigest()
(folder/'startup-diagnostics-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(summary,indent=2))
