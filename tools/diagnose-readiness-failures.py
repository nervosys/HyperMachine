#!/usr/bin/env python3
"""Map retained readiness-timeout snapshots using an owned, identical-kernel VM."""
import argparse
import collections
import hashlib
import importlib.util
import json
from pathlib import Path
import re


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshots(failure):
    rows = []
    for run in failure['runs']:
        for sample in run.get('batch', {}).get('samples', []):
            if sample['success']:
                continue
            text = sample.get('error', '')
            owner = re.search(r'owner sample: RIP=0x([0-9a-f]{16}) .*?run_state=(\w+) .*?TSC=(0x[0-9a-f]+|unavailable) TSC_DEADLINE=(0x[0-9a-f]+|unavailable)', text)
            require(owner is not None, 'failed attempt lacks a complete owner snapshot')
            require(text.startswith('HTTP 503:') and 'within 15s' in text or text.startswith('HTTP 503:') and 'in 15s' in text, 'not a retained guest-readiness timeout')
            tsc, deadline = [int(v,16) if v != 'unavailable' else None for v in owner.groups()[2:]]
            rows.append({'pair':run['pair'], 'variant':run['variant'], 'index':sample['index'],
                'failure_phase':sample.get('failure_phase'), 'rip':owner[1], 'state':owner[2],
                'tsc':tsc, 'tsc_deadline':deadline,
                'deadline_nonzero_at_or_before_tsc':None if tsc is None or deadline is None else deadline != 0 and deadline <= tsc})
    require(rows, 'no failed attempts available')
    return rows


def mapping(address, response):
    require(response.get('exit_code') == 0 and not response.get('timed_out') and not response.get('truncated'), 'symbol query failed')
    lines = response.get('stdout','').splitlines()
    parsed = [re.fullmatch(r'([0-9a-f]{16}) ([A-Za-z]) (\S+)',line) for line in lines]
    require(len(parsed) == 2 and all(parsed), 'symbol bounds unavailable')
    lower, upper = [m.groups() for m in parsed]
    require(int(lower[0],16) <= int(address,16) < int(upper[0],16), 'address outside symbol bounds')
    return {'symbol':lower[2], 'symbol_address':lower[0], 'offset':int(address,16)-int(lower[0],16), 'next_symbol':upper[2], 'next_address':upper[0]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['firecracker','kernel','initrd','failure-report','output']:
        parser.add_argument('--'+name,type=Path,required=True)
    args = parser.parse_args()
    require(not args.output.exists(), 'output exists; preserve previous evidence')
    paths = {name:getattr(args,name) for name in ['firecracker','kernel','initrd','failure_report']}
    failure = json.loads(args.failure_report.read_text())
    require(all(digest(paths[n]) == failure['artifact_sha256'][n] for n in ['kernel','initrd']), 'guest images differ from failure cohort')
    rows = snapshots(failure)
    spec = importlib.util.spec_from_file_location('fc', Path(__file__).with_name('bench-firecracker-local.py'))
    fc = importlib.util.module_from_spec(spec); spec.loader.exec_module(fc)
    paths.update(coordinator=Path(__file__), harness=Path(fc.__file__))
    hashes = {name:digest(path) for name,path in paths.items()}
    original = fc.rpc; queries = {}
    def probe(stream, request_id, body):
        result = original(stream, request_id, body)
        if request_id == 2 and body.get('kind') == 'exec':
            for index,address in enumerate(sorted({r['rip'] for r in rows})):
                command = "awk '$1 <= \""+address+"\" {previous=$0; next} {print previous; print; exit}' /proc/kallsyms"
                response = original(stream,index+3,{'kind':'exec','program':'/bin/sh','args':['-c',command],'timeout_ms':10000})
                queries[address] = {'command':command, 'response':response}
        return result
    fc.rpc = probe; args.timeout = 30
    try: sample = fc.sample(args,0)
    finally: fc.rpc = original
    report = {'diagnostic_only':True, 'performance_comparison':False, 'artifact_sha256':hashes,
        'retained_snapshots':rows, 'sample':sample, 'symbol_queries':queries, 'success':False,
        'limitations':['Symbols are queried in a separately booted identical kernel, not the failed guests',
            'Snapshots at timeout are not execution traces or simultaneous timer measurements',
            'A deadline/TSC comparison does not prove a missed timer, listener backlog or failure cause',
            'The extra guest queries affect sample timing; no scored latency or performance claim']}
    try:
        report['mappings'] = {address:mapping(address,value['response']) for address,value in queries.items()}
        require(set(report['mappings']) == {r['rip'] for r in rows}, 'missing address mappings')
        counts = collections.Counter((report['mappings'][r['rip']]['symbol'],r['state']) for r in rows)
        report['state_counts'] = [{'symbol':k[0], 'state':k[1], 'count':v} for k,v in sorted(counts.items())]
    except ValueError as error: report['measurement_error'] = str(error)
    report['artifacts_unchanged'] = all(digest(path) == hashes[name] for name,path in paths.items())
    report['success'] = 'measurement_error' not in report and sample['success'] and sample['cleanup_success'] and report['artifacts_unchanged']
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['success','sample','artifacts_unchanged']},indent=2))
    return 0 if report['success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
