#!/usr/bin/env python3
"""Validate same-binary diagnostic stages without promoting them to ranked results."""
import argparse
import importlib.util
import json
from pathlib import Path


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def analyze(report, directory):
    modes = load('buffer_stage_modes', directory / 'analyze-boot-buffer-modes.py')
    checked = modes.analyze(report, directory, diagnostic=True)
    phases = load('buffer_client_phases', directory / 'analyze-prepared-phases.py')
    creation = load('buffer_server_creation', directory / 'analyze-prepared-creation.py')
    rows = []
    for row in report['runs']:
        raw = row['prepared_report']
        modes.require(raw['readiness_diagnostics']['creation_stages'] is True, 'server stages absent')
        client = phases.analyze(raw, directory)
        server = creation.analyze(raw, directory)
        modes.require(client['cleanup_verified'] and server['cleanup_verified'], 'stage cleanup differs')
        rows.append({'pair': row['pair'], 'mode': row['mode'], 'activation': row['activation'],
                     'client_phases': client, 'server_creation': server})
    return {'diagnostic_only': True, 'performance_win_established': False,
            'runtime_change_adopted': False, 'managed_competitor_win_established': False,
            'same_executable_verified': checked['same_executable_verified'],
            'runtime_activation_verified': checked['runtime_activation_verified'],
            'cohort_success': checked['cohort_success'], 'cleanup_verified': checked['cleanup_verified'],
            'runs': rows, 'limitations': ['Debug logging may change timing; excluded from rankings',
                                       'Client execution remains an aggregate phase',
                                       'Stage timing does not identify CPU ownership or a causal runtime fix']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('preserve previous stage analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()), Path(__file__).parent), indent=2) + '\n')
