#!/usr/bin/env python3
"""Replay a real sparse named-snapshot report and reject damaged comparison contracts."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    directory = Path(__file__).parent
    spec = importlib.util.spec_from_file_location('borrowed_analysis_check', directory / 'analyze-sparse-named-snapshots.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = json.loads(args.report.read_text())
    checked = module.analyze(report, directory)
    if not checked['cohort_success'] or not checked['cleanup_verified']:
        raise ValueError('requires complete real comparison for negative checks')
    mutations = {
        'published-runtime-claim': lambda r: r.update(runtime_change_adopted=True),
        'changed-artifact': lambda r: r.update(artifacts_unchanged=False),
        'missing-outer-run': lambda r: r['runs'].pop(),
        'wrong-order': lambda r: r['runs'][0].update(variant='candidate'),
        'missing-source-binding': lambda r: r['candidate_source_sha256'].pop('crates/hv2-sandboxd/src/snapshots.rs'),
        'diagnostic-scored': lambda r: r['runs'][0]['prepared_report'].update(diagnostic_only=True),
        'wrong-binary': lambda r: r['runs'][0]['prepared_report']['artifact_sha256'].update(hypermachine='0' * 64),
        'missing-guest-sample': lambda r: r['runs'][0]['prepared_report']['runs'][0]['samples'].pop(),
        'failed-guest-cleanup': lambda r: r['runs'][0]['prepared_report']['runs'][0]['samples'][0].update(cleanup_success=False),
        'unmatched-clock-rng': lambda r: r['runs'][0]['prepared_report']['guest_restore_contract'].update(clock_rng_resynchronised=False),
    }
    def missing_image(r):
        prep = next(row for row in r['runs'] if row['variant']=='candidate')['prepared_report']['preparation']['hypermachine']
        name = next(n for n in prep['source_files'] if n.endswith('.snap.mem') and Path(n).name.startswith('warm-benchmark-'))
        prep['source_files'].pop(name)
    mutations.update({
        'missing-full-named-image': missing_image,
        'invalid-capture-cost': lambda r: r['runs'][0]['prepared_report']['preparation']['hypermachine'].update(named_capture_ms=float('nan')),
        'uncorrected-resource-phase': lambda r: r['runs'][0]['prepared_report'].pop('resource_validation_phase'),
    })
    for name, mutate in mutations.items():
        damaged = copy.deepcopy(report)
        mutate(damaged)
        try:
            module.analyze(damaged, directory)
        except (ValueError, KeyError):
            print(name + ': rejected')
        else:
            raise AssertionError(name + ': accepted damaged evidence')
    print(f'real comparison accepted; {len(mutations)} damaged contracts rejected')


if __name__ == '__main__':
    main()
