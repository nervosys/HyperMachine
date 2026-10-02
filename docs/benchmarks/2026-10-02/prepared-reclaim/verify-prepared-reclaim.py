#!/usr/bin/env python3
"""Verify frozen prepared-source heap reclaim evidence and helper build binding."""
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
    build=json.loads((root/'build-context.json').read_text());helper=json.loads((root/'helper-build.json').read_text())
    report=json.loads((root/'report.json').read_text())
    require(build['daemon_sha256']==report['artifact_sha256']['hypermachine'] and build['compiled_overlays']['crates/hv2-sandboxd/src/main.rs']==hashes['compiled-main.rs'],'accepted daemon binding differs')
    require(helper['source_sha256']==hashes['heap-reclaim-probe.c'] and helper['binary_sha256']==report['artifact_sha256']['helper'] and helper['identical_rebuild'] is True,'helper source/binary binding differs')
    require(len(report['cpu_affinity'])==8 and len(set(report['cpu_affinity']))==8,'CPU affinity differs')
    names={'driver':'diagnose-prepared-reclaim.py','coordinator':'bench-prepared-engines.py','mappings':'prepared-memory-mappings.py','engines':'bench-local-engines.py','firecracker_harness':'bench-firecracker-local.py','helper_source':'heap-reclaim-probe.c'}
    require(all(report['artifact_sha256'][key]==hashes[name] for key,name in names.items()),'frozen driver/source differs')
    require(all(report['artifact_sha256'][key]==value for key,value in manifest['inputs'].items()),'binary/image differs')
    spec=importlib.util.spec_from_file_location('reclaim_analysis',root/'analyze-prepared-reclaim.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    result=module.analyze(report,root)
    require(result==json.loads((root/'analysis.json').read_text()),'recomputed result differs')
    return {'verified_files':len(hashes),'diagnostic_attempts':result['attempts'],'cleanup_verified':result['cleanup_verified'],'performance_win_established':False,'runtime_change_adopted':False}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('archive',type=Path);args=parser.parse_args();print(json.dumps(verify(args.archive),indent=2))
