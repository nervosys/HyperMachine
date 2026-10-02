#!/usr/bin/env python3
"""Require rejection of malformed prepared-engine evidence, including with -O."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def check(analyzer, report):
    spec = importlib.util.spec_from_file_location('prepared_negative', analyzer)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    raw = json.loads(report.read_text())
    module.analyze(raw)
    def missing_run(r): r['runs'].pop()
    def resources(r): r['memory_mb'] = 512
    def state(r): r['runs'][0]['samples'][0]['prepared_process_environment'] = False
    def time(r): r['runs'][0]['samples'][0]['ready_ms'] = float('nan')
    def hold(r): r['runs'][0]['memory_hold_seconds'] = 0
    def memory(r): r['runs'][0]['incremental_process_memory_kib']['Pss_kib'] += 1
    def order(r): r['runs'][0]['engine'] = 'firecracker'
    def count(r): r['pairs'] += 1
    def source(r): r['preparation']['hypermachine']['offering']['snapshotID'] = 'base'
    def full(r): r['preparation']['firecracker']['memory_bytes'] = 1
    def changed(r): r['preparation']['firecracker']['source_unchanged'] = False
    def cleanup(r): r['runs'][0]['samples'][0]['cleanup_success'] = False
    rejected = []
    for mutation in [missing_run,resources,state,time,hold,memory,order,count,source,full,changed,cleanup]:
        candidate = copy.deepcopy(raw)
        mutation(candidate)
        try:
            module.analyze(candidate)
        except (ValueError,KeyError,TypeError):
            rejected.append(mutation.__name__)
        else:
            raise ValueError('malformed evidence accepted: '+mutation.__name__)
    return {'rejected':rejected, 'count':len(rejected)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('analyzer', type=Path)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    print(json.dumps(check(args.analyzer,args.report), indent=2))
