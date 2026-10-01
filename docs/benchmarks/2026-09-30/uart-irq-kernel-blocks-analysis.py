import hashlib, importlib.util, json, statistics
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
report = json.loads(Path('/var/tmp/hm-competitive/uart-irq-kernel-blocks-50.json').read_text())
assert report['complete'] and report['success'] and report['artifacts_unchanged']
assert len(report['blocks']) == 100 and not report['error']
assert report['controlled_cpu_load']['all_alive_through_cohort']
assert report['controlled_cpu_load']['workers_cleaned_up']
source = root/'target/compare-uart-irq-kernels.py'
assert hashlib.sha256(source.read_bytes()).hexdigest() == report['artifact_sha256']['coordinator']
spec = importlib.util.spec_from_file_location('fc', root/'tools/bench-firecracker-local.py')
fc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fc)
groups = {}
means = {}
for block in range(50):
    entries = [entry for entry in report['blocks'] if entry['block'] == block]
    expected = ['original_kernel', 'fixed_irq_kernel'] if block % 2 == 0 else ['fixed_irq_kernel', 'original_kernel']
    assert [entry['kernel'] for entry in entries] == expected
    for entry in entries:
        cohort = entry['report']
        assert entry['process_exit_code'] == 0 and cohort['success'] and cohort['artifacts_unchanged']
        assert cohort['pairs'] == 2 and len(cohort['samples']) == 4
        assert cohort['cpu_count'] == 1 and cohort['memory_mb'] == 1024
        assert cohort['driver_cpu_affinity'] == [report['controlled_cpu_load']['cpu']]
        assert cohort['guest_readiness_timeout_s'] == {'hypermachine': 15, 'firecracker': 15}
        assert not cohort['setup_error'] and not cohort['cleanup_error'] and not cohort['diagnostic_tracing']
        for child, parent in [('hyperMachine', 'daemon'), ('firecracker', 'firecracker'),
                ('kernel', entry['kernel']), ('initrd', 'initrd'), ('harness', 'harness'),
                ('firecracker_harness', 'firecracker_harness')]:
            assert cohort['artifact_sha256'][child] == report['artifact_sha256'][parent]
        for engine in ('hypermachine', 'firecracker'):
            rows = [row for row in cohort['samples'] if row['engine'] == engine]
            assert len(rows) == 2 and all(row['success'] and row['cleanup_success'] for row in rows)
            values = [row['ready_ms'] for row in rows]
            groups.setdefault((entry['kernel'], engine), []).extend(values)
            means[block, entry['kernel'], engine] = statistics.mean(values)
summary = {'success': True, 'groups': [], 'paired_blocks': [],
    'raw_sha256': hashlib.sha256(Path('/var/tmp/hm-competitive/uart-irq-kernel-blocks-50.json').read_bytes()).hexdigest(),
    'analysis_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
for (kernel, engine), values in groups.items():
    assert len(values) == 100
    summary['groups'].append({'kernel': kernel, 'engine': engine, **fc.summary(values)})
for engine in ('hypermachine', 'firecracker'):
    differences = [means[block, 'original_kernel', engine] - means[block, 'fixed_irq_kernel', engine] for block in range(50)]
    summary['paired_blocks'].append({'engine': engine, 'original_minus_fixed_mean_ms': statistics.mean(differences),
        'original_minus_fixed_median_ms': statistics.median(differences),
        'fixed_faster_blocks': sum(value > 0 for value in differences), 'blocks': 50})
Path('/var/tmp/hm-competitive/uart-irq-kernel-blocks-50-summary.json').write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
