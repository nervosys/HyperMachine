#!/usr/bin/env python3
"""Validate sham-controlled prepared-source heap reclaim diagnostics."""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def require(value,message):
    if not value:raise ValueError(message)


def analyze(report,directory):
    require(report['diagnostic_only'] is True and report['performance_win_established'] is False and report['runtime_change_adopted'] is False,'diagnostic scope differs')
    require(report['success'] is True and report['artifacts_unchanged'] is True,'probe failed or inputs changed')
    require(len(report['runs'])==report['pairs']*2,'planned probe missing')
    memory=load('prepared_reclaim_memory',directory/'analyze-prepared-memory.py');maps=load('prepared_reclaim_maps',directory/'prepared-memory-mappings.py')
    rows=[];attempts=0
    for index,row in enumerate(report['runs']):
        order=['sham','trim'] if (index//2)%2==0 else ['trim','sham']
        require(row['pair']==index//2 and row['variant']==order[index%2] and row['success'] is True,'probe counterbalance or success differs')
        require(len(row['operations'])==1 and len(row['observations'])==2,'one-time operation differs')
        operation=row['operations'][0];require(operation['operation']==row['variant'] and operation['result'] in [0,1] and operation['duration_ns']>=0,'operation reply differs')
        before,after=row['observations']
        require(before['stage']=='prepared-before' and after['stage']=='prepared-after' and before['pid']==after['pid'],'observation identity differs')
        for value in [before,after]:
            parsed=maps.parse(value['raw_smaps']);require(all(value[k]==v for k,v in parsed.items()),'raw mapping interpretation differs')
            require(not value['guest_sized_mappings'],'source parent still mapped during intervention')
        raw=row['prepared_report'];require(raw['pairs']==2 and raw['concurrency']==report['concurrency'],'restored profile differs')
        result=memory.analyze(raw,directory);attempts+=result['attempts']
        require(all(raw['artifact_sha256'][k]==report['artifact_sha256'][k] for k in ['hypermachine','firecracker','kernel','initrd','coordinator','engines','firecracker_harness']),'nested inputs differ')
        require(raw['artifact_sha256']['mapping_diagnostics']==report['artifact_sha256']['mappings'],'mapping helper differs')
        total=lambda o:sum(g['Pss_kib'] for g in o['groups'].values())/1024
        heap=lambda o:o['groups'].get('heap',{}).get('Pss_kib',0)/1024
        held=[r['held_process_memory_kib']['Pss_kib']/1024 for r in raw['runs'] if r['engine']=='hypermachine']
        rows.append({'pair':row['pair'],'variant':row['variant'],'before_pss_mib':total(before),'after_pss_mib':total(after),'empty_pss_reduction_mib':total(before)-total(after),'heap_pss_reduction_mib':heap(before)-heap(after),'held_hypermachine_pss_mib':statistics.median(held),'operation_duration_ms':operation['duration_ns']/1000000})
    pairs=[]
    for pair in range(report['pairs']):
        by={r['variant']:r for r in rows if r['pair']==pair};sham,trim=by['sham'],by['trim']
        pairs.append({'pair':pair,'sham_adjusted_empty_reduction_mib':trim['empty_pss_reduction_mib']-sham['empty_pss_reduction_mib'],'sham_adjusted_heap_reduction_mib':trim['heap_pss_reduction_mib']-sham['heap_pss_reduction_mib'],'held_sham_minus_trim_mib':sham['held_hypermachine_pss_mib']-trim['held_hypermachine_pss_mib']})
    return {'diagnostic_only':True,'attempts':attempts,'cleanup_verified':True,'performance_win_established':False,'runtime_change_adopted':False,'runs':rows,'pairs':pairs,'limitations':['The same preload helper is used in sham and trim; neither is the uninstrumented production daemon','Fresh source preparation and host load vary between variants','One-time trim after named-source preparation does not test startup trim before serving','No scored latency, lifecycle stress or sustained allocation comparison; no runtime adoption']}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('report',type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args();require(not args.output.exists(),'output exists')
    args.output.write_text(json.dumps(analyze(json.loads(args.report.read_text()),Path(__file__).parent),indent=2)+'\n')
