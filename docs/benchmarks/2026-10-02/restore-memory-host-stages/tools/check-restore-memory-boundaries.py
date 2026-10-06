#!/usr/bin/env python3
"""Replay actual boundary diagnostics and reject broken identity/order/counters."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--build-context', type=Path, required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('boundary_check_analysis', Path(__file__).with_name('analyze-restore-memory-boundaries.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    raw = json.loads(args.report.read_text())
    context = json.loads(args.build_context.read_text())
    checked = module.analyze(raw, Path(__file__).parent, context)
    if not checked['guests']:
        raise ValueError('requires actual scored guest boundaries')
    vm = checked['guests'][0]['vm']
    def trace(report, name, phase='before_run'):
        log = report['readiness_diagnostics']
        lines = log['node_log'].splitlines(True)
        index = next(i for i, line in enumerate(lines) if 'restore memory boundary diagnostic' in line and
                     'vm=' + vm + ' ' in module.ANSI.sub('', line) and 'phase="' + phase + '"' in module.ANSI.sub('', line))
        line = module.ANSI.sub('', lines[index])
        if name == 'missing': lines.pop(index)
        elif name == 'duplicate': lines.insert(index, lines[index])
        elif name == 'wrong-phase': lines[index] = line.replace('phase="' + phase + '"', 'phase="after_exec"')
        elif name == 'wrong-size': lines[index] = re.sub(r'size_kib=\d+', 'size_kib=512', line)
        elif name == 'pss-over-rss': lines[index] = re.sub(r'pss_kib=\d+', 'pss_kib=1048577', line)
        elif name == 'dirty-over-rss': lines[index] = re.sub(r'private_dirty_kib=\d+', 'private_dirty_kib=1048577', line)
        elif name == 'negative': lines[index] = re.sub(r'read_us=\d+', 'read_us=-1', line)
        elif name == 'unknown-field': lines[index] = line.rstrip('\n') + ' invented_kib=0\n'
        elif name == 'wrong-target': lines[index] = line.replace('hv2_core::vm:', 'other::vm:')
        elif name == 'anonymous-over-rss': lines[index] = re.sub(r'anonymous_kib=\d+', 'anonymous_kib=1048577', line)
        elif name == 'clean-dirty-sum': lines[index] = re.sub(r'private_clean_kib=\d+', 'private_clean_kib=1048577', line)
        log['node_log'] = ''.join(lines)
        log['log_bytes'] = len(log['node_log'].encode('utf-8'))
    mutations = {name: (lambda report, n=name: trace(report, n)) for name in
                 ('missing', 'duplicate', 'wrong-phase', 'wrong-size', 'pss-over-rss', 'dirty-over-rss',
                  'negative', 'unknown-field', 'wrong-target')}
    mutations.update({
        'scored-claim': lambda r: r.update(diagnostic_only=False),
        'wrong-binary': lambda r: r['artifact_sha256'].update(hypermachine='0' * 64),
        'concurrent-guests': lambda r: r.update(concurrency=2),
        'truncated-log': lambda r: r['readiness_diagnostics'].update(log_truncated=True),
        'failed-cleanup': lambda r: r['runs'][0]['samples'][0].update(cleanup_success=False),
        'missing-activation': lambda r: r.pop('boundary_probe_activation'),
    })
    if context.get('counter_profile') == 'anonymous_private_clean':
        mutations.update({name: (lambda report, n=name: trace(report, n)) for name in
                          ('anonymous-over-rss', 'clean-dirty-sum')})
        mutations['wrong-boundary-driver'] = lambda r: r['artifact_sha256'].update(boundary_driver='0' * 64)
    if context.get('restore_stage_profile') == 'host_restore':
        mutations.update({
            'missing-map-stage': lambda r: trace(r, 'missing', 'after_map'),
            'wrong-machine-stage': lambda r: trace(r, 'wrong-phase', 'after_machine'),
            'duplicate-vcpu-stage': lambda r: trace(r, 'duplicate', 'after_vcpu'),
            'missing-device-stage': lambda r: trace(r, 'missing', 'after_devices'),
        })
    for name, mutate in mutations.items():
        damaged = copy.deepcopy(raw)
        mutate(damaged)
        try:
            module.analyze(damaged, Path(__file__).parent, context)
        except (ValueError, KeyError):
            print(name + ': rejected')
        else:
            raise ValueError(name + ': damaged evidence accepted')
    print(f'actual single-guest boundaries accepted; {len(mutations)} damaged contracts rejected')

if __name__ == '__main__':
    main()
