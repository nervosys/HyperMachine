#!/usr/bin/env python3
"""Recompute the archived paired backup compression comparison."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import statistics


def require(value, message):
    if not value: raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    root = parser.parse_args().archive.resolve()
    manifest = json.loads((root / 'manifest.json').read_text())['sha256']
    for name, expected in manifest.items():
        path = (root / name).resolve()
        require(path.is_relative_to(root) and hashlib.sha256(path.read_bytes()).hexdigest() == expected, 'archive hash/path mismatch: ' + name)
    findings = {}
    catalogs = set()
    for cohort in ['paired', 'repeat']:
        report = json.loads((root / cohort / 'report.json').read_text())
        require(report['success'] and report['artifacts_unchanged'] and not report['managed_competitor_comparison'], 'comparison failed')
        require(report['tool_sha256'] == manifest['backup-snapshot-store.py']
                and report['coordinator_sha256'] == manifest['bench-backup-compression.py'], 'comparison provenance mismatch')
        require(report['expanded_bytes'] > 1024**3 and report['files'] > 0, 'workload missing')
        catalogs.add(report['catalog_sha256'])
        rows = report['rows']
        require([(r['pair'], r['level']) for r in rows] == [(pair, level) for pair in range(4) for level in ([1, 6] if pair % 2 == 0 else [6, 1])], 'AB/BA attempts missing')
        require(all(r['all_file_hashes_verified'] and math.isfinite(r['capture_seconds']) and r['capture_seconds'] > 0
                    and r['encrypted_bytes'] == r['compressed_bytes'] + 36 and r['compressed_bytes'] > 0 for r in rows), 'invalid/unverified measurements')
        computed = {str(level): {'median_capture_seconds': statistics.median(r['capture_seconds'] for r in rows if r['level'] == level),
                     'median_encrypted_bytes': statistics.median(r['encrypted_bytes'] for r in rows if r['level'] == level)} for level in [1, 6]}
        require(computed == report['summary'], 'summary differs from measured rows')
        findings[cohort] = computed
    require(len(catalogs) == 1, 'comparison source changed')
    report = json.loads((root / 'kvm-level6/report.json').read_text())
    require(report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors']
            and len(report['checks']) == 15 and all(p['exit_code'] == 0 for p in report['processes_stopped']), 'KVM recovery failed')
    require(report['artifact_sha256']['tool'] == manifest['backup-snapshot-store.py']
            and report['artifact_sha256']['coordinator'] == manifest['check-object-backup.py'], 'KVM provenance mismatch')
    require(all(value is True for name, value in report['kvm'].items() if name != 'backup_receipt'), 'KVM state recovery incomplete')
    receipt = report['kvm']['backup_receipt']
    require(receipt['compression_level'] == 6 and receipt['upload_method'] == 'multipart' and receipt['parts'] >= 2, 'level6 multipart missing')
    regression = json.loads((root / 'default-regression/report.json').read_text())
    require(regression['success'] and regression['artifacts_unchanged'] and regression['emulator_exit_code'] == 0
            and len(regression['checks']) == 9 and all(c['passed'] for c in regression['checks']), 'default multipart regression failed')
    require(regression['tool_sha256'] == manifest['backup-snapshot-store.py']
            and regression['coordinator_sha256'] == manifest['check-multipart-backup.py']
            and regression['checks'][0]['receipt']['compression_level'] == 1, 'default regression provenance changed')
    print(json.dumps({'success': True, 'hashed_files': len(manifest), 'summary': findings, 'managed_competitor_comparison': False}))


if __name__ == '__main__': main()
