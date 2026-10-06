#!/usr/bin/env python3
"""Correlate accepted daemon build/launch logs with prepared client timings."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re
import statistics


def require(value,message):
    if not value:raise ValueError(message)


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


DURATION=r'[0-9]+(?:\.[0-9]+)?(?:ns|µs|ms|s)'
CREATION=re.compile(r'hv2_sandboxd: (sbx-[0-9a-f]{20}) up in ('+DURATION+r'): build ('+DURATION+r'), launch ('+DURATION+r'), agent answering ('+DURATION+r'),\s+network and envd ('+DURATION+r')')


def milliseconds(text):
    match=re.fullmatch(r'([0-9]+(?:\.[0-9]+)?)(ns|µs|ms|s)',text)
    require(match is not None,'invalid Rust duration')
    value=float(match[1])*{'ns':1e-6,'µs':1e-3,'ms':1,'s':1000}[match[2]]
    require(math.isfinite(value) and value>=0,'invalid creation duration')
    return value


def traces(text,ansi):
    result={}
    for raw in text.splitlines():
        line=ansi.sub('',raw)
        if ' up in ' not in line:continue
        require('hv2_sandboxd: ' in line,'unexpected creation trace target')
        match=CREATION.fullmatch(line[line.index('hv2_sandboxd: '):])
        require(match is not None,'malformed creation trace')
        vm,*values=match.groups();require(vm not in result,'duplicate creation identity')
        result[vm]=dict(zip(['logged_up','build','launch','agent_answering','network_envd'],map(milliseconds,values)))
    require(result,'no creation traces captured')
    return result


def analyze(report,directory):
    stages=load('creation_readiness',directory/'analyze-prepared-stages.py')
    checked=stages.analyze(report,directory)
    diagnostic=report['readiness_diagnostics'];require(diagnostic.get('creation_stages') is True,'creation collection absent')
    captured=traces(diagnostic['node_log'],stages.ANSI);readiness=stages.traces(diagnostic['node_log'])
    matched=[];seen=set();unmatched=[]
    for run in report['runs']:
        if run['engine']!='hypermachine':continue
        for sample in run['samples']:
            if not sample['success']:
                unmatched.append({'pair':run['pair'],'index':sample['index'],'sandbox_id':sample.get('sandbox_id')});continue
            vm=sample['sandbox_id'];require(vm not in seen and vm in captured,'successful creation identity missing or duplicated');seen.add(vm)
            values=captured[vm];create=sample['latency_phases_ms']['create_and_notice']
            component_sum=math.fsum(values[k] for k in ['build','launch','agent_answering','network_envd'])
            require(max(component_sum,values['logged_up'])<=create+0.001,'server creation stages exceed client create')
            readiness_ms=math.fsum(readiness[vm]['stages_ms'].values())
            require(readiness_ms<=values['agent_answering']+0.001,'readiness stages exceed enclosing agent phase')
            matched.append({'pair':run['pair'],'index':sample['index'],'sandbox_id':vm,'ready_ms':sample['ready_ms'],'create_ms':create,'exec_ms':sample['latency_phases_ms']['exec'],'creation_stages_ms':values,'component_sum_ms':component_sum,'outside_creation_ms':max(0,create-component_sum),'readiness_ms':readiness_ms,'agent_other_ms':max(0,values['agent_answering']-readiness_ms)})
    base=load('creation_base',directory/'analyze-prepared-engines.py')
    keys=['build','launch','agent_answering','network_envd','outside_creation','exec']
    def value(row,key):return row['creation_stages_ms'][key] if key in row['creation_stages_ms'] else row[key+'_ms']
    def summarize(rows):
        total=math.fsum(r['ready_ms'] for r in rows);output={}
        for key in keys:
            values=[value(r,key) for r in rows]
            output[key]={'mean_ms':statistics.mean(values) if values else None,'p50_ms':base.percentile(values,.5),'p95_ms':base.percentile(values,.95),'p99_ms':base.percentile(values,.99),'fraction_of_total_time':math.fsum(values)/total if total else None}
        return output
    slow=[r for r in matched if r['ready_ms']>=1000]
    return {'diagnostic_only':True,'runtime_change_adopted':False,'performance_win_established':False,'cohort_success':checked['cohort_success'],'cleanup_verified':checked['cleanup_verified'],'matched_successful_hypermachine_attempts':len(matched),'unmatched_failed_attempts':unmatched,'uncorrelated_creation_trace_ids':sorted(set(captured)-seen),'slow_threshold_ms':1000,'slow_attempts':len(slow),'all_stages':summarize(matched),'slow_stages':summarize(slow),'ten_slowest':sorted(matched,key=lambda r:r['ready_ms'],reverse=True)[:10],'limitations':['Existing selective logging can perturb timing; this is not a ranking cohort','Build includes preparation before new_vm returns; launch covers accepted launch_from_snapshot_prefaulted','Agent answering encloses existing queue/connection/Restored observations plus other scheduling time','Network/envd is the existing elapsed phase name; this fixture has no network or volume mounts','Outside creation includes HTTP/handler and registration/response work, not a measured internal cause','Logged up total and final phase use separate elapsed readings; component sum is retained independently','Stage percentiles are not additive; all slow observations remain in overall summaries']}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();require(not args.output.exists(),'output exists; preserve earlier analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
