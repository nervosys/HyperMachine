#!/usr/bin/env python3
"""Reject malformed reclaim evidence with assertions disabled."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def check(report,directory):
    spec=importlib.util.spec_from_file_location('reclaim_negative',directory/'analyze-prepared-reclaim.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    raw=json.loads(report.read_text());module.analyze(raw,directory)
    def incomplete(r):r['runs'].pop()
    def order(r):r['runs'][0]['variant']='trim'
    def repeated_operation(r):r['runs'][0]['operations'].append(r['runs'][0]['operations'][0])
    def wrong_operation(r):r['runs'][0]['operations'][0]['operation']='trim'
    def different_pid(r):r['runs'][0]['observations'][1]['pid']+=1
    def fabricated_pss(r):r['runs'][0]['observations'][0]['groups']['heap']['Pss_kib']+=1
    def source_changed(r):r['runs'][0]['prepared_report']['preparation']['firecracker']['source_unchanged']=False
    def guest_failed(r):r['runs'][0]['prepared_report']['runs'][0]['samples'][0]['prepared_file']=False
    def different_binary(r):r['runs'][0]['prepared_report']['artifact_sha256']['hypermachine']='0'*64
    def claim(r):r['runtime_change_adopted']=True
    rejected=[]
    for mutate in [incomplete,order,repeated_operation,wrong_operation,different_pid,fabricated_pss,source_changed,guest_failed,different_binary,claim]:
        value=copy.deepcopy(raw);mutate(value)
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(mutate.__name__)
        else:raise ValueError('malformed evidence accepted: '+mutate.__name__)
    return {'rejected':rejected,'count':len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--directory',type=Path,default=Path(__file__).parent);args=parser.parse_args();print(json.dumps(check(args.report,args.directory),indent=2))
