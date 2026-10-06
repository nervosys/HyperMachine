from pathlib import Path
import json,hashlib
p=Path(__file__).parent;b=json.loads((p/'baseline/resume-cycles.json').read_text());c=json.loads((p/'candidate/resume-cycles.json').read_text());r=json.loads((p/'candidate/report.json').read_text())
assert len(b)==6 and [row['cycle'] for row in b]==list(range(1,7))
assert all(row['pause_completed'] and row['resume_completed'] and row['exact_udp'] for row in b[:-1])
assert b[-1]['pause_completed'] and not b[-1]['resume_completed'] and not b[-1]['exact_udp'] and b[-1]['failure_type']=='TimeoutError' and b[-1]['elapsed_seconds']>=30
assert len(c)==7 and [row['cycle'] for row in c]==list(range(1,8)) and all(row['pause_completed'] and row['resume_completed'] and row['exact_udp'] for row in c)
assert r['guests_remaining']==0
for key in ['daemon_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert r[key]
assert 'vCPU 0 task exited (exits=7)' in (p/'baseline/daemon.txt').read_text() and 'TimeoutError' in (p/'baseline/stdout.txt').read_text()
s=json.loads((p/'source-context.json').read_text());assert s['candidate_runtime_inputs_sha256']==r['inputs_sha256'] and s['diagnostic_checker_sha256']==hashlib.sha256((p/'checker.py').read_bytes()).hexdigest()
assert s['inspected_vm_sha256']==hashlib.sha256((p/'vm.rs').read_bytes()).hexdigest()
assert not json.loads((p/'cleanup.json').read_text())['tracked_owned_processes_remaining']
print('Baseline sixth added resume timeout retained; candidate seven added plus original resume pass with exact UDP/full cleanup; diagnostic source/input hashes and post-terminal process cleanup verified.')
