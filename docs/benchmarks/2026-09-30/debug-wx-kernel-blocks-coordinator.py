import hashlib, json, os, signal, subprocess, sys, time
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
output = Path('/var/tmp/hm-competitive/debug-wx-kernel-blocks-20.json')
paths = {'daemon': Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd'),
    'firecracker': Path('/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64'),
    'original_kernel': Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),
    'debug_wx_off_kernel': Path('/var/tmp/hm-kernel-debugwx/bzImage'),
    'initrd': Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),
    'harness': root/'tools/bench-local-engines.py',
    'firecracker_harness': root/'tools/bench-firecracker-local.py',
    'coordinator': Path(__file__), 'kernel_config': Path('/var/tmp/hm-kernel-debugwx/resolved.config'), 'kernel_defconfig': Path('/var/tmp/hm-kernel-debugwx/guest.defconfig'), 'kernel_builder': root/'tools/guest-image/build-kernel.sh'}
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
identities = {name: digest(path) for name, path in paths.items()}
core = min(os.sched_getaffinity(0))
os.sched_setaffinity(0, {core})
worker_code = 'import time\nend=time.monotonic()+7200\nx=1\nwhile time.monotonic()<end:\n for _ in range(10000): x=(x*1664525+1013904223)&0xffffffff\n'
worker = subprocess.Popen([sys.executable, '-c', worker_code], stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
report = {'success': False, 'complete': False, 'artifact_sha256': identities,
    'profile': 'native cold create-to-command, concurrency 1, two pairs per node',
    'design': '20 blocks, alternate kernel order; each kernel run alternates engine AB/BA',
    'controlled_cpu_load': {'workers': 1, 'cpu': core, 'worker_code': worker_code,
        'all_alive_through_cohort': False, 'workers_cleaned_up': False},
    'blocks': [], 'error': None,
    'limitations': ['Shared nested-KVM host; no dedicated hardware',
        'Persistent HyperMachine HTTP node versus fresh Firecracker process',
        'No managed SDK, snapshot, density or fleet comparison',
        'Each two-pair run uses a fresh isolated daemon; differs from longer per-node cohorts']}
def save(): output.write_text(json.dumps(report, indent=2))
try:
    time.sleep(.2)
    if worker.poll() is not None: raise RuntimeError('CPU worker did not start')
    for block in range(20):
        order = ('original_kernel', 'debug_wx_off_kernel') if block % 2 == 0 else ('debug_wx_off_kernel', 'original_kernel')
        for kernel in order:
            if worker.poll() is not None: raise RuntimeError('CPU worker exited during cohort')
            command = [sys.executable, str(paths['harness']), '--hypermachine', str(paths['daemon']),
                '--firecracker', str(paths['firecracker']), '--kernel', str(paths[kernel]),
                '--initrd', str(paths['initrd']), '--pairs', '2', '--environment',
                f'shared-WSL-nested-KVM-one-pinned-CPU-worker-kernel-block-{block}-{kernel}']
            child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, start_new_session=True)
            try:
                stdout, stderr = child.communicate(timeout=300)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGTERM)
                try: child.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL); child.communicate(timeout=5)
                raise RuntimeError(f'block {block}/{kernel} timed out; owned process group terminated')
            cohort = json.loads(stdout)
            report['blocks'].append({'block': block, 'kernel': kernel,
                'process_exit_code': child.returncode, 'stderr': stderr, 'report': cohort})
            save()
        print(json.dumps({'completed_blocks': block+1}), flush=True)
    report['complete'] = True
    report['controlled_cpu_load']['all_alive_through_cohort'] = worker.poll() is None
except Exception as error: report['error'] = str(error)
finally:
    if worker.poll() is None:
        worker.terminate()
        try: worker.wait(timeout=5)
        except subprocess.TimeoutExpired: worker.kill(); worker.wait(timeout=5)
    report['controlled_cpu_load']['workers_cleaned_up'] = worker.poll() is not None
    report['artifacts_unchanged'] = all(digest(path) == identities[name] for name, path in paths.items())
    report['success'] = report['complete'] and len(report['blocks']) == 40 and not report['error'] and report['artifacts_unchanged'] and all(
        entry['process_exit_code'] == 0 and entry['report']['success'] for entry in report['blocks']) and all(
        report['controlled_cpu_load'][field] for field in ('all_alive_through_cohort', 'workers_cleaned_up'))
    save()
print(json.dumps({'success': report['success'], 'complete': report['complete'], 'error': report['error'],
    'runs': len(report['blocks']), 'artifacts_unchanged': report['artifacts_unchanged']}), flush=True)
raise SystemExit(0 if report['success'] else 1)
