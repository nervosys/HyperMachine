#!/usr/bin/env python3
"""Reject malformed paired prepared-restore admission evidence with -O."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def check(path,directory):
    spec=importlib.util.spec_from_file_location('admission_negative',directory/'analyze-restore-admission.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    raw=json.loads(path.read_text());module.analyze(raw,directory)
    def nested(value):return value['runs'][0]['prepared_report']
    cases={}
    def case(name,change):
        value=copy.deepcopy(raw);change(value);cases[name]=value
    case('missing_run',lambda r:r['runs'].pop())
    case('wrong_order',lambda r:r['runs'][0].update(variant='candidate'))
    case('wrong_binary',lambda r:nested(r)['artifact_sha256'].update(hypermachine='0'*64))
    case('diagnostic_ranked',lambda r:nested(r).update(diagnostic_only=True))
    case('prepared_source_changed',lambda r:nested(r)['preparation']['hypermachine'].update(source_unchanged=False))
    case('wrong_resources',lambda r:nested(r).update(memory_mb=512))
    case('state_unverified',lambda r:nested(r)['runs'][0]['samples'][0].update(prepared_process_environment=False))
    case('adoption_claim',lambda r:r.update(runtime_change_adopted=True))
    case('wrong_intervention',lambda r:r['intervention'].update(kind='startup_trim'))
    case('zero_slots',lambda r:r['intervention'].update(slots=0))
    case('boolean_slots',lambda r:r['intervention'].update(slots=True))
    case('malformed_source_binding',lambda r:r['intervention'].update(candidate_main_sha256='unknown'))
    case('restore_contract_absent',lambda r:nested(r).pop('guest_restore_contract'))
    case('clock_rng_unverified',lambda r:nested(r)['runs'][0]['samples'][0].update(clock_rng_resynchronised=False))
    rejected=[]
    for name,value in cases.items():
        try:module.analyze(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(name)
        else:raise ValueError('malformed evidence accepted: '+name)
    return {'rejected':rejected,'count':len(rejected)}


def check_recovery(path,directory):
    spec=importlib.util.spec_from_file_location('admission_recovery_negative',directory/'analyze-restore-admission.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    raw=json.loads(path.read_text());module.analyze_recovery(raw,directory);cases={}
    def cycle(value):return value['fault_cycles'][0]
    def sample(value):return cycle(value)['samples'][0]
    def case(name,change):
        value=copy.deepcopy(raw);change(value);cases[name]=value
    case('missing_fault_cycle',lambda r:r['fault_cycles'].pop())
    case('missing_fault_attempt',lambda r:cycle(r)['samples'].pop())
    case('wrong_fault_status',lambda r:sample(r).update(error='HTTP 404: not found'))
    case('wrong_failure_stage',lambda r:sample(r).update(error='HTTP 500: metadata missing'))
    case('unexpected_created_guest',lambda r:sample(r).update(unexpected_sandbox_id='sbx-unknown'))
    case('source_not_restored',lambda r:cycle(r).update(source_restored=False))
    case('source_fingerprint_changed',lambda r:cycle(r).update(source_sha256='0'*64))
    case('guest_retained',lambda r:cycle(r).update(remaining_sandboxes=1))
    case('recovery_failed',lambda r:cycle(r).update(recovery_success=False))
    case('ranking_claim',lambda r:r.update(diagnostic_only=False))
    case('runtime_adoption',lambda r:r.update(runtime_change_adopted=True))
    case('invalid_failure_time',lambda r:sample(r).update(elapsed_ms=float('nan')))
    rejected=[]
    for name,value in cases.items():
        try:module.analyze_recovery(value,directory)
        except (ValueError,KeyError,TypeError):rejected.append(name)
        else:raise ValueError('malformed recovery accepted: '+name)
    return {'rejected':rejected,'count':len(rejected)}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--directory',type=Path,default=Path(__file__).parent);parser.add_argument('--recovery',action='store_true');args=parser.parse_args();print(json.dumps((check_recovery if args.recovery else check)(args.report,args.directory),indent=2))
