#!/usr/bin/env python3
"""Exercise owned prepared-restore failures and recovery outside ranking runs."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import sys
import threading
import time


def require(value,message):
    if not value:raise ValueError(message)


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['candidate','candidate-context','firecracker','kernel','initrd','output']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();require(not args.output.exists(),'output exists; preserve earlier diagnostic')
    warm=load('admission_recovery_warm',Path(__file__).with_name('bench-prepared-engines.py'))
    context=json.loads(args.candidate_context.read_text());args.candidate=args.candidate.resolve(strict=True)
    require(context['limit']==16 and context['candidate_sha256']==warm.engines.digest(args.candidate),'requires source-bound sixteen-slot candidate')
    paths={'driver':Path(__file__).resolve(),'coordinator':Path(warm.__file__).resolve(),'engines':Path(warm.engines.__file__).resolve(),'firecracker_harness':Path(warm.fc.__file__).resolve(),'candidate_context':args.candidate_context.resolve(strict=True),'candidate':args.candidate}
    hashes={k:warm.engines.digest(p) for k,p in paths.items()}
    report={'diagnostic_only':True,'runtime_change_adopted':False,'performance_win_established':False,'artifact_sha256':hashes,'restore_slots':16,'fault_cycles':[],'success':False,'failure_kind':'owned named snapshot file temporarily withheld after preparation','limitations':['Failure/recovery diagnostic, not performance ranking','Exercises launch errors after admission; queued-request cancellation and fleet behavior remain untested']}
    args.output.parent.mkdir(parents=True,exist_ok=True);prepared=args.output.parent/'prepared-report.json';require(not prepared.exists(),'prepared report exists')
    original_popen=warm.subprocess.Popen;original_batch=warm.batch;original_argv=sys.argv;store=None
    def save():args.output.write_text(json.dumps(report,indent=2)+'\n')
    def popen(argv,**kwargs):
        nonlocal store
        if Path(argv[0]).resolve()==args.candidate:store=Path(argv[argv.index('--snapshot-store')+1]).resolve()
        return original_popen(argv,**kwargs)
    def batch(*positional,**keywords):
        arguments,engine,pair,url,*rest=positional
        if engine=='hypermachine':
            require(store is not None,'owned snapshot store absent')
            sources=list((store/'snapshots').glob('warm-benchmark-*.snap'));require(len(sources)==1,'named snapshot ambiguous')
            source=sources[0].resolve();require(source.is_relative_to(store),'snapshot escaped owned store')
            withheld=source.with_suffix('.withheld');require(not withheld.exists(),'withheld path exists')
            before=warm.engines.digest(source);cycle={'pair':pair,'attempted':32,'samples':[],'source_sha256':before,'success':False}
            report['fault_cycles'].append(cycle);save();barrier=threading.Barrier(32)
            def fail(index):
                barrier.wait();started=time.perf_counter();row={'index':index,'expected_rejection':False}
                try:
                    created=warm.engines.request(url,'POST','/v2/sandboxes',{'templateID':'warm-benchmark','timeout':300,'allowInternetAccess':False})
                    row['unexpected_sandbox_id']=created.get('sandboxID')
                    if row['unexpected_sandbox_id']:warm.engines.request(url,'DELETE',f"/sandboxes/{row['unexpected_sandbox_id']}")
                except Exception as error:
                    row['error']=str(error);row['expected_rejection']=str(error).startswith('HTTP 500:') and 'launching:' in str(error)
                row['elapsed_ms']=(time.perf_counter()-started)*1000;return row
            source.rename(withheld)
            try:
                with ThreadPoolExecutor(max_workers=32) as pool:cycle['samples']=list(pool.map(fail,range(32)))
            finally:withheld.rename(source)
            cycle['source_restored']=warm.engines.digest(source)==before
            cycle['remaining_sandboxes']=len(warm.engines.request(url,'GET','/sandboxes'))
            cycle['success']=cycle['source_restored'] and cycle['remaining_sandboxes']==0 and all(s['expected_rejection'] for s in cycle['samples']);save()
            require(cycle['success'],'failure cycle did not reject/recover cleanly')
        row=original_batch(*positional,**keywords)
        if engine=='hypermachine':report['fault_cycles'][-1]['recovery_success']=row['success'];save()
        return row
    code=1
    try:
        warm.subprocess.Popen=popen;warm.batch=batch
        sys.argv=['bench-prepared-engines.py','--readiness-diagnostics','--pairs','2','--concurrency','32','--hypermachine',str(args.candidate),'--output',str(prepared)]
        for name in ['firecracker','kernel','initrd']:sys.argv+=['--'+name,str(getattr(args,name))]
        code=warm.main()
    except Exception as error:report['error']=str(error)
    finally:
        warm.subprocess.Popen=original_popen;warm.batch=original_batch;sys.argv=original_argv
        if prepared.exists():report['prepared_report']=json.loads(prepared.read_text())
        report['artifacts_unchanged']=all(warm.engines.digest(p)==hashes[k] for k,p in paths.items())
        report['success']=code==0 and report['artifacts_unchanged'] and len(report['fault_cycles'])==2 and all(c['success'] and c.get('recovery_success') is True for c in report['fault_cycles']) and report.get('prepared_report',{}).get('success') is True
        save()
    return 0 if report['success'] else 1


if __name__=='__main__':raise SystemExit(main())
