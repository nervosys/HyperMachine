import hashlib, importlib.util, json, os, re, subprocess, tempfile, time
from pathlib import Path

root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
spec = importlib.util.spec_from_file_location('engines',root/'tools/bench-local-engines.py')
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)
paths = dict(daemon=Path('/var/tmp/hm-ioapic-diagnostic/hv2-sandboxd'),
    kernel=Path('/var/tmp/hm-competitive/bzImage-known-uart-irq'),
    initrd=Path('/var/tmp/hm-competitive/guest-no-agent.cpio.gz'),
    probe=Path(__file__),request_harness=Path(engines.__file__))
digest = lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
r = dict(diagnostic_only=True,negative_control=True,success=False,error=None,
    artifact_sha256={name:digest(path) for name,path in paths.items()},cleanup_errors=[])
process = None
owned = set()
with tempfile.TemporaryDirectory(prefix='hm-no-agent-node-',dir='/var/tmp') as directory:
    directory = Path(directory)
    port,proxy = engines.free_port(),engines.free_port()
    while port == proxy: proxy = engines.free_port()
    url = f'http://127.0.0.1:{port}'
    try:
        with (directory/'node.log').open('wb') as logfile:
            process = subprocess.Popen([str(paths['daemon']),'--no-template','--port',str(port),
                '--proxy-port',str(proxy),'--memory-mb','1024','--cpu-cores','1','--capacity','1',
                '--volume-dir',str(directory/'volumes'),'--snapshot-store',str(directory/'snapshots')],
                env={'PATH':'/usr/local/bin:/usr/bin:/bin','RUST_LOG':'warn',
                    'HV2_KERNEL':str(paths['kernel']),'HV2_INITRD':str(paths['initrd'])},
                stdin=subprocess.DEVNULL,stdout=logfile,stderr=subprocess.STDOUT)
            deadline = time.monotonic()+30
            while True:
                if process.poll() is not None: raise RuntimeError('daemon exited')
                try: engines.request(url,'GET','/templates'); break
                except OSError:
                    if time.monotonic() >= deadline: raise
                    time.sleep(.01)
            started = time.monotonic()
            try:
                response = engines.request(url,'POST','/v2/sandboxes',{'templateID':'base','timeout':60})
                owned.add(response['sandboxID'])
                raise AssertionError('agent-absent guest unexpectedly ready')
            except RuntimeError as error:
                r['expected_failure'] = str(error)
            r['elapsed_seconds'] = time.monotonic()-started
            message = r['expected_failure']
            assert 'HTTP 503:' in message and 'guest never became ready' in message
            assert re.search(r'\bTSC=0x[0-9a-f]+ TSC_DEADLINE=0x[0-9a-f]+',message)
            assert 'IOAPIC BASE=' in message
            pins = re.findall(r'\bGSI([0-9]+) RAW=0x[0-9a-f]+',message)
            assert pins == [str(pin) for pin in range(24)], pins
            r['remaining_sandbox_count'] = len(engines.request(url,'GET','/sandboxes'))
            assert r['remaining_sandbox_count'] == 0
            r['success'] = True
    except Exception as error: r['error'] = str(error)
    finally:
        for sandbox in owned:
            try: engines.request(url,'DELETE',f'/sandboxes/{sandbox}')
            except Exception as error: r['cleanup_errors'].append(str(error))
        if process is not None and not engines.stop(process): r['cleanup_errors'].append('daemon did not stop')
        r['node_log_tail'] = (directory/'node.log').read_text(errors='replace')[-8000:]
r['artifacts_unchanged'] = all(digest(path)==r['artifact_sha256'][name] for name,path in paths.items())
r['success'] = r['success'] and r['artifacts_unchanged'] and not r['cleanup_errors']
Path('/var/tmp/hm-competitive/no-agent-diagnostics.json').write_text(json.dumps(r,indent=2))
print(json.dumps(r,indent=2))
raise SystemExit(0 if r['success'] else 1)
