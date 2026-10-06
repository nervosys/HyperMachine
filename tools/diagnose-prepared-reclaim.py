#!/usr/bin/env python3
"""Counterbalance one-time prepared-source heap reclaim in owned daemons."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import socket
import sys
import time


def load(name, path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module


warm=load('prepared_driver',Path(__file__).with_name('bench-prepared-engines.py'))
maps=load('prepared_maps',Path(__file__).with_name('prepared-memory-mappings.py'))


def require(value,message):
    if not value:raise ValueError(message)


def run(args, variant, pair, directory):
    parent,child=socket.socketpair();parent.settimeout(15);stream=parent.makefile('rb')
    original_popen=warm.subprocess.Popen;original_batch=warm.batch;original_argv=sys.argv
    node=None;observations=[];operations=[]
    def popen(argv, **kwargs):
        nonlocal node
        if Path(argv[0]).resolve()==args.hypermachine:
            kwargs['env']={**kwargs['env'],'LD_PRELOAD':str(args.helper),'HM_MEMORY_PROBE_FD':str(child.fileno())}
            kwargs['pass_fds']=(child.fileno(),)
            node=original_popen(argv,**kwargs)
            child.close()
            return node
        return original_popen(argv,**kwargs)
    def batch(*positional, **keywords):
        if not operations:
            require(node is not None and node.poll() is None,'owned node unavailable for probe')
            time.sleep(5)
            observations.append({'stage':'prepared-before',**maps.observe(node.pid)})
            parent.sendall(b'T' if variant=='trim' else b'N')
            reply=json.loads(stream.readline(256))
            require(reply['operation']==variant and reply['result'] in [0,1] and reply['duration_ns']>=0,'invalid probe reply')
            operations.append(reply)
            time.sleep(5)
            observations.append({'stage':'prepared-after',**maps.observe(node.pid)})
        return original_batch(*positional,**keywords)
    path=directory/f'{pair}-{variant}.json'
    error=None
    try:
        warm.subprocess.Popen=popen;warm.batch=batch
        sys.argv=['bench-prepared-engines.py','--mapping-diagnostics','--pairs','2','--concurrency',str(args.concurrency),'--output',str(path)]
        for name in ['hypermachine','firecracker','kernel','initrd']:sys.argv += ['--'+name,str(getattr(args,name))]
        code=warm.main()
    except Exception as exception:
        code=1;error=str(exception)
    finally:
        warm.subprocess.Popen=original_popen;warm.batch=original_batch;sys.argv=original_argv
        if node is not None and node.poll() is None:warm.engines.stop(node)
        stream.close();parent.close();child.close()
    result={'variant':variant,'pair':pair,'observations':observations,'operations':operations,'success':False}
    if path.exists():result['prepared_report']=json.loads(path.read_text())
    if error:result['error']=error
    result['success']=code==0 and len(operations)==1 and len(observations)==2 and result.get('prepared_report',{}).get('success') is True
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['hypermachine','firecracker','kernel','initrd','helper','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--pairs',type=int,default=2);parser.add_argument('--concurrency',type=int,default=8);args=parser.parse_args()
    require(1<=args.pairs<=4 and 1<=args.concurrency<=8,'invalid bounded probe profile')
    require(not args.output.exists(),'output exists; preserve earlier evidence')
    paths={name:getattr(args,name).resolve(strict=True) for name in ['hypermachine','firecracker','kernel','initrd','helper']}
    for name,path in paths.items():setattr(args,name,path)
    paths.update(driver=Path(__file__).resolve(),coordinator=Path(warm.__file__).resolve(),mappings=Path(maps.__file__).resolve(),engines=Path(warm.engines.__file__).resolve(),firecracker_harness=Path(warm.fc.__file__).resolve(),helper_source=Path(__file__).with_name('heap-reclaim-probe.c').resolve())
    hashes={name:warm.engines.digest(path) for name,path in paths.items()}
    affinity=sorted(os.sched_getaffinity(0))[:8];require(len(affinity)==8,'requires eight available CPUs');os.sched_setaffinity(0,affinity)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    report={'diagnostic_only':True,'performance_win_established':False,'runtime_change_adopted':False,'pairs':args.pairs,'concurrency':args.concurrency,'artifact_sha256':hashes,'cpu_affinity':affinity,'order':'fresh-daemon sham/trim AB/BA','runs':[],'success':False}
    for pair in range(args.pairs):
        for variant in (['sham','trim'] if pair%2==0 else ['trim','sham']):
            row=run(args,variant,pair,args.output.parent);report['runs'].append(row)
            args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'pair':pair,'variant':variant,'success':row['success']}),flush=True)
    report['artifacts_unchanged']=all(warm.engines.digest(path)==hashes[name] for name,path in paths.items())
    report['success']=report['artifacts_unchanged'] and all(r['success'] for r in report['runs'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    return 0 if report['success'] else 1


if __name__=='__main__':raise SystemExit(main())
