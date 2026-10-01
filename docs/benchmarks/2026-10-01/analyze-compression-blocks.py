import hashlib, json, statistics, sys
from pathlib import Path

folder = Path(sys.argv[1])
manifest_path = folder/'kernel-compression-blocks.json'
manifest = json.loads(manifest_path.read_text())
assert manifest['complete'] and len(manifest['cohorts']) == 12
expected_kernels = dict(gzip='afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd',
    lz4='d0b6b5801885478da68eb75247ab0ef76c43a2bb3c93fdc04311aef0eddbef72')
cohorts = {}
samples = {(profile,engine):[] for profile in expected_kernels for engine in ['hypermachine','firecracker']}
attempts = {key:0 for key in samples}
failures = []
expected_order = [(block,profile) for block in range(6) for profile in (['gzip','lz4'] if block % 2 == 0 else ['lz4','gzip'])]
assert [(row['block'],row['profile']) for row in manifest['cohorts']] == expected_order
for item in manifest['cohorts']:
    raw = (folder/item['file']).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == item['raw_sha256']
    r = json.loads(raw)
    assert r['artifacts_unchanged'] and not r['setup_error']
    assert item['exit_code'] == r['cohort_exit_code'] == (0 if r['success'] else 1)
    assert r['coordinator_sha256'] == manifest['coordinator_sha256']
    assert r['concurrency'] == 100 and r['pairs'] == 2
    assert r['driver_cpu_affinity'] == list(range(8)) and r['cpu_count'] == 1 and r['memory_mb'] == 1024
    assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
    assert r['remaining_sandbox_count'] == 0 and not r['cleanup_errors']
    assert r['artifact_sha256']['kernel'] == expected_kernels[item['profile']]
    assert r['artifact_sha256']['hypermachine'] == '8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
    assert r['artifact_sha256']['initrd'] == '1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'
    for key,value in dict(firecracker='99ad0f5cd0514a88aad0e9ae8cfdb3cc3b4ab9d190e1194602406c786b5de7a5',
        harness='efbddbf1380dbcbdc06398fddd446bcd59f9f4f582384535377514d12ea307bf',
        shared_harness='6cc4b9a964e7d5cf623d0bc308d5f369cc2751f3c7085d71b04f13cf1f69d433',
        firecracker_harness='38493ed3b069d31bc87552c3c262bb541607cfc2e4bbd408f64950b76b78fea6').items():
        assert r['artifact_sha256'][key] == value
    assert r['guest_readiness_timeout_s'] == dict(hypermachine=15,firecracker=15)
    assert r['firecracker_total_startup_timeout_s'] == 30
    assert r['lifecycle'] == 'native-cold-create-to-command-bursts'
    assert r['hypermachine_template_preflight']['snapshot'] is False
    assert r['common_boot_args'] == 'console=ttyS0,115200 nokaslr rdinit=/init quiet loglevel=3 8250.nr_uarts=1 i8042.noaux i8042.nomux i8042.nopnp i8042.dumbkbd'
    assert [(batch['pair'],batch['engine']) for batch in r['batches']] == [(0,'hypermachine'),(0,'firecracker'),(1,'firecracker'),(1,'hypermachine')]
    rows = [row for batch in r['batches'] for row in batch['samples']]
    assert len(rows) == 400
    assert all(row['cleanup_success'] and 0 <= row['ready_ms'] <= 30000 for row in rows if row['success'])
    assert r['success'] == all(row['success'] and row['cleanup_success'] for row in rows)
    cohorts[item['block'],item['profile']] = {engine:[row['ready_ms'] for row in rows if row['engine'] == engine and row['success']] for engine in ['hypermachine','firecracker']}
    for row in rows:
        attempts[item['profile'],row['engine']] += 1
        if not row['success']:
            failures.append(dict(block=item['block'],profile=item['profile'],**row))
    for engine,values in cohorts[item['block'],item['profile']].items():
        assert len([row for row in rows if row['engine'] == engine]) == 200
        samples[item['profile'],engine].extend(values)
def stats(values):
    values = sorted(values)
    return dict(n=len(values),mean=statistics.mean(values),p50=values[(len(values)-1)//2],p99=values[int((len(values)-1)*.99)])
deltas = {engine:[dict(block=block,delta_ms=statistics.mean(cohorts[block,'lz4'][engine])-statistics.mean(cohorts[block,'gzip'][engine])) for block in range(6) if len(cohorts[block,'lz4'][engine]) == len(cohorts[block,'gzip'][engine]) == 200] for engine in ['hypermachine','firecracker']}
summary = dict(total_attempts=4800,passing_attempts=4800-len(failures),failures=failures,blocks=6,
    success_counts={profile:{engine:dict(attempts=attempts[profile,engine],passed=len(samples[profile,engine]),failed=attempts[profile,engine]-len(samples[profile,engine])) for engine in ['hypermachine','firecracker']} for profile in expected_kernels},
    ready_ms={profile:{engine:stats(samples[profile,engine]) for engine in ['hypermachine','firecracker']} for profile in expected_kernels},
    paired_block_mean_delta_lz4_minus_gzip_ms={engine:dict(complete_blocks_only=True,values=values,median=statistics.median(row['delta_ms'] for row in values),lz4_faster_blocks=sum(row['delta_ms']<0 for row in values)) for engine,values in deltas.items()},
    manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    limitation='Latency quantiles include successful attempts only. Paired latency deltas exclude incomplete-success engine blocks, whose failures remain reported. Six paired kernel blocks on shared nested hardware; within-batch guests are correlated. Native cold readiness includes image loading and guest command validation; it does not establish fleet, managed-platform or universal performance.')
(folder/'kernel-compression-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(summary,indent=2))
