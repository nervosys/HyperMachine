#!/usr/bin/env python3
"""Replay actual mode comparisons and reject damaged activation or guest evidence."""
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
    spec = importlib.util.spec_from_file_location('buffer_mode_analysis_check', directory / 'analyze-boot-buffer-modes.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    report = json.loads(args.report.read_text())
    checked = module.analyze(report, directory)
    if not checked['cohort_success'] or not checked['cleanup_verified']:
        raise ValueError('requires complete actual mode comparison')
    mutations = {
        'published-runtime-claim': lambda r: r.update(runtime_change_adopted=True),
        'changed-artifact': lambda r: r.update(artifacts_unchanged=False),
        'missing-outer-run': lambda r: r['runs'].pop(),
        'wrong-order': lambda r: r['runs'][0].update(mode='borrowed'),
        'missing-source-binding': lambda r: r['mode_source_sha256'].pop('crates/hv2-core/src/backends/kvm.rs'),
        'missing-runtime-activation': lambda r: r['runs'][0].update(activation=[]),
        'activation-after-scoring': lambda r: r['runs'][0]['activation'][0].update(captured_before_scoring=False),
        'wrong-runtime-mode': lambda r: r['runs'][0]['activation'][0].update(observed='borrowed'),
        'diagnostic-scored': lambda r: r['runs'][0]['prepared_report'].update(diagnostic_only=True),
        'different-executable': lambda r: r['runs'][0]['prepared_report']['artifact_sha256'].update(hypermachine='0' * 64),
        'missing-guest-sample': lambda r: r['runs'][0]['prepared_report']['runs'][0]['samples'].pop(),
        'failed-guest-cleanup': lambda r: r['runs'][0]['prepared_report']['runs'][0]['samples'][0].update(cleanup_success=False),
        'unmatched-clock-rng': lambda r: r['runs'][0]['prepared_report']['guest_restore_contract'].update(clock_rng_resynchronised=False),
    }
    if 'resource_validation_phase' in report['runs'][0]['prepared_report']:
        def nested(r): return r['runs'][0]['prepared_report']
        mutations.update({
            'resource-check-during-timing': lambda r: nested(r).update(resource_validation_phase='during_timing'),
            'missing-batch-resource-phase': lambda r: nested(r)['runs'][0].pop('resource_validation_phase'),
            'missing-verified-resources': lambda r: nested(r)['runs'][0]['samples'][0].pop('verified_resources'),
            'wrong-verified-memory': lambda r: nested(r)['runs'][0]['samples'][0]['verified_resources'].update(memory_mb=512),
            'boolean-verified-cpu': lambda r: nested(r)['runs'][0]['samples'][0]['verified_resources'].update(cpu_count=True),
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
    print(f'actual same-binary comparison accepted; {len(mutations)} damaged contracts rejected')


if __name__ == '__main__':
    main()
