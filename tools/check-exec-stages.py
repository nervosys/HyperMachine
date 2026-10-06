#!/usr/bin/env python3
"""Replay actual execution diagnostics and reject damaged trace/accounting evidence."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    directory = Path(__file__).parent
    analysis = load('exec_integrity_analysis', directory / 'analyze-exec-stages.py')
    mode = load('exec_integrity_mode', directory / 'analyze-boot-buffer-modes.py')
    report = json.loads(args.report.read_text())
    checked = analysis.analyze(report, directory)
    if not checked['cohort_success'] or not checked['cleanup_verified']:
        raise ValueError('requires complete actual execution diagnostic')
    try:
        mode.analyze(report, directory)
    except ValueError:
        print('diagnostic promoted to ranked input: rejected')
    else:
        raise AssertionError('diagnostic accepted by ranked analyzer')

    def edit_trace(data, kind):
        raw = data['runs'][0]['prepared_report']
        sample = next(batch['samples'][0] for batch in raw['runs'] if batch['engine'] == 'hypermachine')
        vm = sample['sandbox_id']
        diagnostic = raw['readiness_diagnostics']
        lines = diagnostic['node_log'].splitlines(True)
        message = analysis.HANDLER_MESSAGE if kind == 'missing-handler' else analysis.AGENT_MESSAGE
        index = next(i for i, line in enumerate(lines) if message in line and 'vm=' + vm in analysis.ANSI.sub('', line))
        lines[index] = analysis.ANSI.sub('', lines[index])
        if kind.startswith('missing'):
            lines.pop(index)
        elif kind == 'duplicate-agent':
            lines.insert(index, lines[index])
        elif kind == 'negative-queue':
            lines[index] = re.sub(r'blocking_queue_ms=\S+', 'blocking_queue_ms=-1', lines[index])
        elif kind == 'nonfinite-command':
            lines[index] = re.sub(r'command_ms=\S+', 'command_ms=nan', lines[index])
        elif kind == 'over-budget-command':
            lines[index] = re.sub(r'command_ms=\S+', 'command_ms=1000000000000', lines[index])
        elif kind == 'failed-command':
            lines[index] = lines[index].replace('succeeded=true', 'succeeded=false')
        elif kind == 'wrong-phase':
            lines[index] = lines[index].replace('phase="command"', 'phase="connect"')
        diagnostic['node_log'] = ''.join(lines)
        diagnostic['log_bytes'] = len(diagnostic['node_log'].encode('utf-8'))

    mutations = {name: (lambda r, k=name: edit_trace(r, k)) for name in
                 ('missing-agent', 'missing-handler', 'duplicate-agent', 'negative-queue',
                  'nonfinite-command', 'over-budget-command', 'failed-command', 'wrong-phase')}
    mutations.update({
        'scored-top-level': lambda r: r.update(diagnostic_only=False),
        'wrong-binary': lambda r: r['runs'][0]['prepared_report']['artifact_sha256'].update(hypermachine='0' * 64),
        'activation-after-scoring': lambda r: r['runs'][0]['activation'][0].update(captured_before_scoring=False),
        'bad-guest-cleanup': lambda r: r['runs'][0]['prepared_report']['runs'][0]['samples'][0].update(cleanup_success=False),
        'unmatched-clock-rng': lambda r: r['runs'][0]['prepared_report']['guest_restore_contract'].update(clock_rng_resynchronised=False),
    })
    for name, mutate in mutations.items():
        damaged = copy.deepcopy(report)
        mutate(damaged)
        try:
            analysis.analyze(damaged, directory)
        except (ValueError, KeyError):
            print(name + ': rejected')
        else:
            raise AssertionError(name + ': accepted damaged diagnostics')
    print('actual execution diagnostic accepted; fourteen invalid promotions/contracts rejected')


if __name__ == '__main__':
    main()
