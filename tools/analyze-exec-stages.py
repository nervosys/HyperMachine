#!/usr/bin/env python3
"""Correlate private command-stage diagnostics with enclosing HTTP client time."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re
import statistics


ANSI = re.compile(r'\x1b\[[0-9;]*m')
AGENT_MESSAGE = 'guest command execution stages'
HANDLER_MESSAGE = 'sandbox command handler stages'
AGENT = re.compile(r'vm=(sbx-[0-9a-f]{20}) blocking_queue_ms=(\S+) connect_ms=(\S+) command_ms=(\S+) succeeded=(true|false) phase="(connect|command)"')
HANDLER = re.compile(r'vm=(sbx-[0-9a-f]{20}) handler_ms=(\S+) guest_exec_ms=(\S+) succeeded=(true|false)')


def require(value, message):
    if not value:
        raise ValueError(message)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def traces(text, message, pattern, target, keys):
    result, auxiliary = {}, 0
    for raw in text.splitlines():
        line = ANSI.sub('', raw)
        if message not in line:
            continue
        require(target + ':' in line, 'unexpected execution trace target')
        fields = line.split(message + ' ', 1)[1]
        if message == AGENT_MESSAGE and fields.startswith('vm=template '):
            auxiliary += 1
            continue
        match = pattern.fullmatch(fields)
        require(match is not None, 'malformed command trace')
        vm, *parts = match.groups()
        values = dict(zip(keys, map(float, parts[:len(keys)])))
        require(all(math.isfinite(value) and value >= 0 for value in values.values()), 'invalid command duration')
        success = parts[len(keys)] == 'true'
        phase = parts[-1] if message == AGENT_MESSAGE else None
        require(phase != 'connect' or not success, 'failed connection marked successful')
        result.setdefault(vm, []).append({'stages_ms': values, 'succeeded': success, 'phase': phase})
    require(result, 'no execution traces captured')
    return result, auxiliary


def analyze(report, directory):
    stages = load('exec_creation_analysis', directory / 'analyze-boot-buffer-stages.py')
    existing = stages.analyze(report, directory)
    base = load('exec_percentiles', directory / 'analyze-prepared-engines.py')
    rows = []
    for outer in report['runs']:
        raw = outer['prepared_report']
        diagnostic = raw['readiness_diagnostics']
        agent, auxiliary = traces(diagnostic['node_log'], AGENT_MESSAGE, AGENT,
                                  'hv2_agent::agent_vm', ['blocking_queue', 'connect', 'command_rpc'])
        handler, _ = traces(diagnostic['node_log'], HANDLER_MESSAGE, HANDLER,
                            'hv2_sandboxd', ['handler', 'guest_exec'])
        matched, seen = [], set()
        for batch in raw['runs']:
            if batch['engine'] != 'hypermachine':
                continue
            for sample in batch['samples']:
                if not sample['success']:
                    continue
                vm = sample['sandbox_id']
                require(vm not in seen and vm in agent and vm in handler, 'successful exec identity missing or duplicated')
                seen.add(vm)
                require(len(agent[vm]) == 1 and len(handler[vm]) == 1, 'scored execution trace ambiguous')
                a, h = agent[vm][0], handler[vm][0]
                require(a['succeeded'] and a['phase'] == 'command' and h['succeeded'], 'successful client has failed command trace')
                client = sample['latency_phases_ms']['exec']
                worker = math.fsum(a['stages_ms'].values())
                guest, total = h['stages_ms']['guest_exec'], h['stages_ms']['handler']
                require(worker <= guest + .001 and guest <= total + .001 and total <= client + .001, 'execution stages exceed enclosing phase')
                components = {**a['stages_ms'], 'guest_exec_other': max(0, guest - worker),
                              'handler_other': max(0, total - guest), 'outside_handler': max(0, client - total)}
                require(abs(math.fsum(components.values()) - client) <= .003, 'execution accounting differs')
                matched.append({'pair': batch['pair'], 'index': sample['index'], 'sandbox_id': vm,
                                'ready_ms': sample['ready_ms'], 'client_exec_ms': client, 'components_ms': components})
        keys = ['blocking_queue', 'connect', 'command_rpc', 'guest_exec_other', 'handler_other', 'outside_handler']

        def summarize(samples):
            ready_total = math.fsum(sample['ready_ms'] for sample in samples)
            exec_total = math.fsum(sample['client_exec_ms'] for sample in samples)
            result = {}
            for key in keys:
                values = [sample['components_ms'][key] for sample in samples]
                result[key] = {'mean_ms': statistics.mean(values) if values else None,
                               'p50_ms': base.percentile(values, .5), 'p95_ms': base.percentile(values, .95),
                               'p99_ms': base.percentile(values, .99),
                               'fraction_of_exec_time': math.fsum(values) / exec_total if exec_total else None,
                               'fraction_of_readiness_time': math.fsum(values) / ready_total if ready_total else None}
            return result

        slow = [sample for sample in matched if sample['ready_ms'] >= 1000]
        rows.append({'pair': outer['pair'], 'mode': outer['mode'], 'matched_attempts': len(matched),
                     'slow_attempts': len(slow), 'all_exec_stages': summarize(matched),
                     'slow_exec_stages': summarize(slow),
                     'uncorrelated_agent_ids': sorted(set(agent) - seen),
                     'uncorrelated_handler_ids': sorted(set(handler) - seen),
                     'auxiliary_template_exec_traces': auxiliary,
                     'ten_slowest': sorted(matched, key=lambda s: s['ready_ms'], reverse=True)[:10]})
    return {'diagnostic_only': True, 'performance_win_established': False, 'runtime_change_adopted': False,
            'cohort_success': existing['cohort_success'], 'cleanup_verified': existing['cleanup_verified'],
            'same_executable_verified': existing['same_executable_verified'],
            'runtime_activation_verified': existing['runtime_activation_verified'],
            'creation_analysis': existing, 'execution_analysis': rows,
            'limitations': ['Instrumentation changes allocation and timing; excluded from rankings',
                           'Command RPC includes host transport/polling and guest work, not guest CPU alone',
                           'Outside-handler time includes HTTP, scheduling and response work; its cause is unmeasured',
                           'Individual percentiles are not additive; original slow attempts remain included']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'preserve previous execution analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()), Path(__file__).parent), indent=2) + '\n')
