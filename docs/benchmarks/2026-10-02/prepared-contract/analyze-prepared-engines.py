#!/usr/bin/env python3
"""Validate and summarize all prepared-snapshot benchmark attempts."""
import argparse
import json
import math
from pathlib import Path
import statistics


def require(value,message):
    if not value:raise ValueError(message)


def number(value):
    return isinstance(value,(float,int)) and not isinstance(value,bool) and math.isfinite(value)


def percentile(values,q):
    return sorted(values)[max(0,math.ceil(len(values)*q)-1)] if values else None


def analyze(report):
    require(report['cpu_count']==1 and report['memory_mb']==1024 and report['guest_readiness_timeout_s']==15,'guest resources/deadline differ')
    require(len(report['runs'])==report['pairs']*2,'incomplete planned runs')
    require(report['artifacts_unchanged'] is True,'inputs changed during cohort')
    contract=report.get('guest_restore_contract');entropy_hashes=[]
    if contract is not None:require(contract['clock_rng_resynchronised'] is True and contract['entropy_bytes']==64,'guest restore contract differs')
    prep=report['preparation'];base=prep['hypermachine']['base_template']
    require(base['snapshot'] is True and base['cpuCount']==1 and base['memoryMB']==1024,'HM base not prepared at matching resources')
    require(prep['hypermachine']['offering']['snapshotID']=='warm-benchmark:default','named snapshot differs')
    require(prep['firecracker']['snapshot_type']=='Full' and prep['firecracker']['memory_bytes']==1024*1024*1024 and prep['firecracker']['source_parent_stopped'] is True,'FC source not a stopped full snapshot')
    require(all(p['source_unchanged'] is True for p in prep.values()),'prepared source changed')
    valid_runs=all(row['success'] for row in report['runs'])
    require(report['success']==('setup_error' not in report and not report['cleanup_errors'] and report.get('owned_node_stopped') is True and valid_runs),'cohort success inconsistent')
    for i,row in enumerate(report['runs']):
        order=('hypermachine','firecracker') if (i//2)%2==0 else ('firecracker','hypermachine')
        require(row['pair']==i//2 and row['engine']==order[i%2],'counterbalance differs')
        samples=row['samples'];require(len(samples)==report['concurrency'] and {s['index'] for s in samples}==set(range(report['concurrency'])),'planned attempts missing/duplicated')
        require(number(row['start_spread_ms']) and row['start_spread_ms']>=0,'arrival spread missing')
        for sample in samples:
            if sample['success']:
                require(number(sample['ready_ms']) and sample['ready_ms']>=0,'invalid readiness time')
                require(all(sample.get(k) is True for k in ['prepared_file','prepared_process_environment','independent_child_write']),'prepared state validation missing')
                if contract is not None:
                    require(sample.get('clock_rng_resynchronised') is True,'guest clock/RNG maintenance missing')
                    notice=sample['restored_notice'];require(notice['entropy_bytes']==64,'restore entropy size differs')
                    if row['engine']=='hypermachine':require(notice.get('acknowledged_via_create') is True,'HM restore notice unverified')
                    else:
                        require(notice.get('acknowledged') is True,'FC restore notice not acknowledged')
                        require(isinstance(notice['unix_time_ns'],int) and not isinstance(notice['unix_time_ns'],bool) and 0<notice['unix_time_ns']<2**64,'invalid restored clock time')
                        require(isinstance(notice['attempts'],int) and not isinstance(notice['attempts'],bool) and notice['attempts']>=1,'invalid restore notice attempt count')
                        sha=notice['entropy_sha256'];require(isinstance(sha,str) and len(sha)==64 and all(c in '0123456789abcdef' for c in sha),'invalid entropy fingerprint')
                        entropy_hashes.append(sha)
        if row['success']:
            require(all(s['success'] and s['cleanup_success'] for s in samples) and not row['cleanup_errors'] and 'memory_error' not in row,'successful batch inconsistent')
            require(row['memory_hold_seconds']==5 and number(row['memory_read_ms']) and row['memory_read_ms']>=0,'held measurement missing')
            memory=row['held_process_memory_kib'];baseline=row['empty_process_memory_baseline_kib'];incremental=row['incremental_process_memory_kib']
            require(memory.keys()==baseline.keys()==incremental.keys() and 'Pss_kib' in memory,'memory fields differ')
            require(all(number(v) and v>=0 for source in [memory,baseline] for v in source.values()),'invalid absolute memory')
            require(all(number(incremental[k]) and incremental[k]==v-baseline[k] for k,v in memory.items()),'baseline subtraction differs')
    result={'engines':{},'pairs':[],'artifacts_unchanged':True,'prepared_sources_unchanged':True,'cohort_success':report['success'],'latencies_conditional_on_success':True,'memory_conditional_on_complete_successful_batches':True,'runtime_change_adopted':False,'managed_competitor_win_established':False}
    for engine in ['hypermachine','firecracker']:
        runs=[r for r in report['runs'] if r['engine']==engine];samples=[s for r in runs for s in r['samples']];values=[s['ready_ms'] for s in samples if s['success'] and s['cleanup_success']];complete=[r for r in runs if r['success']]
        result['engines'][engine]={'planned':report['pairs']*report['concurrency'],'attempted':len(samples),'passed':len(values),'failed':len(samples)-len(values),'successful_p50_ms':percentile(values,.5),'successful_p95_ms':percentile(values,.95),'successful_p99_ms':percentile(values,.99),'median_held_pss_mib':statistics.median(r['held_process_memory_kib']['Pss_kib']/1024 for r in complete) if complete else None,'median_incremental_pss_mib':statistics.median(r['incremental_process_memory_kib']['Pss_kib']/1024 for r in complete) if complete else None}
    for pair in range(report['pairs']):
        runs=report['runs'][pair*2:pair*2+2];complete=all(r['success'] for r in runs);row={'pair':pair,'complete_successful_pair':complete,'hypermachine_mean_reduction_ms':None,'hypermachine_p99_reduction_ms':None,'hypermachine_held_pss_reduction_mib':None}
        if complete:
            by={r['engine']:r for r in runs};h,f=by['hypermachine'],by['firecracker'];hv=[s['ready_ms'] for s in h['samples']];fv=[s['ready_ms'] for s in f['samples']]
            row.update(hypermachine_mean_reduction_ms=statistics.mean(fv)-statistics.mean(hv),hypermachine_p99_reduction_ms=percentile(fv,.99)-percentile(hv,.99),hypermachine_held_pss_reduction_mib=(f['held_process_memory_kib']['Pss_kib']-h['held_process_memory_kib']['Pss_kib'])/1024)
        result['pairs'].append(row)
    result['cleanup_verified']=not report['cleanup_errors'] and report.get('remaining_sandboxes')==0 and report.get('owned_node_stopped') is True and report.get('owned_node_exit_code')==0 and all(s['cleanup_success'] for r in report['runs'] for s in r['samples'])
    if 'owned_firecracker_processes' in report:result['cleanup_verified']=result['cleanup_verified'] and bool(report['owned_firecracker_processes']) and all(isinstance(p['exit_code'],int) for p in report['owned_firecracker_processes'])
    result['hypermachine_faster_complete_pairs']=sum(p['hypermachine_mean_reduction_ms'] is not None and p['hypermachine_mean_reduction_ms']>0 for p in result['pairs'])
    if contract is not None:
        require(len(entropy_hashes)==len(set(entropy_hashes)),'restored children reused entropy')
        result['matched_guest_restore_contract']=True
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    require(not args.output.exists(),'output exists; preserve earlier analysis');args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text())),indent=2)+'\n')
