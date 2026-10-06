#!/usr/bin/env python3
"""Validate and summarize owned prepared-guest mapping diagnostics."""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics


def load(name, path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(value,message):
    if not value:raise ValueError(message)


def analyze(report, directory):
    require(report.get('diagnostic_only') is True,'requires diagnostic cohort')
    checked=load('prepared',directory/'analyze-prepared-engines.py').analyze(report)
    require(checked['cohort_success'] and checked['cleanup_verified'],'diagnostic attempts or cleanup failed')
    parser=load('mapping_parser',directory/'prepared-memory-mappings.py')
    rows=[]
    for batch in report['runs']:
        engine=batch['engine'];observations=batch['held_mapping_observations']
        require(len(observations)==(1 if engine=='hypermachine' else report['concurrency']),'held process count differs')
        require(len({o['pid'] for o in observations})==len(observations),'duplicate process observations')
        baseline=batch.get('empty_mapping_observations',[])
        require(len(baseline)==(1 if engine=='hypermachine' else 0),'baseline observations differ')
        for observation in observations+baseline:
            parsed=parser.parse(observation['raw_smaps'])
            require(all(observation[k]==v for k,v in parsed.items()),'mapping summary differs from raw smaps')
            require(observation['read_duration_ns']>=0,'negative diagnostic read time')
        guest_maps=[m for o in observations for m in o['guest_sized_mappings']]
        require(len(guest_maps)==report['concurrency'],'guest-sized mapping count differs')
        require(all(m['Size_kib']==1048576 and m['permissions']=='rw-p' and m['path'] and not m['path'].startswith('[') for m in guest_maps),'guest-sized maps not matching private file images')
        # Size/path identify candidate guest mappings; they do not prove ownership.
        groups={}
        for o in observations:
            for category,values in o['groups'].items():
                group=groups.setdefault(category,{k:0 for k in values})
                for k,v in values.items():group[k]+=v
        total=sum(g['Pss_kib'] for g in groups.values())
        guest=groups['guest_sized']['Pss_kib']
        rows.append({'engine':engine,'pair':batch['pair'],'mapping_pss_mib':total/1024,'guest_sized_pss_mib':guest/1024,'guest_sized_private_dirty_mib':sum(m['Private_Dirty_kib'] for m in guest_maps)/1024,'other_mapping_pss_mib':(total-guest)/1024,'empty_mapping_pss_mib':sum(g['Pss_kib'] for o in baseline for g in o['groups'].values())/1024,'groups':groups})
    summary={}
    for engine in ['hypermachine','firecracker']:
        values=[r for r in rows if r['engine']==engine]
        summary[engine]={k:statistics.median(r[k] for r in values) for k in ['mapping_pss_mib','guest_sized_pss_mib','guest_sized_private_dirty_mib','other_mapping_pss_mib','empty_mapping_pss_mib']}
    paired_gaps=[]
    for pair in range(report['pairs']):
        by={r['engine']:r for r in rows if r['pair']==pair}
        hm,fc=by['hypermachine'],by['firecracker']
        categories=set(hm['groups'])|set(fc['groups'])
        gaps={k:(hm['groups'].get(k,{}).get('Pss_kib',0)-fc['groups'].get(k,{}).get('Pss_kib',0))/1024 for k in sorted(categories)}
        total=hm['mapping_pss_mib']-fc['mapping_pss_mib']
        require(abs(sum(gaps.values())-total)<1e-9,'mapping category gaps do not sum to total')
        paired_gaps.append({'pair':pair,'hypermachine_minus_firecracker_mapping_pss_mib':total,
                            'category_gaps_mib':gaps})
    return {'diagnostic_only':True,'performance_win_established':False,'runtime_change_adopted':False,'attempts':sum(v['attempted'] for v in checked['engines'].values()),'cleanup_verified':checked['cleanup_verified'],'engines':summary,'batches':rows,'paired_mapping_gaps':paired_gaps,'limitations':['Sequential procfs reads are not atomic and can differ from smaps_rollup','Guest-sized file mappings are candidates identified by size/path, not proven ownership','Private dirty pages include restore writes and guest activity; this does not isolate their causes','Mapping reads extend guest lifetime and cannot be pooled into benchmark latency cohorts']}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args()
    require(not args.output.exists(),'output exists; preserve earlier analysis')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
