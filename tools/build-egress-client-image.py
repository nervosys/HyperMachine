#!/usr/bin/env python3
"""Add the installed trusted host curl to the accepted guest fixture image.

Linux only. This is an owned HTTPS correctness fixture, never a performance
image. The accepted input stays unchanged. No downloaded client is executed.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


ACCEPTED_SHA256 = '1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'posix' and os.geteuid() == 0, 'owned Linux root fixture required')
    require(digest(args.base) == ACCEPTED_SHA256, 'base is not the accepted immutable fixture')
    require(not args.output.exists() and not args.report.exists(), 'output already exists')
    require(args.base.resolve() not in (args.output.resolve(), args.report.resolve()), 'input/output overlap')
    require(args.output.resolve() != args.report.resolve(), 'output/report overlap')
    client = Path('/usr/bin/curl')
    require(client.is_file(), 'installed trusted host curl required')
    linked = subprocess.run(['ldd', str(client)], capture_output=True, text=True, check=True, timeout=15)
    require('not found' not in linked.stdout, 'client dependency is missing')
    dependencies = sorted(set(re.findall(r'(?:=>\s+|^\s*)(/\S+)', linked.stdout, re.MULTILINE)))
    require(dependencies, 'dynamic client dependency inventory is empty')
    catalog = {}
    with tempfile.TemporaryDirectory(prefix='hm-egress-client-') as directory:
        root = Path(directory)
        # Extraction is restricted to the exact previously accepted image hash.
        unpacked = gzip.decompress(args.base.read_bytes())
        subprocess.run(['cpio', '-id', '--no-absolute-filenames', '--quiet'], input=unpacked,
                       cwd=root, check=True, capture_output=True, timeout=20)
        for source, destination in [(client, Path('/bin/curl'))] + [(Path(p), Path(p)) for p in dependencies]:
            require(source.is_file(), 'dependency is not a regular installed file')
            target = root / destination.relative_to('/')
            target.parent.mkdir(parents=True, exist_ok=True)
            require(not target.is_symlink(), 'fixture target is a symlink')
            shutil.copyfile(source, target)
            target.chmod(0o755)
            catalog[str(destination)] = digest(source)
        started = subprocess.run(['chroot', str(root), '/bin/curl', '--version'], capture_output=True,
                                 text=True, check=True, timeout=15)
        require('https' in started.stdout, 'fixture client lacks HTTPS support')
        names = sorted(['.'] + [str(p.relative_to(root)) for p in root.rglob('*')])
        for name in names:
            os.utime(root / name, (0, 0), follow_symlinks=False)
        entries = b'\0'.join(name.encode() for name in names) + b'\0'
        packed = subprocess.run(['cpio', '--null', '-o', '-H', 'newc', '-R', '0:0', '--reproducible', '--quiet'],
                                input=entries, cwd=root, capture_output=True, check=True, timeout=20)
        image = gzip.compress(packed.stdout, compresslevel=9, mtime=0)
        # Recheck every installed input before publishing the local fixture.
        for source, destination in [(client, Path('/bin/curl'))] + [(Path(p), Path(p)) for p in dependencies]:
            require(digest(source) == catalog[str(destination)], 'installed client input changed')
        require(digest(args.base) == ACCEPTED_SHA256, 'accepted image changed')
        with args.output.open('xb') as stream:
            stream.write(image)
    report = {'base_sha256': ACCEPTED_SHA256, 'image_sha256': digest(args.output),
              'image_bytes': len(image), 'client_files_sha256': catalog,
              'client_version': started.stdout, 'client_started_in_chroot': True,
              'limits': ['No KVM HTTPS request observed', 'Not a performance benchmark image']}
    with args.report.open('x') as stream:
        json.dump(report, stream, indent=2); stream.write('\n')
    print(f'Built owned HTTPS fixture: {len(image)} bytes, {len(catalog)} client files')


if __name__ == '__main__':
    main()
