#!/usr/bin/env python3
"""Verify frozen allocator experiments and reject corrupted analysis inputs."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path


def require(value, message):
    if not value: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    root = parser.parse_args().archive.resolve()
    manifest = json.loads((root / 'manifest.json').read_text())['sha256']
    for name, expected in manifest.items():
        path = (root / name).resolve()
        require(path.is_relative_to(root) and hashlib.sha256(path.read_bytes()).hexdigest() == expected, 'archive hash/path mismatch: ' + name)
    spec = importlib.util.spec_from_file_location('allocator', root / 'analyze-daemon-allocator.py')
    analyzer = importlib.util.module_from_spec(spec); spec.loader.exec_module(analyzer)
    totals = 0
    for cohort in ['smoke-corrected', 'c100-repeat']:
        report = json.loads((root / cohort / 'report.json').read_text())
        identities = report['artifact_sha256']
        require(identities['daemon'] == identities['baseline'] == identities['candidate'] == '2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f', 'accepted daemon identity mismatch')
        require(identities['allocator'] == '51952ffe97354b56197c9b765582023d435cfbe1930b0c03b778bdb5faa1fff5', 'allocator identity mismatch')
        for key, name in [('harness','bench-daemon-allocator.py'),('budget','bench-cold-start-limit.py'),
                ('comparison','bench-connection-wait.py'),('burst','bench-local-engines-concurrent.py'),
                ('shared','bench-local-engines.py'),('firecracker','bench-firecracker-local.py')]:
            require(identities[key] == manifest[name], 'coordinator provenance mismatch')
        recomputed = analyzer.analyze(report)
        require(recomputed == json.loads((root / cohort / 'analysis.json').read_text()), 'analysis differs from raw attempts')
        totals += sum(value['attempted'] for value in recomputed['variants'].values())
    require('HV2_KERNEL must name a bzImage' in (root / 'smoke/bindings.log').read_text(), 'initial preflight failure omitted')
    interrupted = json.loads((root / 'c100/report.json').read_text())
    require(len(interrupted['runs']) == 4 and interrupted['pairs'] == 4 and 'artifacts_unchanged' not in interrupted
            and interrupted['success'] is False, 'interrupted cohort hidden or promoted')
    partial_attempts = sum(len(row.get('batch', {}).get('samples', [])) for row in interrupted['runs'])
    require(partial_attempts == 400 and any(row['cleanup_errors'] for row in interrupted['runs']), 'partial attempts/cleanup loss omitted')
    totals += partial_attempts
    report = json.loads((root / 'smoke-corrected/report.json').read_text())
    def change(name, modify):
        value = copy.deepcopy(report); modify(value)
        try: analyzer.analyze(value)
        except (ValueError, KeyError): return
        raise ValueError('corruption accepted: ' + name)
    cases = [
        ('mapping', lambda r: r['runs'][0].update(allocator_mapped=True)),
        ('preload', lambda r: r['runs'][0]['daemon_environment'].update(LD_PRELOAD=r['allocator_path'])),
        ('configuration', lambda r: r['runs'][0]['daemon_environment'].update(MALLOC_CONF='narenas:1')),
        ('bindings', lambda r: r['allocation_symbols_bound'].update(malloc=False)),
        ('cleanup', lambda r: r['runs'][0]['cleanup_errors'].append('injected')),
        ('stopped', lambda r: r['runs'][0].update(daemon_exit_code=None)),
        ('budget', lambda r: r.update(candidate_limit=8)),
        ('attempts', lambda r: r['runs'][0]['batch']['samples'].pop()),
        ('latency', lambda r: r['runs'][0]['batch']['samples'][0].update(ready_ms=float('nan'))),
        ('memory', lambda r: r['runs'][0]['batch']['idle_process_memory_kib'].update(Pss_kib=-1)),
        ('idle', lambda r: r['runs'][0]['batch'].update(memory_idle_actual_seconds=0)),
        ('outcome', lambda r: r.update(success=False)),
    ]
    for name, modify in cases: change(name, modify)
    print(json.dumps(dict(success=True, hashed_files=len(manifest), attempted=totals,
                         malformed_reports_rejected=len(cases), allocator_change_adopted=False)))


if __name__ == '__main__': main()
