#!/usr/bin/env python3
"""Replace the agent in an exact accepted image for owned UDP KVM checks."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

BASE_SHA256 = '1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('base', 'agent', 'output', 'report'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--udp-echo', type=Path)
    parser.add_argument('--tcp-fixture', type=Path)
    args = parser.parse_args()
    if digest(args.base) != BASE_SHA256 or args.output.exists() or args.report.exists():
        raise ValueError('exact accepted base and fresh output/report required')
    if len({p.resolve() for p in (args.base, args.agent, args.output, args.report)}) != 4:
        raise ValueError('input/output paths overlap')
    agent_hash = digest(args.agent)
    echo_hash = digest(args.udp_echo) if args.udp_echo else None
    headers = subprocess.run(['readelf', '-l', str(args.agent)], capture_output=True,
                             text=True, check=True, timeout=15).stdout
    if 'INTERP' in headers:
        raise ValueError('guest agent must be static')
    if args.udp_echo:
        if args.udp_echo.resolve() in {p.resolve() for p in (args.base, args.agent, args.output, args.report)}:
            raise ValueError('echo input/output overlap')
        headers = subprocess.run(['readelf', '-l', str(args.udp_echo)], capture_output=True,
                                 text=True, check=True, timeout=15).stdout
        if 'INTERP' in headers:
            raise ValueError('echo fixture must be static')
    tcp_hash = digest(args.tcp_fixture) if args.tcp_fixture else None
    if args.tcp_fixture:
        if args.tcp_fixture.resolve() in {p.resolve() for p in (args.base, args.agent, args.output, args.report)} or (args.udp_echo and args.tcp_fixture.resolve() == args.udp_echo.resolve()):
            raise ValueError('TCP fixture input/output overlap')
        headers = subprocess.run(['readelf', '-l', str(args.tcp_fixture)], capture_output=True, text=True, check=True, timeout=15).stdout
        if 'INTERP' in headers: raise ValueError('TCP fixture must be static')
    with tempfile.TemporaryDirectory(prefix='hm-udp-image-') as temporary:
        root = Path(temporary)
        subprocess.run(['cpio', '-id', '--no-absolute-filenames', '--quiet'],
                       input=gzip.decompress(args.base.read_bytes()), cwd=root,
                       capture_output=True, check=True, timeout=20)
        target = root / 'bin/hv2-guest-agentd'
        if target.is_symlink():
            raise ValueError('agent target is a symlink')
        shutil.copyfile(args.agent, target)
        target.chmod(0o755)
        if args.udp_echo:
            target = root / 'bin/hm-udp-echo'
            if target.exists() or target.is_symlink():
                raise ValueError('echo target already exists')
            shutil.copyfile(args.udp_echo, target)
            target.chmod(0o755)
        if args.tcp_fixture:
            target = root / 'bin/tcp-fixture'
            if target.exists() or target.is_symlink(): raise ValueError('TCP fixture target already exists')
            shutil.copyfile(args.tcp_fixture, target); target.chmod(0o755)
        names = sorted(['.'] + [str(p.relative_to(root)) for p in root.rglob('*')])
        for name in names:
            os.utime(root / name, (0, 0), follow_symlinks=False)
        packed = subprocess.run(['cpio', '--null', '-o', '-H', 'newc', '-R', '0:0',
                                 '--reproducible', '--quiet'], cwd=root,
                                input=b'\0'.join(name.encode() for name in names) + b'\0',
                                capture_output=True, check=True, timeout=20).stdout
        image = gzip.compress(packed, compresslevel=9, mtime=0)
        if digest(args.agent) != agent_hash or digest(args.base) != BASE_SHA256:
            raise ValueError('input changed during image build')
        if args.udp_echo and digest(args.udp_echo) != echo_hash:
            raise ValueError('echo input changed during image build')
        if args.tcp_fixture and digest(args.tcp_fixture) != tcp_hash:
            raise ValueError('TCP fixture changed during image build')
        with args.output.open('xb') as output:
            output.write(image)
    with args.report.open('x') as report:
        json.dump({'base_sha256': BASE_SHA256, 'agent_sha256': agent_hash,
                   'echo_sha256': echo_hash, 'tcp_fixture_sha256': tcp_hash,
                   'image_sha256': digest(args.output), 'image_bytes': len(image)}, report, indent=2)
        report.write('\n')


if __name__ == '__main__':
    main()
