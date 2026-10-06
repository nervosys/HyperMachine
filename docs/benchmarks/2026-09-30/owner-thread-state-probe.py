import datetime, hashlib, json, sys, time
from pathlib import Path

pid = int(sys.argv[1])
rows = []
for _ in range(60):
    tasks = Path(f'/proc/{pid}/task')
    if not tasks.exists():
        break
    for task in tasks.iterdir():
        try:
            name = (task / 'comm').read_text().strip()
            if not name.startswith('vcpu-'):
                continue
            fields = (task / 'stat').read_text().rsplit(') ', 1)[1].split()
            rows.append(dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                tid=int(task.name), name=name, state=fields[0], utime=int(fields[11]),
                stime=int(fields[12]), start_ticks=int(fields[19]),
                wchan=(task / 'wchan').read_text().strip()))
        except (FileNotFoundError, ProcessLookupError):
            pass
    time.sleep(1)
report = dict(pid=pid, observations=rows, source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    limitation='Read-only shared-host observations; match thread lifetime and timestamps to failed boots before interpreting')
Path('/var/tmp/hm-competitive/owner-thread-observations-long.json').write_text(json.dumps(report, indent=2))
print(json.dumps(dict(pid=pid, observations=len(rows), source_sha256=report['source_sha256'])))
