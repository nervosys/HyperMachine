#!/usr/bin/env python3
"""Verify frozen automatic-worker functional evidence, not performance."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'docs/benchmarks/2026-10-01'
m = json.loads((root / 'scheduled-worker-manifest.json').read_text())
raw = (root / 'scheduled-worker-run-1.json').read_bytes()
assert hashlib.sha256(raw).hexdigest() == m['report_sha256']
assert hashlib.sha256((root / 'scheduled-worker-core-source.patch').read_bytes()).hexdigest() == m['core_source_patch_sha256']
assert hashlib.sha256((root / 'scheduled-worker-coordinator.py').read_bytes()).hexdigest() == m['artifact_sha256']['coordinator']
r = json.loads(raw)
assert r['artifact_sha256'] == m['artifact_sha256']
assert r['success'] and not r['cleanup_errors'] and r['remaining_sandboxes'] == 0
assert len(r['cases']) == m['verified_cases'] == 16
assert all(c['success'] for c in r['cases'])
assert len(r['owned_processes_stopped']) == m['registered_processes_stopped'] == 22
c = r['cases'][0]
assert c['name'] == 'scheduled-VM-worker-TLS-resume-restart-and-no-replay'
assert c['result']['guest_exit_code'] == 7
assert all(c['result'][k] is True for k in [
    'paused_guest_resumed', 'literal_environment_preserved', 'durable_output_recovered',
    'duplicate_guest_execution_refused', 'history_survives_cancellation',
    'automatic_worker', 'restart_continues_next_occurrence'])
print('Verified frozen 16-case worker run and cleanup; no performance comparison.')
