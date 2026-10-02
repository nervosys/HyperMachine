#!/usr/bin/env python3
"""Reject malformed cold-budget memory reports, including under Python -O."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location('measurement', Path(__file__).with_name('analyze-cold-budget-memory.py'))
measurement = importlib.util.module_from_spec(spec); spec.loader.exec_module(measurement)


def check(report):
    measurement.analyze(report)
    rejected = []
    cases = [
        ('wrong_hold',lambda r:r.__setitem__('memory_idle_seconds',0)),
        ('shortened_hold',lambda r:r['runs'][0]['batch'].__setitem__('memory_idle_actual_seconds',4)),
        ('missing_pss',lambda r:r['runs'][0]['batch']['idle_process_memory_kib'].pop('Pss_kib')),
        ('nan_pss',lambda r:r['runs'][0]['batch']['idle_process_memory_kib'].__setitem__('Pss_kib',float('nan'))),
        ('negative_pss',lambda r:r['runs'][0]['batch']['idle_process_memory_kib'].__setitem__('Pss_kib',-1)),
        ('wrong_subtraction',lambda r:r['runs'][0]['batch']['incremental_idle_process_memory_kib'].__setitem__('Pss_kib',0)),
        ('wrong_empty_baseline',lambda r:r['runs'][0]['empty_node_memory_before_batch_kib'].__setitem__('Pss_kib',0)),
        ('missing_run',lambda r:r['runs'].pop()),
        ('wrong_budget',lambda r:r['runs'][0].__setitem__('cold_start_limit',32)),
        ('false_success',lambda r:r.__setitem__('success',False)),
        ('wrong_resource',lambda r:r.__setitem__('memory_mb',512)),
        ('failed_cleanup',lambda r:r['runs'][0].__setitem__('remaining_sandbox_count',1)),
    ]
    for name, edit in cases:
        candidate = copy.deepcopy(report); edit(candidate)
        try: measurement.analyze(candidate)
        except (ValueError, KeyError): rejected.append(name)
        else: raise RuntimeError('accepted malformed report: '+name)
    return {'rejected_reports':len(rejected), 'cases':rejected}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    print(json.dumps(check(json.loads(args.report.read_text())),indent=2))
