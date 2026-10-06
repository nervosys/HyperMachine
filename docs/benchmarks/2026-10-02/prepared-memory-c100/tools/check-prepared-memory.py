#!/usr/bin/env python3
"""Require malformed mapping diagnostic rejection, with assertions disabled."""
import argparse
import copy
import json
from pathlib import Path
from importlib.util import spec_from_file_location, module_from_spec


def check(report, directory):
    spec=spec_from_file_location('memory_analysis',directory/'analyze-prepared-memory.py');module=module_from_spec(spec);spec.loader.exec_module(module)
    raw=json.loads(report.read_text());module.analyze(raw,directory)
    def missing_process(r): r['runs'][1]['held_mapping_observations'].pop()
    def duplicated_pid(r): r['runs'][1]['held_mapping_observations'][1]['pid']=r['runs'][1]['held_mapping_observations'][0]['pid']
    def fabricated_pss(r): r['runs'][0]['held_mapping_observations'][0]['groups']['guest_sized']['Pss_kib']+=1
    def missing_raw(r): r['runs'][0]['held_mapping_observations'][0]['raw_smaps']=''
    def invalid_unit(r):
        o=r['runs'][0]['held_mapping_observations'][0];o['raw_smaps']=o['raw_smaps'].replace('kB','bytes',1)
    def no_baseline(r): r['runs'][0]['empty_mapping_observations']=[]
    def scoring_claim(r): r['diagnostic_only']=False
    rejected=[]
    for mutation in [missing_process,duplicated_pid,fabricated_pss,missing_raw,invalid_unit,no_baseline,scoring_claim]:
        value=copy.deepcopy(raw);mutation(value)
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(mutation.__name__)
        else:raise ValueError('malformed diagnostic accepted: '+mutation.__name__)
    return {'rejected':rejected,'count':len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--directory',type=Path,default=Path(__file__).parent);args=parser.parse_args()
    print(json.dumps(check(args.report,args.directory),indent=2))
