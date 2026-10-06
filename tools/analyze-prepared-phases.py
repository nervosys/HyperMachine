#!/usr/bin/env python3
"""Validate client phase accounting and locate prepared-readiness tails."""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics


def require(value,message):
    if not value:raise ValueError(message)


def analyze(raw,directory):
    spec=importlib.util.spec_from_file_location('prepared_phase_base',directory/'analyze-prepared-engines.py');base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
    require(raw.get('latency_phase_measurement')=='client_monotonic_no_added_rpc','phase collection absent')
    checked=base.analyze(raw);require(checked.get('matched_guest_restore_contract') is True,'guest contract differs')
    result={'diagnostic_phase_accounting':True,'engines':{},'cleanup_verified':checked['cleanup_verified'],'attempted':sum(v['attempted'] for v in checked['engines'].values()),'passed':sum(v['passed'] for v in checked['engines'].values()),'cohort_success':checked['cohort_success'],'runtime_change_adopted':False,'managed_competitor_win_established':False,'slow_threshold_ms':1000,'limitations':['Client create includes HTTP queueing, provisioning, snapshot restore, guest maintenance and response delivery','Client phases do not separate server CPU, blocking queues or guest scheduling','Phase percentiles need not sum to total readiness percentiles','Slow samples are described, never discarded from overall statistics']}
    for engine in ['hypermachine','firecracker']:
        samples=[{'pair':r['pair'],**s} for r in raw['runs'] if r['engine']==engine for s in r['samples'] if s['success'] and s['cleanup_success']]
        keys=['create_and_notice','exec'] if engine=='hypermachine' else ['process_and_load','connect_and_notice','exec']
        phases={}
        for key in keys:
            values=[s['latency_phases_ms'][key] for s in samples]
            phases[key]={'mean_ms':statistics.mean(values) if values else None,'p50_ms':base.percentile(values,.5),'p95_ms':base.percentile(values,.95),'p99_ms':base.percentile(values,.99),'at_least_1000_ms':sum(v>=1000 for v in values)}
        slow=[s for s in samples if s['ready_ms']>=1000]
        total=sum(s['ready_ms'] for s in slow)
        result['engines'][engine]={'readiness':checked['engines'][engine],'phases':phases,'slow_successful_attempts':len(slow),'slow_phase_time_fractions':{key:sum(s['latency_phases_ms'][key] for s in slow)/total if total else None for key in keys},'ten_slowest':[{'pair':s['pair'],'index':s['index'],'ready_ms':s['ready_ms'],'latency_phases_ms':s['latency_phases_ms']} for s in sorted(samples,key=lambda s:s['ready_ms'],reverse=True)[:10]]}
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args();require(not args.output.exists(),'output exists; retain prior analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
