#!/usr/bin/env python3
"""Verify frozen private guest URL functional evidence; no timing claims."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(root):
    manifest = json.loads((root / 'manifest.json').read_text())
    for name, expected in manifest['sha256'].items():
        path = (root / name).resolve()
        require(path.is_relative_to(root.resolve()), 'manifest path escaped archive')
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, f'hash mismatch: {name}')
    report = json.loads((root / 'kvm/report.json').read_text())
    require(report['success'] is True, 'fixture did not pass')
    require(report['artifacts_unchanged'] is True and report['artifact_sha256_after'] == report['artifact_sha256'], 'artifacts changed')
    require(report['remaining_sandboxes'] == 0 and not report['cleanup_errors'], 'incomplete cleanup')
    stopped = report['owned_processes_stopped']
    require(stopped and all(isinstance(p['exit_code'], int) for p in stopped), 'owned process still running')
    cases = report['cases']
    require(cases and all(c['success'] is True for c in cases), 'functional case failed')
    indexed = {c['name']: c for c in cases}
    require(len(indexed) == len(cases), 'duplicate case names')
    for name in ('private-web-browser-login-and-guest-identity', 'private-web-custom-domain-authenticated', 'private-web-unauthorized-request-does-not-resume', 'private-web-policy-reload-and-expiry'):
        require(name in indexed, f'missing case: {name}')
    require(report['private_web_transport_gate']['plaintext_startup_refused'] is True, 'plaintext startup accepted')
    for name in ('private-web-browser-login-and-guest-identity', 'private-web-custom-domain-authenticated', 'private-web-unauthorized-request-does-not-resume'):
        result = indexed[name]['result']
        require(result['status'] == 200 and result['identity_from_policy'] is True and result['login_credential_absent'] is True, 'private identity verification failed')
    require(all(indexed['private-web-policy-reload-and-expiry']['result'].get(k) is True for k in ('old_key_revoked', 'invalid_reload_preserved_policy', 'expired_key_refused')), 'rotation evidence incomplete')
    for name, file in (('coordinator', 'coordinator.py'), ('audit_verifier', 'verify-access-audit.py')):
        require(hashlib.sha256((root / file).read_bytes()).hexdigest() == report['artifact_sha256'][name], 'frozen coordinator differs')
    spec = importlib.util.spec_from_file_location('private_web_audit_verifier', root / 'verify-access-audit.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    checked = module.verify((root / 'kvm/report-access.jsonl').read_bytes(), bytes.fromhex('42' * 32))
    require(checked['verified_records'] == report['access_audit']['verified_records'] and checked['uncompleted_admissions'] == 0, 'independent audit mismatch')
    audit = report['access_audit']
    require(audit['credentials_absent'] is True and audit['uncompleted_admissions'] == 0, 'audit verification failed')
    return {'verified_files': len(manifest['sha256']), 'passed_cases': len(cases), 'artifacts_unchanged': True, 'cleanup_verified': True, 'functional_only': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.archive), indent=2))


if __name__ == '__main__':
    main()
