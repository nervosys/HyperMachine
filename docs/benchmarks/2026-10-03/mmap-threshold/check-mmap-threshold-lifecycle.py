#!/usr/bin/env python3
"""Run named-source deletion and pause/resume checks with both allocator policies."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import time

parser = argparse.ArgumentParser(description=__doc__)
for name in ('daemon', 'kernel', 'initrd', 'output'):
    parser.add_argument('--' + name, type=Path, required=True)
args = parser.parse_args()
if args.output.exists():
    raise ValueError('preserve earlier evidence')
spec = importlib.util.spec_from_file_location('threshold_lifecycle', Path(__file__).with_name('check-sparse-named-lifecycle.py'))
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
daemon = args.daemon.resolve(strict=True)
digest = fixture.engines.digest(daemon)
original_launch = fixture.subprocess.Popen
activations = []
owned = []

def launch(command, *positional, **kwargs):
    matched = Path(command[0]).resolve() == daemon
    if matched:
        if len(activations) >= 2:
            raise ValueError('unexpected extra daemon')
        environment = dict(kwargs['env'])
        if len(activations) == 1:
            environment['MALLOC_MMAP_THRESHOLD_'] = '131072'
        kwargs['env'] = environment
    process = original_launch(command, *positional, **kwargs)
    if matched:
        owned.append(process)
        deadline = time.monotonic() + 3
        while True:
            if process.poll() is not None:
                raise ValueError('owned daemon exited before verification')
            actual = {}
            try:
                if fixture.engines.digest(Path(f'/proc/{process.pid}/exe')) == digest:
                    raw = Path(f'/proc/{process.pid}/environ').read_bytes()
                    if len(raw) > 65536:
                        raise ValueError('environment exceeds limit')
                    actual = dict(item.decode().split('=', 1) for item in raw.split(b'\0') if item)
            except OSError:
                pass
            if actual == environment:
                break
            if time.monotonic() >= deadline:
                raise ValueError('owned executable/environment mismatch')
            time.sleep(.01)
        activations.append({'pid': process.pid, 'executable_sha256': digest, 'environment': actual})
    return process

fixture.subprocess.Popen = launch
sys.argv = ['check-sparse-named-lifecycle.py', '--baseline', str(daemon), '--candidate', str(daemon),
            '--kernel', str(args.kernel), '--initrd', str(args.initrd), '--output', str(args.output)]
try:
    code = fixture.main()
finally:
    fixture.subprocess.Popen = original_launch
    for process in owned:
        if process.poll() is None:
            fixture.engines.stop(process)
report = json.loads(args.output.read_bytes())
report.update(same_binary=True, activations=activations, candidate_environment={'MALLOC_MMAP_THRESHOLD_': '131072'},
              wrapper_sha256=fixture.engines.digest(Path(__file__)))
args.output.write_text(json.dumps(report, indent=2) + '\n')
if code or not report['all_variants_passed'] or not report['cleanup_verified'] or len(activations) != 2:
    raise ValueError('lifecycle check failed')
