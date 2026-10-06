#!/usr/bin/env python3
"""Reconstruct the accepted benchmark source from Git objects and frozen overlays."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess


def restore(repo, context, output):
    context = context.resolve(strict=True)
    output = output.resolve()
    if output.exists():
        raise ValueError('requires a new output directory')
    metadata = json.loads(context.read_text())
    commit = metadata['base_commit']
    if not re.fullmatch('[0-9a-f]{40}|[0-9a-f]{64}', commit):
        raise ValueError('invalid base commit')
    catalog = metadata['accepted_source_sha256']
    overlays = metadata['accepted_overlays']
    if len(catalog) != metadata['accepted_source_files'] or not set(overlays) <= set(catalog):
        raise ValueError('source catalog differs')
    for name, sha in catalog.items():
        path = PurePosixPath(name)
        if (path.is_absolute() or any(part in ('.git', '..') for part in path.parts)
                or path.as_posix() != name or any(char in name for char in ':\\\r\n\0')
                or not re.fullmatch('[0-9a-f]{64}', sha)):
            raise ValueError('invalid source entry')
    raw = subprocess.check_output(['git', '-C', str(repo.resolve(strict=True)), 'cat-file', '--batch'],
                                  input=''.join(commit + ':' + name + '\n' for name in catalog).encode())
    contents, offset = {}, 0
    for name, sha in catalog.items():
        end = raw.index(b'\n', offset)
        header = raw[offset:end].split()
        if len(header) != 3 or header[1] != b'blob':
            raise ValueError('base source unavailable: ' + name)
        length = int(header[2])
        offset = end + 1
        data = raw[offset:offset + length]
        offset += length + 1
        if name in overlays:
            path = (context.parent / 'accepted-overlays' / name).resolve(strict=True)
            if not path.is_relative_to(context.parent) or overlays[name] != sha:
                raise ValueError('overlay binding differs: ' + name)
            data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != sha:
            raise ValueError('accepted source hash differs: ' + name)
        contents[name] = data
    if offset != len(raw):
        raise ValueError('unexpected Git batch payload')
    output.mkdir(parents=True)
    for name, data in contents.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    return {'restored_source_files': len(contents), 'base_commit': commit,
            'accepted_overlay_files': len(overlays), 'output': str(output),
            'all_source_hashes_verified': True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--context', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(restore(args.repo, args.context, args.output), indent=2))
