#!/usr/bin/env python3
"""Reject malformed startup-trim comparison evidence with -O enabled."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def check(report,directory):
    spec=importlib.util.spec_from_file_location('startup_negative',directory/'analyze-startup-reclaim.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    raw=json.loads(report.read_text());module.analyze(raw,directory)
    def missing_run(r):r['runs'].pop()
    def wrong_order(r):r['runs'][0]['variant']='candidate'
    def wrong_binary(r):r['runs'][0]['prepared_report']['artifact_sha256']['hypermachine']='0'*64
    def missing_trim(r):r['runs'][1]['prepared_report']['node_log_tail']=''
    def repeated_trim(r):r['runs'][1]['prepared_report']['node_log_tail']+='\nHV2_STARTUP_RECLAIM_EXPERIMENT released=1 duration_ns=123\n'
    def diagnostic(r):r['runs'][0]['prepared_report']['diagnostic_only']=True
    def source_changed(r):r['runs'][0]['prepared_report']['preparation']['hypermachine']['source_unchanged']=False
    def wrong_resources(r):r['runs'][0]['prepared_report']['memory_mb']=512
    def missing_state(r):r['runs'][0]['prepared_report']['runs'][0]['samples'][0]['prepared_process_environment']=False
    def adoption_claim(r):r['runtime_change_adopted']=True
    rejected=[]
    for mutate in [missing_run,wrong_order,wrong_binary,missing_trim,repeated_trim,diagnostic,source_changed,wrong_resources,missing_state,adoption_claim]:
        value=copy.deepcopy(raw);mutate(value)
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(mutate.__name__)
        else:raise ValueError('malformed evidence accepted: '+mutate.__name__)
    return {'rejected':rejected,'count':len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--directory',type=Path,default=Path(__file__).parent);args=parser.parse_args();print(json.dumps(check(args.report,args.directory),indent=2))
