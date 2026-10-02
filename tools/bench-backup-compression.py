#!/usr/bin/env python3
"""Compare gzip levels on one locked offline store, with verified bundle bytes."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import statistics
import tarfile
import tempfile
import time


def require(value, message):
    if not value: raise ValueError(message)


def digest(path):
    with path.open('rb') as stream: return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--store', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pairs', type=int, default=4)
    args = parser.parse_args()
    require(2 <= args.pairs <= 20, 'pairs must be 2 to 20')
    os.umask(0o077)
    root = args.store.resolve(strict=True)
    args.output = args.output.resolve()
    require(not args.output.is_relative_to(root), 'output must be outside store')
    args.output.mkdir(parents=True, exist_ok=False)
    tool = Path(__file__).with_name('backup-snapshot-store.py')
    spec = importlib.util.spec_from_file_location('backup', tool)
    backup = importlib.util.module_from_spec(spec); spec.loader.exec_module(backup)
    report = {'success': False, 'managed_competitor_comparison': False, 'rows': [],
              'tool_sha256': digest(tool), 'coordinator_sha256': digest(Path(__file__)),
              'measurement': 'scan, dependency verification, gzip tar capture, final rescan; warm-cache AB/BA pairs; no upload or restore timing'}
    try:
        with backup.offline_lock(root), tempfile.TemporaryDirectory(prefix='capture-', dir=args.output) as temporary:
            temporary = Path(temporary)
            before = backup.scan(root, 64 * 1024**3)
            report.update(files=len(before['files']), expanded_bytes=before['expanded_bytes'],
                          catalog_sha256=hashlib.sha256(backup.canonical(before)).hexdigest())
            for pair in range(args.pairs):
                for level in ([1, 6] if pair % 2 == 0 else [6, 1]):
                    output = temporary / 'bundle.tar.gz'
                    start = time.perf_counter()
                    manifest = backup.make_bundle(root, output, 64 * 1024**3, level)
                    elapsed = time.perf_counter() - start
                    require(manifest == before, 'source catalog changed')
                    found = set()
                    with tarfile.open(output, 'r:gz') as archive:
                        entry = archive.next()
                        require(entry.name == backup.MANIFEST, 'manifest missing')
                        with archive.extractfile(entry) as stream:
                            require(backup.read_json(stream.read()) == manifest, 'manifest changed')
                        while entry := archive.next():
                            require(entry.isfile() and entry.name.startswith('store/'), 'unexpected bundle entry')
                            name = entry.name[6:]
                            require(name in manifest['files'] and name not in found, 'unexpected/duplicate file')
                            with archive.extractfile(entry) as stream:
                                checksum = hashlib.file_digest(stream, 'sha256').hexdigest()
                            require(checksum == manifest['files'][name]['sha256'] and entry.size == manifest['files'][name]['size'], 'captured bytes changed')
                            found.add(name)
                    require(found == set(manifest['files']), 'bundle file missing')
                    report['rows'].append(dict(pair=pair, level=level, capture_seconds=elapsed,
                        compressed_bytes=output.stat().st_size, encrypted_bytes=output.stat().st_size + 36,
                        all_file_hashes_verified=True))
                    output.unlink()
            require(backup.scan(root, 64 * 1024**3) == before, 'store changed during comparison')
        report['summary'] = {str(level): {
            'median_capture_seconds': statistics.median(r['capture_seconds'] for r in report['rows'] if r['level'] == level),
            'median_encrypted_bytes': statistics.median(r['encrypted_bytes'] for r in report['rows'] if r['level'] == level)} for level in [1, 6]}
        report['success'] = True
    except Exception as error: report['error'] = str(error)
    report['artifacts_unchanged'] = digest(tool) == report['tool_sha256'] and digest(Path(__file__)) == report['coordinator_sha256']
    report['success'] &= report['artifacts_unchanged']
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report), flush=True)
    return 0 if report['success'] else 1


if __name__ == '__main__': raise SystemExit(main())
