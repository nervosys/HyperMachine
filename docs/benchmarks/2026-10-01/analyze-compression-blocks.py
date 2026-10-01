import hashlib, json, statistics, sys
from pathlib import Path

folder = Path(sys.argv[1])
manifest_path = folder/'kernel-compression-blocks.json'
manifest = json.loads(manifest_path.read_text())
assert manifest['complete'] and manifest['success'] and len(manifest['cohorts']) == 12
expected_kernels = dict(gzip='afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd',
    lz4='d0b6b5801885478da68eb75247ab0ef76c43a2bb3c93fdc04311aef0eddbef72')
cohorts = {}
samples = {(profile,engine):[] for profile in expected_kernels for engine in ['hypermachine','firecracker']}
expected_order = [(block,profile) for block in range(6) for profile in (['gzip','lz4'] if block % 2 == 0 else ['lz4','gzip'])]
assert [(row['block'],row['profile']) for row in manifest['cohorts']] == expected_order
for item in manifest['cohorts']:
    raw = (folder/item['file']).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == item['raw_sha256']
    r = json.loads(raw)
    assert item['exit_code'] == 0 and r['success'] and r['artifacts_unchanged']
    assert r['coordinator_sha256'] == manifest['coordinator_sha256']
    assert r['concurrency'] == 100 and r['pairs'] == 2
    assert r['driver_cpu_affinity'] == list(range(8)) and r['cpu_count'] == 1 and r['memory_mb'] == 1024
    assert r['controlled_cpu_load']['all_alive_through_cohort'] and r['controlled_cpu_load']['workers_cleaned_up']
    assert r['cohort_exit_code'] == 0 and r['remaining_sandbox_count'] == 0 and not r['cleanup_errors']
    assert r['artifact_sha256']['kernel'] == expected_kernels[item['profile']]
    assert r['artifact_sha256']['hypermachine'] == '8ea52a369067d99f281b96bc513c59216304ac8e7eea0462d746f854e9d7aa5d'
    assert r['artifact_sha256']['initrd'] == '1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'
    assert [(batch['pair'],batch['engine']) for batch in r['batches']] == [(0,'hypermachine'),(0,'firecracker'),(1,'firecracker'),(1,'hypermachine')]
    rows = [row for batch in r['batches'] for row in batch['samples']]
    assert len(rows) == 400 and all(row['success'] and row['cleanup_success'] and 0 <= row['ready_ms'] <= 30000 for row in rows)
    cohorts[item['block'],item['profile']] = {engine:[row['ready_ms'] for row in rows if row['engine'] == engine] for engine in ['hypermachine','firecracker']}
    for engine,values in cohorts[item['block'],item['profile']].items():
        assert len(values) == 200
        samples[item['profile'],engine].extend(values)
def stats(values):
    values = sorted(values)
    return dict(n=len(values),mean=statistics.mean(values),p50=values[(len(values)-1)//2],p99=values[int((len(values)-1)*.99)])
deltas = {engine:[statistics.mean(cohorts[block,'lz4'][engine])-statistics.mean(cohorts[block,'gzip'][engine]) for block in range(6)] for engine in ['hypermachine','firecracker']}
summary = dict(passing_attempts=4800,blocks=6,
    ready_ms={profile:{engine:stats(samples[profile,engine]) for engine in ['hypermachine','firecracker']} for profile in expected_kernels},
    paired_block_mean_delta_lz4_minus_gzip_ms={engine:dict(values=values,median=statistics.median(values),lz4_faster_blocks=sum(value<0 for value in values)) for engine,values in deltas.items()},
    manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
    analysis_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    limitation='Six paired kernel blocks on shared nested hardware; guests within batches are correlated. Native cold readiness includes image loading and guest command validation; it does not establish fleet, managed-platform or universal performance.')
(folder/'kernel-compression-summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(summary,indent=2))
