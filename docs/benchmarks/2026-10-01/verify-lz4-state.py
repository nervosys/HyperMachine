import hashlib, importlib.util, json, os, subprocess, tempfile, time
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
spec = importlib.util.spec_from_file_location('engines', root/'tools/bench-local-engines.py')
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
paths = {'daemon': Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd'),
         'kernel': Path('/var/tmp/hm-kernel-lz4/bzImage'),
         'initrd': Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),
         'probe': Path(__file__), 'request_harness': root/'tools/bench-local-engines.py'}
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
report = {'artifact_sha256': {name: digest(path) for name, path in paths.items()},
          'diagnostic_only': True, 'success': False, 'error': None, 'checks': [],
          'cleanup_errors': []}
owned = set()
process = None
with tempfile.TemporaryDirectory(prefix='hm-mptable-state-', dir='/var/tmp') as directory:
    directory = Path(directory)
    port, proxy = engines.free_port(), engines.free_port()
    while port == proxy: proxy = engines.free_port()
    url = f'http://127.0.0.1:{port}'
    def request(method, path, body=None): return engines.request(url, method, path, body)
    def execute(sandbox, command):
        value = request('POST', f'/sandboxes/{sandbox}/exec', {'cmd': command, 'timeout_secs': 10})
        if value.get('exit_code') != 0 or value.get('timed_out'):
            raise RuntimeError(f'guest command failed: {value}')
        return value['stdout']
    try:
        with (directory/'node.log').open('wb') as logfile:
            process = subprocess.Popen([str(paths['daemon']), '--port', str(port),
                '--proxy-port', str(proxy), '--memory-mb', '1024', '--cpu-cores', '1',
                '--capacity', '8', '--volume-dir', str(directory/'volumes'),
                '--snapshot-store', str(directory/'snapshots')],
                env={'PATH': '/usr/local/bin:/usr/bin:/bin', 'RUST_LOG': 'warn',
                     'HV2_KERNEL': str(paths['kernel']), 'HV2_INITRD': str(paths['initrd'])},
                stdin=subprocess.DEVNULL, stdout=logfile, stderr=subprocess.STDOUT)
            deadline = time.monotonic()+30
            while True:
                if process.poll() is not None: raise RuntimeError('daemon exited before readiness')
                try: templates = request('GET', '/templates'); break
                except OSError:
                    if time.monotonic() >= deadline: raise
                    time.sleep(.01)
            base = next((entry for entry in templates if 'base' in entry.get('aliases', [])), None)
            if not base or base.get('snapshot') is not True:
                raise RuntimeError('state validation requires a snapshot-backed base template')
            report['template'] = base
            value = request('POST', '/v2/sandboxes', {'templateID': 'base', 'timeout': 600})
            sandbox = value['sandboxID']
            owned.add(sandbox)
            report['kernel_topology'] = execute(sandbox,
                "dmesg | grep -E 'MP-table|Processors:|IOAPIC|MPTABLE' ")
            report['interrupts_before'] = execute(sandbox, 'cat /proc/interrupts')
            if 'IO-APIC' not in report['interrupts_before']:
                raise RuntimeError('guest does not report I/O APIC interrupts')
            execute(sandbox, 'printf before > /root/mptable-state')
            request('POST', f'/sandboxes/{sandbox}/checkpoints', {'name': 'before'})
            execute(sandbox, 'printf after > /root/mptable-state')
            request('POST', f'/sandboxes/{sandbox}/checkpoints/before/restore')
            if execute(sandbox, 'cat /root/mptable-state') != 'before':
                raise RuntimeError('checkpoint did not roll back guest state')
            report['checks'].append('checkpoint restores prior guest state')
            request('POST', f'/sandboxes/{sandbox}/pause', {})
            if request('GET', f'/sandboxes/{sandbox}')['state'] != 'paused':
                raise RuntimeError('pause did not suspend guest')
            request('POST', f'/sandboxes/{sandbox}/resume', {'timeout': 600})
            if execute(sandbox, 'cat /root/mptable-state') != 'before':
                raise RuntimeError('resume lost guest state')
            report['checks'].append('pause/resume retains guest state and command readiness')
            children = request('POST', f'/sandboxes/{sandbox}/fork', {'count': 2, 'timeout': 600})
            for child in children:
                if 'sandbox' in child: owned.add(child['sandbox']['sandboxID'])
            if len(children) != 2 or any('sandbox' not in child for child in children):
                raise RuntimeError(f'fork failed: {children}')
            for child in children:
                if execute(child['sandbox']['sandboxID'], 'cat /root/mptable-state') != 'before':
                    raise RuntimeError('fork lost guest state')
            report['checks'].append('two forked guests retain state and answer commands')
            report['interrupts_after'] = execute(sandbox, 'cat /proc/interrupts')
            report['success'] = True
    except Exception as error: report['error'] = str(error)
    finally:
        for sandbox in owned:
            try: request('DELETE', f'/sandboxes/{sandbox}')
            except Exception as error: report['cleanup_errors'].append(str(error))
        if process is not None and process.poll() is None:
            try:
                if request('GET', '/sandboxes') != []:
                    report['cleanup_errors'].append('isolated node has remaining sandbox records')
            except Exception as error: report['cleanup_errors'].append(str(error))
        if process is not None:
            if not engines.stop(process): report['cleanup_errors'].append('daemon did not stop')
        if (directory/'node.log').exists():
            report['node_log_tail'] = (directory/'node.log').read_text(errors='replace')[-8000:]
report['artifacts_unchanged'] = all(digest(path) == report['artifact_sha256'][name] for name, path in paths.items())
report['success'] = report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors']
Path('/var/tmp/hm-kernel-lz4/state.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
raise SystemExit(0 if report['success'] else 1)
