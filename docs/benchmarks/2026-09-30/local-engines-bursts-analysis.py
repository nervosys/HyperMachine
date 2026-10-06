import hashlib, importlib.util, json, statistics
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
spec = importlib.util.spec_from_file_location('engines', root/'tools/bench-local-engines.py')
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
profiles = [(1,100), (8,25), (50,10), (100,10)]
summary = {'groups':[], 'provenance_verified':True,
    'analysis_sha256':engines.digest(Path(__file__)), 'raw_sha256':{}}
identities = None
for concurrency, pairs in profiles:
    path = Path(f'/var/tmp/hm-competitive/local-engines-bursts-c{concurrency}-{pairs}.json')
    report = json.loads(path.read_text())
    summary['raw_sha256'][path.name] = engines.digest(path)
    assert report['concurrency'] == concurrency and report['pairs'] == pairs
    assert report['artifacts_unchanged'] and not report['setup_error'] and not report['cleanup_errors']
    assert report['remaining_sandbox_count'] == 0 and len(report['batches']) == 2*pairs
    assert report['cpu_count'] == 1 and report['memory_mb'] == 1024
    assert report['driver_cpu_affinity'] == list(range(8))
    assert report['guest_readiness_timeout_s'] == {'hypermachine':15, 'firecracker':15}
    assert report['firecracker_total_startup_timeout_s'] == 30
    assert report['controlled_cpu_load']['workers'] == 1 and report['controlled_cpu_load']['cpu'] == 0
    assert report['controlled_cpu_load']['all_alive_through_cohort'] and report['controlled_cpu_load']['workers_cleaned_up']
    assert engines.digest(root/'target/run-concurrent-engines.py') == report['coordinator_sha256']
    if identities is None: identities = report['artifact_sha256']
    else: assert report['artifact_sha256'] == identities
    for pair in range(pairs):
        batches = report['batches'][2*pair:2*pair+2]
        order = ['hypermachine','firecracker'] if pair % 2 == 0 else ['firecracker','hypermachine']
        assert [batch['engine'] for batch in batches] == order
        for batch in batches:
            assert batch['pair'] == pair and batch['concurrency'] == concurrency and not batch['error']
            assert len(batch['samples']) == concurrency
            assert [row['index'] for row in batch['samples']] == list(range(concurrency))
            assert all(row['engine'] == batch['engine'] and row['pair'] == pair for row in batch['samples'])
            assert all(row['validated_offset_ms'] >= 0 for row in batch['samples'])
            assert batch['readiness_wall_ms'] == max(row['validated_offset_ms'] for row in batch['samples'])
            assert batch['total_wall_ms_including_cleanup'] >= batch['readiness_wall_ms']
            for row in batch['samples']:
                if row['success']:
                    assert row['ready_ms'] > 0 and row['start_offset_ms'] >= 0
                    assert row['validated_offset_ms'] + .001 >= row['start_offset_ms'] + row['ready_ms']
    for engine in ['hypermachine','firecracker']:
        batches = [batch for batch in report['batches'] if batch['engine'] == engine]
        rows = [row for batch in batches for row in batch['samples']]
        passed = [row for row in rows if row['success'] and row['cleanup_success']]
        assert len(rows) == concurrency*pairs
        ready = engines.fc.summary([row['ready_ms'] for row in passed])
        assert ready == report['ready_ms'][engine]
        rates = [sum(row['success'] and row['cleanup_success'] for row in batch['samples'])/(batch['readiness_wall_ms']/1000)
            for batch in batches]
        memory = []
        for batch in batches:
            if batch['all_guests_validated_while_held'] and all(row['cleanup_success'] for row in batch['samples']):
                assert batch['held_process_count'] == (1 if engine == 'hypermachine' else concurrency)
                memory.append(batch['held_process_memory_kib']['Pss_kib']/1024)
        summary['groups'].append({'concurrency':concurrency, 'engine':engine, 'batches':pairs,
            'attempts':len(rows), 'passed':len(passed), 'failed':len(rows)-len(passed),
            'failure_rate':(len(rows)-len(passed))/len(rows), 'readiness_ms':ready,
            'median_ready_attempts_per_second':statistics.median(rates),
            'median_launch_spread_ms':statistics.median(batch['launch_spread_ms'] for batch in batches),
            'median_held_process_pss_mib':statistics.median(memory) if memory else None,
            'memory_measurement_batches':len(memory),
            'node_initial_pss_mib':report['node_memory_baseline_kib']['Pss_kib']/1024 if engine == 'hypermachine' else None})
summary['artifact_sha256'] = identities
for name, path in [('harness',root/'tools/bench-local-engines-concurrent.py'),
        ('shared_harness',root/'tools/bench-local-engines.py'), ('firecracker_harness',root/'tools/bench-firecracker-local.py')]:
    assert engines.digest(path) == identities[name]
Path('/var/tmp/hm-competitive/local-engines-bursts-matrix-summary.json').write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
