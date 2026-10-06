#!/usr/bin/env python3
"""Verify version-pinned backup evidence and retain failed fixture instrumentation."""
import argparse
import hashlib
import json
from pathlib import Path
import re


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
    for cohort, coordinator, success in [('synthetic', 'first-check-multipart-backup.py', False),
            ('synthetic-corrected', 'second-check-multipart-backup.py', False), ('synthetic-verified', 'check-multipart-backup.py', True)]:
        report = json.loads((root / cohort / 'report.json').read_text())
        require(report['success'] is success and report['artifacts_unchanged'] and report['emulator_exit_code'] == 0
                and not report['cleanup_errors'] and not report['performance_comparison'], 'cohort outcome changed')
        require(report['tool_sha256'] == manifest['backup-snapshot-store.py']
                and report['coordinator_sha256'] == manifest[coordinator], 'synthetic provenance mismatch')
        require(len(report['checks']) == (11 if success else 9) and all(c['passed'] for c in report['checks']), 'checks missing')
        if not success:
            require(report['error'] == 'wrong-version response was not refused/closed', 'failed instrumentation hidden')
            continue
        log = re.sub(r'\x1b\[[0-9;]*m', '', (root / cohort / 's3.log').read_text())
        for method in ['single', 'multipart']:
            check = next(c for c in report['checks'] if c['name'] == method + '-version-pinned-recovery')
            old, new = check['old_receipt'], check['new_receipt']
            require(old['upload_method'] == new['upload_method'] == method
                    and old['version_id'] != new['version_id'] and old['version_id'] != 'null'
                    and old['sha256'] != new['sha256'] and check['wrong_response_refused'] and check['deleted_version_refused'], 'version recovery incomplete')
            for receipt in [old, new]:
                require('GET /hm-owned-multipart-fixture/' + receipt['object'] + '?versionId=' + receipt['version_id'] + ' HTTP/1.1" 200' in log, 'version GET wire evidence missing')
            require('DELETE /hm-owned-multipart-fixture/' + old['object'] + '?versionId=' + old['version_id'] in log, 'explicit version deletion not exercised')
    report = json.loads((root / 'kvm/report.json').read_text())
    require(report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors']
            and len(report['checks']) == 15 and all(p['exit_code'] == 0 for p in report['processes_stopped']), 'KVM recovery failed')
    require(report['artifact_sha256']['tool'] == manifest['backup-snapshot-store.py']
            and report['artifact_sha256']['coordinator'] == manifest['check-object-backup.py']
            and report['artifact_sha256']['daemon'] == '2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f', 'KVM provenance mismatch')
    require(all(value is True for name, value in report['kvm'].items() if name != 'backup_receipt'), 'guest recovery incomplete')
    receipt = report['kvm']['backup_receipt']
    require(receipt['upload_method'] == 'multipart' and receipt['parts'] >= 2 and receipt['version_id'] != 'null', 'KVM version receipt missing')
    log = re.sub(r'\x1b\[[0-9;]*m', '', (root / 'kvm/s3.log').read_text())
    deleted = log.index('DELETE /hm-owned-backup-fixture/kvm.hmb HTTP/1.1" 204')
    fetched = log.index('GET /hm-owned-backup-fixture/kvm.hmb?versionId=' + receipt['version_id'] + ' HTTP/1.1" 200')
    require(deleted < fetched, 'KVM version not fetched after current-key deletion')
    print(json.dumps({'success': True, 'hashed_files': len(manifest), 'failed_fixture_cohorts_retained': 2, 'managed_store_durability_verified': False}))


if __name__ == '__main__': main()
