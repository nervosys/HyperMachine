"""Daemon minor page faults per cold sandbox create: a count, not a timing.

Starts the daemon exactly as tools/bench-local-engines.py does, creates and
deletes N sandboxes one at a time, and reads the daemon's own minflt from
/proc before and after each create. Usage: faults.py BINARY N
"""
import json, os, socket, subprocess, sys, tempfile, time, urllib.request

binary, n = sys.argv[1], int(sys.argv[2])


def free_port():
    s = socket.socket(); s.bind(('127.0.0.1', 0)); p = s.getsockname()[1]; s.close(); return p


def request(url, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url + path, data=data, method=method,
                                 headers={'content-type': 'application/json'} if data else {})
    with urllib.request.urlopen(req, timeout=60) as r:
        text = r.read()
        return json.loads(text) if text else None


def counters(pid):
    # Totals over every thread of the process (field 10 = minflt).
    total = 0
    for tid in os.listdir(f'/proc/{pid}/task'):
        try:
            fields = open(f'/proc/{pid}/task/{tid}/stat').read().rsplit(')', 1)[1].split()
            total += int(fields[7])
        except OSError:
            pass
    rss = int(open(f'/proc/{pid}/statm').read().split()[1]) * 4
    return total, rss


tmp = tempfile.mkdtemp(prefix='hm-faults-')
port, proxy = free_port(), free_port()
url = f'http://127.0.0.1:{port}'
proc = subprocess.Popen(
    [binary, '--port', str(port), '--proxy-port', str(proxy), '--memory-mb', '1024', '--cpu-cores', '1',
     '--capacity', '128', '--no-template', '--volume-dir', tmp + '/volumes', '--snapshot-store', tmp + '/snapshots'],
    env={'PATH': '/usr/local/bin:/usr/bin:/bin', 'HV2_KERNEL': '/var/tmp/hm-competitive/bzImage-known-uart-irq',
         'HV2_INITRD': '/var/tmp/hm-competitive/guest-output-drain.cpio.gz', 'RUST_LOG': 'warn'},
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(300):
        try:
            request(url, 'GET', '/sandboxes'); break
        except Exception:
            time.sleep(0.1)
    deltas = []
    for i in range(n + 1):
        before, _ = counters(proc.pid)
        sbx = request(url, 'POST', '/v2/sandboxes', {'templateID': 'base', 'timeout': 300, 'allowInternetAccess': False})['sandboxID']
        out = request(url, 'POST', f'/sandboxes/{sbx}/exec', {'cmd': "printf ok", 'timeout_secs': 10})
        assert out['stdout'] == 'ok', out
        after, rss = counters(proc.pid)
        request(url, 'DELETE', f'/sandboxes/{sbx}')
        if i:  # the first create also warms the daemon; it is not counted
            deltas.append(after - before)
    deltas.sort()
    print(json.dumps({'binary': os.path.basename(binary), 'creates': n,
                      'minor_faults_per_create_median': deltas[len(deltas) // 2],
                      'min': deltas[0], 'max': deltas[-1], 'daemon_rss_kib_after': counters(proc.pid)[1]}))
finally:
    proc.terminate(); proc.wait(timeout=30)
