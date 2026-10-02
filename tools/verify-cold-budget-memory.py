#!/usr/bin/env python3
"""Verify frozen 16/32 cold-budget latency and held-memory evidence."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(root):
    root = root.resolve()
    manifest = json.loads((root/'manifest.json').read_text())
    hashes = manifest['sha256']
    for name, expected in hashes.items():
        path = (root/name).resolve()
        require(path.is_relative_to(root), 'manifest escapes archive')
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, 'hash mismatch: '+name)
    context = json.loads((root/'build-context.json').read_text())
    require(hashes['compiled-main.rs'] == context['compiled_overlays']['crates/hv2-sandboxd/src/main.rs'], 'compiled main differs')
    spec = importlib.util.spec_from_file_location('frozen_budget_memory', root/'analyze-cold-budget-memory.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    attempts, cohorts, cleanup = 0, 0, True
    for cohort in manifest['cohorts']:
        raw = json.loads((root/cohort['report']).read_text())
        result = module.analyze(raw)
        require(result == json.loads((root/cohort['analysis']).read_text()), 'recomputed analysis differs')
        require(raw['baseline_limit'] == 16 and raw['candidate_limit'] == 32 and raw['same_binary'], 'budget/binary isolation differs')
        require(raw['memory_idle_seconds'] == 5 and raw['guest_readiness_timeout_s'] == 15, 'hold/deadline differs')
        require(len(raw['driver_cpu_affinity']) == 8 and len(set(raw['driver_cpu_affinity'])) == 8 and raw['added_CPU_load'] is False, 'CPU configuration differs')
        inputs = raw['artifact_sha256']
        require(inputs['baseline'] == inputs['candidate'] == context['daemon_sha256'], 'accepted daemon differs')
        require(all(inputs[name] == manifest['inputs'][name] for name in ['kernel','initrd']), 'guest image differs')
        require(all(inputs[name] in hashes.values() for name in ['harness','comparison','burst','shared','firecracker']), 'frozen coordinator missing')
        require(all(v['attempted'] == v['planned'] for v in result['variants'].values()), 'planned attempts missing')
        require(result['artifacts_unchanged'] is True, 'inputs changed during cohort')
        require(result['runtime_change_adopted'] is False and result['competitor_win_established'] is False, 'unmeasured adoption/competitor claim')
        require(raw['concurrency'] == cohort['concurrency'] and raw['pairs'] == cohort['pairs'], 'planned profile differs')
        attempts += sum(v['attempted'] for v in result['variants'].values())
        cleanup = cleanup and result['cleanup_verified']
        cohorts += 1
    return {'verified_files':len(hashes), 'cohorts':cohorts, 'attempts':attempts, 'cleanup_verified':cleanup, 'runtime_change_adopted':False, 'competitor_win_established':False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.archive),indent=2))
