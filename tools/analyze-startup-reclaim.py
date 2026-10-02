#!/usr/bin/env python3
"""Validate fresh-daemon startup trim comparison including control attempts."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import statistics


def require(value,message):
    if not value:raise ValueError(message)


def analyze(report,directory):
    spec=importlib.util.spec_from_file_location('startup_warm_analysis',directory/'analyze-prepared-engines.py');warm=importlib.util.module_from_spec(spec);spec.loader.exec_module(warm)
    require(report['artifacts_unchanged'] is True and report['runtime_change_adopted'] is False and report['managed_competitor_win_established'] is False,'input or claim differs')
    require(len(report['runs'])==report['pairs']*2,'planned outer runs missing')
    values={k:[] for k in ['baseline','candidate']};controls={k:[] for k in values};memory={k:[] for k in values};incremental={k:[] for k in values};nested=[]
    require(report['success']==all(r['success'] for r in report['runs']),'outer success inconsistent')
    for index,row in enumerate(report['runs']):
        order=['baseline','candidate'] if (index//2)%2==0 else ['candidate','baseline']
        variant=row['variant'];require(row['pair']==index//2 and variant==order[index%2],'counterbalance differs')
        raw=row['prepared_report']
        require(raw.get('diagnostic_only',False) is False,'mapping/preload diagnostic not scored')
        require(raw['pairs']==2 and raw['concurrency']==report['concurrency'],'nested profile differs')
        require(raw['artifact_sha256']['hypermachine']==report['artifact_sha256'][variant],'variant executable differs')
        require(all(raw['artifact_sha256'][k]==report['artifact_sha256'][k] for k in ['firecracker','kernel','initrd','coordinator','engines','firecracker_harness']),'nested inputs differ')
        result=warm.analyze(raw);require(row['success']==result['cohort_success'],'nested success inconsistent')
        markers=re.findall(r'HV2_STARTUP_RECLAIM_EXPERIMENT released=([01]) duration_ns=(\d+)',raw.get('node_log_tail',''))
        require(len(markers)==(1 if variant=='candidate' else 0),'one-time startup intervention unverified')
        for batch in raw['runs']:
            samples=[s['ready_ms'] for s in batch['samples'] if s['success'] and s['cleanup_success']]
            (values if batch['engine']=='hypermachine' else controls)[variant].extend(samples)
            if batch['engine']=='hypermachine' and batch['success']:
                memory[variant].append(batch['held_process_memory_kib']['Pss_kib']/1024)
                incremental[variant].append(batch['incremental_process_memory_kib']['Pss_kib']/1024)
        nested.append({'pair':row['pair'],'variant':variant,'cleanup_verified':result['cleanup_verified'],'engines':result['engines'],'startup_trim':{'released':int(markers[0][0]),'duration_ms':int(markers[0][1])/1000000} if markers else None})
    summary={}
    for variant in values:
        planned=report['pairs']*2*report['concurrency'];samples=values[variant]
        summary[variant]={'planned':planned,'passed':len(samples),'failed':planned-len(samples),'successful_p50_ms':warm.percentile(samples,.5),'successful_p95_ms':warm.percentile(samples,.95),'successful_p99_ms':warm.percentile(samples,.99),'median_held_pss_mib':statistics.median(memory[variant]) if memory[variant] else None,'median_incremental_pss_mib':statistics.median(incremental[variant]) if incremental[variant] else None,'firecracker_control_passed':len(controls[variant]),'firecracker_control_planned':planned,'firecracker_control_p50_ms':warm.percentile(controls[variant],.5),'firecracker_control_p99_ms':warm.percentile(controls[variant],.99)}
    pairs=[]
    for pair in range(report['pairs']):
        by={r['variant']:r['prepared_report'] for r in report['runs'] if r['pair']==pair}
        complete=all(r['success'] for r in by.values());out={'pair':pair,'complete':complete}
        if complete:
            samples={v:[s['ready_ms'] for b in raw['runs'] if b['engine']=='hypermachine' for s in b['samples']] for v,raw in by.items()}
            held={v:statistics.median(b['held_process_memory_kib']['Pss_kib']/1024 for b in raw['runs'] if b['engine']=='hypermachine') for v,raw in by.items()}
            control_samples={v:[s['ready_ms'] for b in raw['runs'] if b['engine']=='firecracker' for s in b['samples']] for v,raw in by.items()}
            out.update(mean_reduction_ms=statistics.mean(samples['baseline'])-statistics.mean(samples['candidate']),p99_reduction_ms=warm.percentile(samples['baseline'],.99)-warm.percentile(samples['candidate'],.99),held_reduction_mib=held['baseline']-held['candidate'],firecracker_control_mean_shift_ms=statistics.mean(control_samples['candidate'])-statistics.mean(control_samples['baseline']))
        pairs.append(out)
    return {'variants':summary,'pairs':pairs,'cohorts':nested,'cleanup_verified':all(r['cleanup_verified'] for r in nested),'cohort_success':report['success'],'latencies_conditional_on_success':True,'memory_conditional_on_complete_successful_batches':True,'runtime_change_adopted':False,'managed_competitor_win_established':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args();require(not args.output.exists(),'output exists; preserve prior analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
