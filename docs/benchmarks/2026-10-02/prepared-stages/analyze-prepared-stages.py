#!/usr/bin/env python3
"""Correlate owned-daemon restore traces with client create measurements."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re
import statistics


FILTER='warn,hv2_agent::agent_vm=debug'
MESSAGE='restored guest readiness stages'
ANSI=re.compile(r'\x1b\[[0-9;]*m')
FIELDS=re.compile(r'vm=(sbx-[0-9a-f]{20}) blocking_queue_ms=(\S+) connect_ms=(\S+)(?: restored_ms=(\S+))? succeeded=(true|false) phase="(connect|restored)"')


def require(value,message):
    if not value:raise ValueError(message)


def traces(text):
    result={}
    for raw in text.splitlines():
        line=ANSI.sub('',raw)
        if MESSAGE not in line:continue
        require('hv2_agent::agent_vm:' in line,'unexpected readiness trace target')
        match=FIELDS.fullmatch(line.split(MESSAGE+' ',1)[1])
        require(match is not None,'malformed readiness trace')
        vm,queue,connect,restored,succeeded,phase=match.groups()
        values={'blocking_queue':float(queue),'connect':float(connect)}
        if restored is not None:values['restored']=float(restored)
        require(all(math.isfinite(v) and v>=0 for v in values.values()),'invalid server stage duration')
        require((phase=='restored')==('restored' in values),'phase fields differ')
        require(phase!='connect' or succeeded=='false','connect failure marked successful')
        require(vm not in result,'duplicate readiness identity')
        result[vm]={'succeeded':succeeded=='true','phase':phase,'stages_ms':values}
    require(result,'no readiness traces captured')
    return result


def analyze(report,directory):
    spec=importlib.util.spec_from_file_location('prepared_stage_base',directory/'analyze-prepared-engines.py')
    base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
    checked=base.analyze(report)
    require(report.get('diagnostic_only') is True,'logging cohort must be diagnostic')
    diagnostic=report['readiness_diagnostics']
    require(diagnostic['rust_log']==FILTER and diagnostic['logging_may_affect_timing'] is True,'diagnostic filter or timing limitation differs')
    require(diagnostic['max_log_bytes']==16*1024*1024 and diagnostic['log_truncated'] is False,'log retention incomplete')
    text=diagnostic['node_log']
    require(diagnostic['log_bytes']==len(text.encode('utf-8')) and diagnostic['log_bytes']<=diagnostic['max_log_bytes'],'log byte count differs')
    captured=traces(text);matched=[];seen=set();unmatched_attempts=[]
    for run in report['runs']:
        if run['engine']!='hypermachine':continue
        for sample in run['samples']:
            vm=sample.get('sandbox_id')
            if not sample['success']:
                unmatched_attempts.append({'pair':run['pair'],'index':sample['index'],'sandbox_id':vm})
                continue
            require(vm not in seen and vm in captured,'successful sandbox identity missing or duplicated')
            seen.add(vm);trace=captured[vm]
            require(trace['succeeded'] and trace['phase']=='restored','successful client has failed readiness trace')
            create=sample['latency_phases_ms']['create_and_notice'];stages=trace['stages_ms'];total=math.fsum(stages.values())
            require(total<=create+0.001,'server stages exceed enclosing client create')
            matched.append({'pair':run['pair'],'index':sample['index'],'sandbox_id':vm,'ready_ms':sample['ready_ms'],'create_and_notice_ms':create,'exec_ms':sample['latency_phases_ms']['exec'],'server_stages_ms':stages,'server_readiness_ms':total,'other_create_ms':max(0,create-total)})
    def summary(rows):
        keys=['blocking_queue','connect','restored','server_readiness','other_create','exec','ready']
        output={}
        for key in keys:
            values=[r['server_stages_ms'][key] if key in r['server_stages_ms'] else r[key+'_ms'] for r in rows]
            output[key]={'mean_ms':statistics.mean(values) if values else None,'p50_ms':base.percentile(values,.5),'p95_ms':base.percentile(values,.95),'p99_ms':base.percentile(values,.99)}
        return output
    slow=[r for r in matched if r['ready_ms']>=1000]
    def fractions(rows):
        total=math.fsum(r['ready_ms'] for r in rows)
        return {key:math.fsum(r['server_stages_ms'][key] if key in r['server_stages_ms'] else r[key+'_ms'] for r in rows)/total if total else None for key in ['blocking_queue','connect','restored','other_create','exec']}
    return {'diagnostic_only':True,'runtime_change_adopted':False,'performance_win_established':False,'cleanup_verified':checked['cleanup_verified'],'cohort_success':checked['cohort_success'],'matched_successful_hypermachine_attempts':len(matched),'unmatched_failed_attempts':unmatched_attempts,'uncorrelated_trace_ids':sorted(set(captured)-seen),'slow_threshold_ms':1000,'slow_attempts':len(slow),'all_stages':summary(matched),'slow_stages':summary(slow),'all_time_fractions':fractions(matched),'slow_time_fractions':fractions(slow),'ten_slowest':sorted(matched,key=lambda r:r['ready_ms'],reverse=True)[:10],'limitations':['Selective logging can perturb timings; this is not a ranking cohort','Server queue, connection and Restored RPC durations are existing accepted-daemon observations','Other create time includes HTTP, provisioning, restore and response work; its internal cause is not identified','Successful creates are correlated by source-bound VM name equal to sandbox ID; failed creates may return no ID','Individual stage percentiles are not additive; slow samples remain in overall summaries']}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args()
    require(not args.output.exists(),'output exists; retain previous evidence')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
