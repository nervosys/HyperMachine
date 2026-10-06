#!/usr/bin/env python3
"""Reject malformed creation traces and inherited readiness corruption cases."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def check(raw,directory):
    inherited=load('creation_inherited_checks',directory/'check-prepared-stages.py').check(raw,directory)
    module=load('creation_negative_checks',directory/'analyze-prepared-creation.py');module.analyze(raw,directory)
    cases={}
    def case(name,change):
        value=copy.deepcopy(raw);change(value);cases[name]=value
    def rewrite(value,change):
        item=value['readiness_diagnostics'];ansi=re.compile(r'\x1b\[[0-9;]*m')
        item['node_log']=change(ansi.sub('',item['node_log']));item['log_bytes']=len(item['node_log'].encode())
    case('creation_metadata_absent',lambda v:v['readiness_diagnostics'].pop('creation_stages'))
    case('creation_traces_absent',lambda v:rewrite(v,lambda s:'\n'.join(line for line in s.splitlines() if ' up in ' not in line)))
    case('creation_duplicate',lambda v:rewrite(v,lambda s:s+'\n'+'\n'.join(line for line in s.splitlines() if ' up in ' in line)))
    case('creation_negative',lambda v:rewrite(v,lambda s:re.sub(r'build '+module.DURATION,'build -1ms',s)))
    case('creation_nan',lambda v:rewrite(v,lambda s:re.sub(r'launch '+module.DURATION,'launch NaNms',s)))
    case('creation_unknown_unit',lambda v:rewrite(v,lambda s:re.sub(r'network and envd '+module.DURATION,'network and envd 1sec',s)))
    case('creation_exceeds_client',lambda v:rewrite(v,lambda s:re.sub(r'launch '+module.DURATION,'launch 100000s',s)))
    case('agent_phase_excludes_readiness',lambda v:rewrite(v,lambda s:re.sub(r'agent answering '+module.DURATION,'agent answering 0ns',s)))
    case('creation_wrong_target',lambda v:rewrite(v,lambda s:s.replace('hv2_sandboxd:','unexpected:')))
    case('creation_unmatched_identity',lambda v:rewrite(v,lambda s:re.sub(r'hv2_sandboxd: sbx-[0-9a-f]{20}','hv2_sandboxd: sbx-'+'0'*20,s)))
    rejected=[]
    for name,value in cases.items():
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(name)
        else:raise ValueError('malformed creation accepted: '+name)
    return {'readiness_rejected':inherited['rejected'],'creation_rejected':rejected,'count':inherited['count']+len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);args=parser.parse_args();print(json.dumps(check(json.loads(args.report.read_text()),Path(__file__).parent),indent=2))
