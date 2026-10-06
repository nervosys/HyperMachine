#!/usr/bin/env python3
"""Compare accepted and startup-trim daemons in counterbalanced fresh cohorts."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys

spec=importlib.util.spec_from_file_location('startup_prepared',Path(__file__).with_name('bench-prepared-engines.py'))
warm=importlib.util.module_from_spec(spec);spec.loader.exec_module(warm)


def require(value,message):
    if not value:raise ValueError(message)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','candidate','firecracker','kernel','initrd','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--pairs',type=int,default=6);parser.add_argument('--concurrency',type=int,default=8);args=parser.parse_args()
    require(1<=args.pairs<=12 and 1<=args.concurrency<=100,'invalid profile')
    require(not args.output.exists(),'output exists; preserve previous evidence')
    paths={name:getattr(args,name).resolve(strict=True) for name in ['baseline','candidate','firecracker','kernel','initrd']}
    for name,path in paths.items():setattr(args,name,path)
    paths.update(driver=Path(__file__).resolve(),coordinator=Path(warm.__file__).resolve(),engines=Path(warm.engines.__file__).resolve(),firecracker_harness=Path(warm.fc.__file__).resolve())
    hashes={name:warm.engines.digest(path) for name,path in paths.items()}
    require(hashes['baseline']!=hashes['candidate'],'requires distinct accepted/candidate binaries')
    affinity=sorted(os.sched_getaffinity(0))[:8];require(len(affinity)==8,'requires eight CPUs');os.sched_setaffinity(0,affinity)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    report={'pairs':args.pairs,'concurrency':args.concurrency,'cpu_affinity':affinity,'artifact_sha256':hashes,'runs':[],'success':False,'runtime_change_adopted':False,'managed_competitor_win_established':False,'order':'fresh-daemon baseline/candidate AB/BA with internal HM/FC AB/BA','limitations':['Shared WSL/KVM host with uncontrolled background load','Fresh prepared sources and daemon per variant; preparation excluded from restore timing','Two internal engine pairs per variant; nested batches are not independent hosts','One-time trim is after initial template construction, before named-source preparation','No allocator helper preload; distinct source-bound binaries']}
    original_argv=sys.argv
    try:
        for pair in range(args.pairs):
            for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
                path=args.output.parent/f'{pair}-{variant}.json';row={'pair':pair,'variant':variant,'success':False}
                sys.argv=['bench-prepared-engines.py','--pairs','2','--concurrency',str(args.concurrency),'--hypermachine',str(getattr(args,variant)),'--output',str(path)]
                for name in ['firecracker','kernel','initrd']:sys.argv += ['--'+name,str(getattr(args,name))]
                try:
                    code=warm.main()
                    if path.exists():row['prepared_report']=json.loads(path.read_text())
                    row['success']=code==0 and row.get('prepared_report',{}).get('success') is True
                except Exception as error:row['error']=str(error)
                report['runs'].append(row);args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'pair':pair,'variant':variant,'success':row['success']}),flush=True)
    finally:sys.argv=original_argv
    report['artifacts_unchanged']=all(warm.engines.digest(path)==hashes[name] for name,path in paths.items())
    report['success']=report['artifacts_unchanged'] and all(r['success'] for r in report['runs'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    return 0 if report['success'] else 1


if __name__=='__main__':raise SystemExit(main())
