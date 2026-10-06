#!/usr/bin/env python3
"""Verify frozen readiness snapshots and same-kernel symbol query evidence."""
import argparse
import collections
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(root):
    root=root.resolve();manifest=json.loads((root/'manifest.json').read_text())
    for name,sha in manifest['sha256'].items():
        p=(root/name).resolve();require(p.is_relative_to(root),'manifest escapes archive')
        require(hashlib.sha256(p.read_bytes()).hexdigest()==sha,'hash mismatch: '+name)
    context=json.loads((root/'build-context.json').read_text())
    require(context['compiled_agent_sha256']==manifest['sha256']['guest_agent.rs.txt'] and context['windows_overlay_matches'] is True and context['provisional_core_excluded'] is True and context['runtime_logic_changed'] is False,'test source context differs')
    spec=importlib.util.spec_from_file_location('snapshots',root/'diagnose-readiness-failures.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    report=json.loads((root/'report.json').read_text());failure=json.loads((root/'failure-report.json').read_text())
    require(report['diagnostic_only'] is True and report['performance_comparison'] is False,'diagnostic scope differs')
    require(report['success'] and report['artifacts_unchanged'] and report['sample']['success'] and report['sample']['cleanup_success'],'probe or cleanup failed')
    require(report['artifact_sha256']['failure_report']==manifest['sha256']['failure-report.json'],'retained cohort differs')
    require(report['artifact_sha256']['coordinator']==manifest['sha256']['diagnose-readiness-failures.py'] and report['artifact_sha256']['harness']==manifest['sha256']['bench-firecracker-local.py'],'frozen probe differs')
    require(all(report['artifact_sha256'][n]==failure['artifact_sha256'][n] for n in ['kernel','initrd']),'guest images differ')
    rows=module.snapshots(failure)
    require(rows==report['retained_snapshots'],'snapshot interpretation differs')
    maps={a:module.mapping(a,v['response']) for a,v in report['symbol_queries'].items()}
    require(maps==report['mappings'] and set(maps)=={r['rip'] for r in rows},'symbol interpretation differs')
    counts=collections.Counter((maps[r['rip']]['symbol'],r['state']) for r in rows)
    require(report['state_counts']==[{'symbol':k[0],'state':k[1],'count':v} for k,v in sorted(counts.items())],'state totals differ')
    return {'verified_files':len(manifest['sha256']),'failed_snapshots':len(rows),'mapped_addresses':len(maps),'probe_cleanup_verified':True,'performance_comparison':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args()
    print(json.dumps(verify(args.archive),indent=2))
