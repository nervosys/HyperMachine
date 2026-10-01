import hashlib, json, statistics, sys
from pathlib import Path
root = Path('/var/tmp/hm-competitive')
result = {'measurement_kind': 'fixed-idle-process-pss', 'profiles': [], 'limitations': ['Three alternating batch pairs per concurrency; within-batch guests share load and are not independent samples', 'Shared nested WSL/KVM host, eight-CPU affinity and one pinned CPU worker', 'HM process increment subtracts its current empty daemon; FC sums fresh VMM processes against zero processes, excluding benchmark controller footprint', 'PSS excludes host kernel memory and is not a density or fleet-capacity result']}
for concurrency in [1, 8, 50, 100]:
    path = root / ('idle-memory-c' + str(concurrency) + '.json')
    report = json.loads(path.read_bytes())
    assert report['concurrency'] == concurrency and report['pairs'] == 3
    assert report['memory_idle_seconds'] == 5
    assert report['artifacts_unchanged'] and not report['cleanup_errors']
    assert report['remaining_sandbox_count'] == 0
    assert report['controlled_cpu_load']['all_alive_through_cohort'] and report['controlled_cpu_load']['workers_cleaned_up']
    profile = {'concurrency': concurrency, 'raw_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'cohort_success': report['success'], 'engines': {}, 'paired_hm_minus_fc_incremental_pss_mib': [], 'paired_hm_minus_fc_held_pss_mib': []}
    by_pair, by_pair_held = {}, {}
    for engine in ['hypermachine', 'firecracker']:
        batches = [batch for batch in report['batches'] if batch['engine'] == engine]
        assert len(batches) == 3 and {batch['pair'] for batch in batches} == {0, 1, 2}
        samples = [row for batch in batches for row in batch['samples']]
        valid = []
        for batch in batches:
            if not batch['success'] or not batch['all_guests_validated_while_held']:
                continue
            assert batch['held_process_count'] == (1 if engine == 'hypermachine' else concurrency)
            assert batch['memory_idle_actual_seconds'] >= 5
            assert batch['guest_idle_at_measurement_start_ms']['min'] >= 5000
            fields = ['idle_process_memory_kib', 'empty_process_memory_baseline_kib', 'incremental_idle_process_memory_kib']
            held, baseline, delta = [batch[field]['Pss_kib'] for field in fields]
            assert delta == held - baseline
            if engine == 'firecracker': assert baseline == 0
            valid.append(batch)
            by_pair[(engine, batch['pair'])] = delta / 1024
            by_pair_held[(engine, batch['pair'])] = held / 1024
        entry = {'attempts': len(samples), 'passed': sum(row['success'] and row['cleanup_success'] for row in samples), 'valid_memory_batches': len(valid), 'failed_rows': [row for row in samples if not row['success'] or not row['cleanup_success']]}
        if valid:
            deltas = [batch['incremental_idle_process_memory_kib']['Pss_kib']/1024 for batch in valid]
            entry.update(incremental_idle_pss_mib_median=statistics.median(deltas), incremental_idle_pss_mib_min=min(deltas), incremental_idle_pss_mib_max=max(deltas), amortized_incremental_pss_mib_per_guest_median=statistics.median(deltas)/concurrency,
                empty_pss_mib=[batch['empty_process_memory_baseline_kib']['Pss_kib']/1024 for batch in valid],
                held_idle_pss_mib=[batch['idle_process_memory_kib']['Pss_kib']/1024 for batch in valid],
                post_cleanup_empty_pss_mib=[batch['empty_node_memory_after_cleanup_kib']['Pss_kib']/1024 for batch in valid] if engine == 'hypermachine' else None)
        profile['engines'][engine] = entry
    for pair in range(3):
        if all((engine, pair) in by_pair for engine in ['hypermachine', 'firecracker']):
            profile['paired_hm_minus_fc_incremental_pss_mib'].append(by_pair[('hypermachine', pair)] - by_pair[('firecracker', pair)])
            profile['paired_hm_minus_fc_held_pss_mib'].append(by_pair_held[('hypermachine', pair)] - by_pair_held[('firecracker', pair)])
    result['profiles'].append(profile)
output = Path('/var/tmp/hm-competitive/idle-memory-summary.json')
output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))