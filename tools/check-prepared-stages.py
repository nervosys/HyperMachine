#!/usr/bin/env python3
"""Reject corrupted trace identities, timings and incomplete diagnostic logs."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re


def check(raw,directory):
    spec=importlib.util.spec_from_file_location('stage_checks',directory/'analyze-prepared-stages.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    module.analyze(raw,directory)
    cases={}
    def case(name,change):
        value=copy.deepcopy(raw);change(value);cases[name]=value
    def diagnostic(value):return value['readiness_diagnostics']
    def sample(value):return next(r for r in value['runs'] if r['engine']=='hypermachine')['samples'][0]
    def rewrite(value,change):
        item=diagnostic(value);item['node_log']=change(item['node_log']);item['log_bytes']=len(item['node_log'].encode())
    case('not_diagnostic',lambda v:v.update(diagnostic_only=False))
    case('truncated_log',lambda v:diagnostic(v).update(log_truncated=True))
    case('wrong_filter',lambda v:diagnostic(v).update(rust_log='debug'))
    case('byte_count',lambda v:diagnostic(v).update(log_bytes=0))
    case('missing_sample_identity',lambda v:sample(v).pop('sandbox_id'))
    case('unmatched_sample_identity',lambda v:sample(v).update(sandbox_id='sbx-'+'0'*20))
    case('missing_traces',lambda v:rewrite(v,lambda s:'no trace\n'))
    case('duplicate_trace',lambda v:rewrite(v,lambda s:s+s))
    def plain_change(value,key,new):
        rewrite(value,lambda s:re.sub(key+r'=\S+',key+'='+new,module.ANSI.sub('',s)))
    case('nan_duration',lambda v:plain_change(v,'blocking_queue_ms','NaN'))
    case('negative_duration',lambda v:plain_change(v,'connect_ms','-1'))
    case('stage_exceeds_create',lambda v:plain_change(v,'restored_ms','100000000'))
    case('failed_server_trace',lambda v:plain_change(v,'succeeded','false'))
    case('missing_stage',lambda v:rewrite(v,lambda s:re.sub(r' restored_ms=\S+','',module.ANSI.sub('',s))))
    rejected=[]
    for name,value in cases.items():
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(name)
        else:raise ValueError('malformed diagnostic accepted: '+name)
    return {'rejected':rejected,'count':len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);args=parser.parse_args()
    print(json.dumps(check(json.loads(args.report.read_text()),Path(__file__).parent),indent=2))
