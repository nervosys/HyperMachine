#!/usr/bin/env python3
"""Verify raw mappings, driver provenance and cleanup for prepared diagnostics."""
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
        path=(root/name).resolve();require(path.is_relative_to(root),'archive path escapes root')
        require(hashlib.sha256(path.read_bytes()).hexdigest()==sha,'hash mismatch: '+name)
    context=json.loads((root/'build-context.json').read_text())
    require(context['daemon_sha256']==manifest['inputs']['hypermachine'] and context['compiled_overlays']['crates/hv2-sandboxd/src/main.rs']==hashes['compiled-main.rs'],'accepted build differs')
    spec=importlib.util.spec_from_file_location('frozen_diagnostic',root/'analyze-prepared-memory.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    attempts=0
    for cohort in manifest['cohorts']:
        raw=json.loads((root/cohort['report']).read_text());result=module.analyze(raw,root)
        require(raw['pairs']==3 and raw['concurrency']==8,'diagnostic profile differs')
        require(result==json.loads((root/cohort['analysis']).read_text()),'raw mapping interpretation differs')
        require(all(raw['artifact_sha256'][k]==v for k,v in manifest['inputs'].items()),'binary/image differs')
        for key,name in [('coordinator','bench-prepared-engines.py'),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py'),('mapping_diagnostics','prepared-memory-mappings.py')]:
            require(raw['artifact_sha256'][key]==hashes[name],'frozen driver differs: '+key)
        require(result['cleanup_verified'] and raw['artifacts_unchanged'],'cleanup or unchanged inputs unverified')
        attempts+=result['attempts']
    return {'verified_files':len(hashes),'cohorts':len(manifest['cohorts']),'diagnostic_attempts':attempts,'cleanup_verified':True,'performance_win_established':False,'runtime_change_adopted':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args()
    print(json.dumps(verify(args.archive),indent=2))
