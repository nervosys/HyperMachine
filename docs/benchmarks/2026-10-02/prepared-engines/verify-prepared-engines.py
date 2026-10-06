#!/usr/bin/env python3
"""Verify frozen prepared-engine cohorts, including unscored setup failures."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(value, message):
    if not value:
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
    require(hashes['compiled-main.rs'] == context['compiled_overlays']['crates/hv2-sandboxd/src/main.rs'], 'compiled source differs')
    require(context['daemon_sha256'] == manifest['inputs']['hypermachine'], 'accepted daemon differs')
    spec = importlib.util.spec_from_file_location('prepared_analysis', root/'analyze-prepared-engines.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    attempts, unattempted, scored_cleanup = 0, 0, True
    for cohort in manifest['cohorts']:
        raw = json.loads((root/cohort['report']).read_text())
        require(raw['concurrency'] == cohort['concurrency'] and raw['pairs'] == cohort['pairs'], 'profile differs')
        require(len(raw['driver_cpu_affinity']) == 8 and len(set(raw['driver_cpu_affinity'])) == 8, 'CPU affinity differs')
        require(raw['artifacts_unchanged'] is True, 'cohort inputs changed')
        require(all(raw['artifact_sha256'][k] == v for k,v in manifest['inputs'].items()), 'binary/image differs')
        for key, name in [('coordinator',cohort['coordinator']),('engines','bench-local-engines.py'),('firecracker_harness','bench-firecracker-local.py')]:
            require(raw['artifact_sha256'][key] == hashes[name], 'frozen driver differs: '+key)
        if cohort['setup_failed']:
            require(raw['success'] is False and raw.get('setup_error') and raw['runs'] == [], 'setup failure contains scored attempts')
            require(raw.get('owned_node_stopped') is True and raw.get('owned_node_exit_code') == 0, 'failed setup node not stopped')
            require(raw.get('remaining_sandboxes') == cohort['reported_remaining_sandboxes'], 'failed setup inventory differs')
            unattempted += raw['pairs']*raw['concurrency']*2
        else:
            result = module.analyze(raw)
            require(result == json.loads((root/cohort['analysis']).read_text()), 'recomputed analysis differs')
            require(all(v['attempted'] == v['planned'] for v in result['engines'].values()), 'missing attempts')
            attempts += sum(v['attempted'] for v in result['engines'].values())
            scored_cleanup = scored_cleanup and result['cleanup_verified']
    return {'verified_files':len(hashes), 'scored_attempts':attempts, 'setup_failures':sum(c['setup_failed'] for c in manifest['cohorts']), 'unattempted_due_to_setup':unattempted, 'scored_cleanup_verified':scored_cleanup, 'first_failed_setup_empty_inventory_verified':False, 'runtime_change_adopted':False, 'managed_competitor_win_established':False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    print(json.dumps(verify(parser.parse_args().archive), indent=2))
