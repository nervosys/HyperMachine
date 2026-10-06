#!/usr/bin/env python3
"""Verify source-bound prepared admission comparisons and failure recovery."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(value,message):
    if not value:raise ValueError(message)


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def verify(root):
    root=root.resolve();manifest=json.loads((root/'manifest.json').read_text());hashes=manifest['sha256']
    for name,sha in hashes.items():
        path=(root/name).resolve();require(path.is_relative_to(root),'archive path escapes')
        require(hashlib.sha256(path.read_bytes()).hexdigest()==sha,'hash mismatch: '+name)
    build=json.loads((root/'build-context.json').read_text());source=json.loads((root/'source-context.json').read_text());accepted_build=json.loads((root/'accepted-build-context.json').read_text())
    require(source['only_changed_file']=='crates/hv2-sandboxd/src/main.rs' and source['provisional_core_excluded'] is True and source['accepted_source_files']==len(source['accepted_source_sha256'])==550,'source isolation differs')
    require(source['candidate_main_sha256']==build['candidate_main_sha256']==hashes['candidate-main.rs'] and build['limit']==source['limit']==16 and build['build_exit_code']==0,'candidate build binding differs')
    require(build['baseline_sha256']==accepted_build['daemon_sha256'],'accepted baseline differs')
    require(all(source['accepted_source_sha256'][name]==sha for name,sha in accepted_build['clean_boot_sha256'].items()),'clean core differs')
    for archive_name,source_name in manifest['accepted_source_bindings'].items():require(hashes[archive_name]==source['accepted_source_sha256'][source_name],'accepted source differs: '+archive_name)
    generator=load('admission_frozen_generator',root/'experiment-restore-admission.py');accepted=(root/'accepted-main.rs').read_bytes()
    require(hashlib.sha256(accepted).hexdigest()==generator.BASE_SHA256 and generator.render(accepted.decode(),16).encode()==(root/'candidate-main.rs').read_bytes(),'generated candidate differs')
    comments=json.loads((root/'compiler-comments.json').read_text());require(comments['same_compiler_comment'] is True and comments['baseline']==comments['candidate'],'compiler identity differs')
    analyzer=load('admission_frozen_analysis',root/'analyze-restore-admission.py');base=load('admission_frozen_base',root/'analyze-prepared-engines.py')
    attempted=passed=0;cleanup=True;entropy=[];pooled_tail_regressions=0
    for cohort in manifest['cohorts']:
        raw=json.loads((root/cohort['report']).read_text());result=analyzer.analyze(raw,root)
        require(result==json.loads((root/cohort['analysis']).read_text()),'recomputed comparison differs')
        require(raw['pairs']==cohort['pairs'] and raw['concurrency']==cohort['concurrency'],'paired profile differs')
        require(len(raw['cpu_affinity'])==len(set(raw['cpu_affinity']))==8,'CPU affinity differs')
        require(raw['intervention']['slots']==16 and raw['intervention']['candidate_main_sha256']==hashes['candidate-main.rs'],'intervention differs')
        require(raw['artifact_sha256']['baseline']==build['baseline_sha256'] and raw['artifact_sha256']['candidate']==build['candidate_sha256'],'variant binaries differ')
        require(raw['artifact_sha256']['candidate_context']==hashes['build-context.json'],'build context differs')
        for key,sha in manifest['inputs'].items():require(raw['artifact_sha256'][key]==sha,'image/control differs')
        for key,name in [('driver','bench-restore-admission.py'),('coordinator','bench-prepared-engines.py'),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py')]:require(raw['artifact_sha256'][key]==hashes[name],'frozen driver differs: '+key)
        for value in result['variants'].values():attempted+=value['planned']+value['firecracker_control_planned'];passed+=value['passed']+value['firecracker_control_passed']
        cleanup=cleanup and result['cleanup_verified']
        pooled_tail_regressions+=result['variants']['candidate']['successful_p99_ms']>result['variants']['baseline']['successful_p99_ms']
        for row in raw['runs']:
            entropy.extend(s['restored_notice']['entropy_sha256'] for run in row['prepared_report']['runs'] if run['engine']=='firecracker' for s in run['samples'] if s['success'])
    recovery=json.loads((root/'recovery-report.json').read_text());recovered=analyzer.analyze_recovery(recovery,root)
    require(recovered==json.loads((root/'recovery-analysis.json').read_text()),'recomputed recovery differs')
    require(recovery['artifact_sha256']['candidate']==build['candidate_sha256'] and recovery['artifact_sha256']['candidate_context']==hashes['build-context.json'],'recovery build differs')
    for key,name in [('driver','diagnose-restore-admission.py'),('coordinator','bench-prepared-engines.py'),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py')]:require(recovery['artifact_sha256'][key]==hashes[name],'recovery driver differs')
    for key,sha in manifest['inputs'].items():require(recovery['prepared_report']['artifact_sha256'][key]==sha,'recovery input differs')
    entropy.extend(s['restored_notice']['entropy_sha256'] for run in recovery['prepared_report']['runs'] if run['engine']=='firecracker' for s in run['samples'] if s['success'])
    require(len(entropy)==len(set(entropy)),'successful controls reused entropy across cohorts')
    require(manifest.get('adoption_rejected') is True and pooled_tail_regressions>=1,'tail-regression rejection unsupported')
    return {'verified_files':len(hashes),'paired_attempts':attempted,'paired_passed':passed,'paired_failed':attempted-passed,'cleanup_verified':cleanup and recovered['cleanup_verified'],'expected_launch_failures':recovered['observed_launch_rejections'],'post_fault_restores_passed':recovered['recovery_restore_passed'],'pooled_tail_regression_cohorts':pooled_tail_regressions,'candidate_adoption_rejected':True,'runtime_change_adopted':False,'managed_competitor_win_established':False,'request_cancellation_verified':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args();print(json.dumps(verify(args.archive),indent=2))
