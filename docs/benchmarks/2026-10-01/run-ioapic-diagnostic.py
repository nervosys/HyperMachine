import argparse, hashlib, json, os, signal, subprocess, sys, time
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--concurrency', type=int, required=True)
parser.add_argument('--pairs', type=int, required=True)
parser.add_argument('--name', required=True)
args = parser.parse_args()
root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
cores = sorted(os.sched_getaffinity(0))[:8]
os.sched_setaffinity(0, cores)
worker_code = 'import time\nend=time.monotonic()+7200\nx=1\nwhile time.monotonic()<end:\n for _ in range(10000): x=(x*1664525+1013904223)&0xffffffff\n'
worker = subprocess.Popen([sys.executable, '-c', worker_code], stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    preexec_fn=lambda: os.sched_setaffinity(0, {cores[0]}))
report = None
child = None
try:
    time.sleep(.2)
    if worker.poll() is not None: raise RuntimeError('CPU worker did not start')
    command = [sys.executable, str(root/'tools/bench-local-engines-concurrent.py'),
        '--hypermachine', '/var/tmp/hm-ioapic-diagnostic/hv2-sandboxd',
        '--firecracker', '/var/tmp/hm-competitive/firecracker/firecracker-v1.17.0-x86_64',
        '--kernel', '/var/tmp/hm-competitive/bzImage-known-uart-irq',
        '--initrd', '/var/tmp/hm-competitive/guest-output-drain.cpio.gz',
        '--pairs', str(args.pairs), '--concurrency', str(args.concurrency),
        '--environment', 'shared-WSL-nested-KVM-eight-host-CPU-affinity-one-pinned-worker-concurrent-bursts']
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, start_new_session=True)
    stdout, stderr = child.communicate(timeout=900)
    report = json.loads(stdout)
    report['diagnostic_only'] = True
    report['diagnostic_scope'] = 'Failure-only IOAPIC and TSC/deadline formatter; warn logging; no interrupt fix'
    report['cohort_exit_code'] = child.returncode
    report['cohort_stderr'] = stderr
    report['controlled_cpu_load'] = {'workers':1, 'cpu':cores[0], 'worker_code':worker_code,
        'all_alive_through_cohort':worker.poll() is None, 'workers_cleaned_up':False}
    report['coordinator_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
except subprocess.TimeoutExpired:
    os.killpg(child.pid, signal.SIGTERM)
    try: child.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL); child.communicate(timeout=5)
    raise
finally:
    if worker.poll() is None:
        worker.terminate()
        try: worker.wait(timeout=5)
        except subprocess.TimeoutExpired: worker.kill(); worker.wait(timeout=5)
    if report is not None:
        report['controlled_cpu_load']['workers_cleaned_up'] = worker.poll() is not None
        report['success'] = report['success'] and report['cohort_exit_code'] == 0 and all(
            report['controlled_cpu_load'][field] for field in ('all_alive_through_cohort', 'workers_cleaned_up'))
        Path('/var/tmp/hm-competitive', args.name+'.json').write_text(json.dumps(report, indent=2))
        print(json.dumps({'success':report['success'], 'ready_ms':report['ready_ms'],
            'setup_error':report['setup_error'], 'cleanup_errors':report['cleanup_errors'],
            'failed_samples':[{key:row.get(key) for key in ('engine','pair','index','error','cleanup_error','hold_error')}
                for batch in report['batches'] for row in batch['samples'] if not row['success'] or not row['cleanup_success']]}))
raise SystemExit(0 if report is not None and report['success'] else 1)
