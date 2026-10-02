import contextlib, hashlib, importlib.util, io, json, os, subprocess, sys
from pathlib import Path

root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
spec = importlib.util.spec_from_file_location('engines', root/'tools/bench-local-engines.py')
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
observations = []
command = "printf 'UPTIME\\n'; cat /proc/uptime; printf 'DMESG\\n'; dmesg; printf 'INTERRUPTS\\n'; cat /proc/interrupts"
request_original = engines.request
rpc_original = engines.fc.rpc

def request(url, method, path, body=None):
    result = request_original(url, method, path, body)
    if method == 'POST' and path.endswith('/exec') and body and body.get('cmd', '').startswith("printf '%s' 'hm-engine-"):
        diagnostic = request_original(url, method, path, {'cmd': command, 'timeout_secs': 10})
        observations.append({'engine': 'hypermachine', 'response': diagnostic})
        if diagnostic.get('exit_code') != 0 or diagnostic.get('timed_out'):
            raise RuntimeError('HyperMachine guest diagnostic failed')
    return result

def rpc(stream, request_id, body):
    result = rpc_original(stream, request_id, body)
    if request_id == 2 and body.get('kind') == 'exec':
        diagnostic = rpc_original(stream, 3, {'kind': 'exec', 'program': '/bin/sh',
            'args': ['-c', command], 'timeout_ms': 10000})
        observations.append({'engine': 'firecracker', 'response': diagnostic})
        if diagnostic.get('exit_code') != 0 or diagnostic.get('timed_out') or diagnostic.get('truncated'):
            raise RuntimeError('Firecracker guest diagnostic failed')
    return result

engines.request = request
engines.fc.rpc = rpc
core = min(os.sched_getaffinity(0))
os.sched_setaffinity(0, {core})
worker_code = 'import time\nend=time.monotonic()+300\nx=1\nwhile time.monotonic()<end:\n for _ in range(10000): x=(x*1664525+1013904223)&0xffffffff\n'
worker = subprocess.Popen([sys.executable, '-c', worker_code], stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
report = None
worker_alive = False
source_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
try:
    sys.argv = ['guest-boot-diagnostic', '--hypermachine', '/var/tmp/hm-competitive-target/release/hv2-sandboxd',
        '--firecracker', '/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64',
        '--kernel', '/var/tmp/hm-competitive/bzImage', '--initrd', '/var/tmp/hm-competitive/guest-output-drain.cpio.gz',
        '--pairs', '3', '--hypermachine-log-filter', 'hv2_sandboxd=debug',
        '--environment', 'shared-WSL-nested-KVM-one-pinned-CPU-worker-guest-boot-DIAGNOSTIC']
    output = io.StringIO()
    with contextlib.redirect_stdout(output): engines.main()
    report = json.loads(output.getvalue())
    worker_alive = worker.poll() is None
finally:
    engines.stop(worker)
if report is None: raise RuntimeError('diagnostic produced no report')
report.update(diagnostic_only=True, guest_observations=observations,
    coordinator_sha256=source_hash, controlled_cpu_load={'cpu': core, 'workers': 1,
        'worker_code': worker_code, 'all_alive_through_cohort': worker_alive,
        'workers_cleaned_up': worker.poll() is not None})
report['limitations'].append('Ready timing includes diagnostic guest commands; not a benchmark comparison')
report['success'] = report['success'] and worker_alive and worker.poll() is not None and len(observations) == 6
Path('/var/tmp/hm-competitive/guest-boot-stages-diagnostic-3.json').write_text(json.dumps(report, indent=2))
print(json.dumps({'success': report['success'], 'observations': len(observations),
    'artifacts_unchanged': report['artifacts_unchanged'], 'setup_error': report['setup_error'],
    'cleanup_error': report['cleanup_error'], 'controlled_cpu_load': report['controlled_cpu_load']}))
raise SystemExit(0 if report['success'] else 1)
