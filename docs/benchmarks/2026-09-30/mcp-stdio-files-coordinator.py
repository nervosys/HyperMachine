import hashlib, importlib.util, json, os, subprocess, tempfile, time
from pathlib import Path

root = Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine')
spec = importlib.util.spec_from_file_location('engines', root/'tools/bench-local-engines.py')
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
paths = {'daemon': Path('/var/tmp/hm-competitive-target/release/hv2-sandboxd'),
         'kernel': Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),
         'initrd': Path('/var/tmp/hm-competitive/guest-output-drain.cpio.gz'),
         'cli': Path('/var/tmp/hm-competitive-target/debug/hm'),
         'probe': Path(__file__), 'harness': root/'tools/e2e-mcp-sandbox.py'}
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
report = {'artifact_sha256': {name: digest(path) for name, path in paths.items()},
          'success': False, 'error': None, 'cleanup_errors': []}
process = None
with tempfile.TemporaryDirectory(prefix='hm-mcp-files-', dir='/var/tmp') as directory:
    directory = Path(directory)
    port, proxy = engines.free_port(), engines.free_port()
    while port == proxy: proxy = engines.free_port()
    url = f'http://127.0.0.1:{port}'
    def request(method, path, body=None): return engines.request(url, method, path, body)
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
                raise RuntimeError('test requires snapshot-backed base template')
            report['template'] = base
            child = subprocess.run(['/var/tmp/hm-mcp-venv/bin/python', str(paths['harness']),
                '--binary', str(paths['cli']), '--api-url', url,
                '--envd-proxy', f'http://127.0.0.1:{proxy}',
                '--environment', 'WSL2 Debian Linux KVM, local snapshot template, 1 vCPU/1024 MiB'],
                capture_output=True, text=True, timeout=240)
            report['client'] = json.loads(child.stdout)
            report['client_exit_code'] = child.returncode
            report['success'] = child.returncode == 0 and report['client']['success']
    except Exception as error: report['error'] = str(error)
    finally:
        if process is not None and process.poll() is None:
            try:
                remaining = request('GET', '/sandboxes')
                report['remaining_sandboxes'] = remaining
                if remaining:
                    report['cleanup_errors'].append('client left sandbox records')
                    for sandbox in remaining:
                        request('DELETE', f"/sandboxes/{sandbox['sandboxID']}")
            except Exception as error: report['cleanup_errors'].append(str(error))
        if process is not None:
            if not engines.stop(process): report['cleanup_errors'].append('daemon did not stop')
        if (directory/'node.log').exists():
            report['node_log_tail'] = (directory/'node.log').read_text(errors='replace')[-8000:]
report['artifacts_unchanged'] = all(digest(path) == report['artifact_sha256'][name] for name, path in paths.items())
report['success'] = report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors']
Path('/var/tmp/hm-competitive/mcp-stdio-files.json').write_text(json.dumps(report, indent=2))
print(json.dumps({key: report[key] for key in ['success', 'error', 'cleanup_errors', 'artifacts_unchanged']}))
raise SystemExit(0 if report['success'] else 1)
