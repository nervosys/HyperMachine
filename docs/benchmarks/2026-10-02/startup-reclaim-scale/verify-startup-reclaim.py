#!/usr/bin/env python3
"""Verify source-bound startup-trim benchmark artifacts and all attempts."""
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
        path=(root/name).resolve();require(path.is_relative_to(root),'archive path escapes')
        require(hashlib.sha256(path.read_bytes()).hexdigest()==sha,'hash mismatch: '+name)
    build=json.loads((root/'build-context.json').read_text());source=json.loads((root/'source-context.json').read_text())
    require(source['only_changed_file']=='crates/hv2-sandboxd/src/main.rs' and source['provisional_core_excluded'] is True,'source isolation differs')
    require(source['accepted_source_sha256']['crates/hv2-sandboxd/src/main.rs']==hashes['accepted-main.rs'] and source['candidate_main_sha256']==hashes['candidate-main.rs'],'compiled main binding differs')
    require(all(source['accepted_source_sha256'][name]==sha for name,sha in build['clean_boot_sha256'].items()),'clean core source differs')
    require(build['candidate_main_sha256']==hashes['candidate-main.rs'] and build['build_exit_code']==0,'candidate build binding differs')
    for archive_name,source_name in manifest.get('accepted_source_bindings',{}).items():
        require(hashes[archive_name]==source['accepted_source_sha256'][source_name],'accepted source binding differs: '+archive_name)
    spec=importlib.util.spec_from_file_location('startup_generator',root/'experiment-startup-reclaim.py');generator=importlib.util.module_from_spec(spec);spec.loader.exec_module(generator)
    accepted=(root/'accepted-main.rs').read_bytes();require(hashlib.sha256(accepted).hexdigest()==generator.BASE_SHA256,'generator base differs')
    require(accepted.decode().replace(generator.ANCHOR,generator.INSERT+generator.ANCHOR).encode()==(root/'candidate-main.rs').read_bytes(),'candidate differs from generated experiment')
    spec=importlib.util.spec_from_file_location('startup_analysis',root/'analyze-startup-reclaim.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    comments=json.loads((root/'compiler-comments.json').read_text())
    require(comments['same_compiler_comment'] is True and comments['baseline']==comments['candidate'],'compiler identity differs')
    attempts=0;passed=0;cleanup=True;all_pairs=[]
    for cohort in manifest['cohorts']:
        raw=json.loads((root/cohort['report']).read_text());result=module.analyze(raw,root)
        require(result==json.loads((root/cohort['analysis']).read_text()),'recomputed analysis differs')
        require(raw['pairs']==cohort['pairs'] and raw['concurrency']==cohort['concurrency'],'profile differs')
        require(len(raw['cpu_affinity'])==8 and len(set(raw['cpu_affinity']))==8,'CPU configuration differs')
        require(raw['artifact_sha256']['baseline']==build['baseline_sha256'] and raw['artifact_sha256']['candidate']==build['candidate_sha256'],'build binaries differ')
        require(all(raw['artifact_sha256'][key]==sha for key,sha in manifest['inputs'].items()),'binary/image differs')
        for key,name in [('driver','bench-startup-reclaim.py'),('coordinator','bench-prepared-engines.py'),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py')]:require(raw['artifact_sha256'][key]==hashes[name],'driver differs: '+key)
        attempts+=sum(v['planned']+v['firecracker_control_planned'] for v in result['variants'].values());cleanup=cleanup and result['cleanup_verified']
        passed+=sum(v['passed']+v['firecracker_control_passed'] for v in result['variants'].values())
        all_pairs.extend(result['pairs'])
    output={'verified_files':len(hashes),'planned_attempts':attempts,'passed_attempts':passed,'failed_attempts':attempts-passed,'cleanup_verified':cleanup,'runtime_change_adopted':False,'managed_competitor_win_established':False}
    if manifest.get('adoption_rejected') is True:
        require(all_pairs and all(p['complete'] and p['mean_reduction_ms']<0 and p['p99_reduction_ms']<0 for p in all_pairs),'scale rejection is not supported by all paired means/tails')
        output['candidate_adoption_rejected']=True
    return output


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args();print(json.dumps(verify(args.archive),indent=2))
