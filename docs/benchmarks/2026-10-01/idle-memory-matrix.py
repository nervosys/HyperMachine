import subprocess, sys
from pathlib import Path
root = Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine')
statuses = []
for concurrency in [1, 8, 50, 100]:
    print('Starting fixed-idle memory cohort C' + str(concurrency), flush=True)
    run = subprocess.run([sys.executable, str(root/'target/idle-memory-coordinator.py'), '--concurrency', str(concurrency), '--pairs', '3', '--name', 'idle-memory-c'+str(concurrency)])
    statuses.append(run.returncode)
    print('Completed C' + str(concurrency) + ': exit ' + str(run.returncode), flush=True)
raise SystemExit(0 if all(code == 0 for code in statuses) else 1)