#!/usr/bin/env python3
"""Verify matched clock/RNG restoration, raw attempts and source bindings."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(value,message):
    if not value:raise ValueError(message)


def verify(root):
    root=root.resolve();manifest=json.loads((root/'manifest.json').read_text());hashes=manifest['sha256']
    for name,sha in hashes.items():
        p=(root/name).resolve();require(p.is_relative_to(root),'archive escapes root')
        require(hashlib.sha256(p.read_bytes()).hexdigest()==sha,'hash mismatch: '+name)
    build=json.loads((root/'build-context.json').read_text());source=json.loads((root/'source-context.json').read_text())
    require(build['daemon_sha256']==manifest['inputs']['hypermachine'],'accepted daemon differs')
    require(build['compiled_overlays']['crates/hv2-sandboxd/src/main.rs']==hashes['accepted-main.rs'],'compiled main differs')
    for archive_name,source_name in manifest['accepted_source_bindings'].items():require(hashes[archive_name]==source['accepted_source_sha256'][source_name],'accepted source differs: '+archive_name)
    require(all(source['accepted_source_sha256'][name]==sha for name,sha in build['clean_boot_sha256'].items()),'clean core differs')
    spec=importlib.util.spec_from_file_location('contract_analysis',root/'analyze-prepared-engines.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    attempted=passed=0;cleanup=True
    entropy=[]
    for cohort in manifest['cohorts']:
        raw=json.loads((root/cohort['report']).read_text());result=module.analyze(raw)
        require(result.get('matched_guest_restore_contract') is True,'matched restore contract unverified')
        require(result==json.loads((root/cohort['analysis']).read_text()),'recomputed analysis differs')
        require(raw['pairs']==cohort['pairs'] and raw['concurrency']==cohort['concurrency'],'profile differs')
        require(len(raw['driver_cpu_affinity'])==8 and len(set(raw['driver_cpu_affinity']))==8,'CPU affinity differs')
        require(all(raw['artifact_sha256'][k]==sha for k,sha in manifest['inputs'].items()),'binary/image differs')
        for key,name in [('coordinator','bench-prepared-engines.py'),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py')]:require(raw['artifact_sha256'][key]==hashes[name],'frozen driver differs: '+key)
        for value in result['engines'].values():
            require(value['attempted']==value['planned'],'planned child missing');attempted+=value['attempted'];passed+=value['passed']
        entropy += [s['restored_notice']['entropy_sha256'] for b in raw['runs'] if b['engine']=='firecracker' for s in b['samples'] if s['success']]
        cleanup=cleanup and result['cleanup_verified']
    require(len(entropy)==len(set(entropy)),'successful controls reused entropy across cohorts')
    return {'verified_files':len(hashes),'attempted':attempted,'passed':passed,'failed':attempted-passed,'cleanup_verified':cleanup,'matched_guest_restore_contract':True,'runtime_change_adopted':False,'managed_competitor_win_established':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args();print(json.dumps(verify(args.archive),indent=2))
