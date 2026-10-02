import hashlib, json, statistics
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine/docs/benchmarks/2026-10-01')
result={'diagnostic_only':True,'profiles':[],'production_defaults_changed':False,'latency_or_competitor_win_established':False}
for raw,source,setting in [('memory-retention-default-c8.json','diagnose-memory-retention-default.py',None),('memory-retention-arena2-c8.json','diagnose-memory-retention-arena.py',2)]:
 r=json.loads((root/raw).read_bytes())
 assert r['success'] and r['owned_daemon_reaped'] and r['artifacts_unchanged'] and not r['cleanup_errors']
 assert r['remaining_sandbox_count']==0
 assert hashlib.sha256((root/source).read_bytes()).hexdigest()==r['artifact_sha256']['diagnostic']
 rows=[row for batch in r['batches'] for row in batch['samples']]
 assert len(rows)==24 and all(row['success'] and row['cleanup_success'] for row in rows)
 after=[s for s in r['snapshots'] if s['stage']=='after-cleanup']
 assert len(after)==3
 assert all(s['kvm_vm_handles']==0 and s['kvm_vcpu_handles']==0 and s['large_mapping_count']==0 for s in after)
 result['profiles'].append({'allocator_arena_max':setting,'raw_file':raw,'raw_sha256':hashlib.sha256((root/raw).read_bytes()).hexdigest(),'source_file':source,'source_sha256':r['artifact_sha256']['diagnostic'],'attempts':24,'passed':24,'post_cleanup_pss_mib':[s['process_memory_kib']['Pss_kib']/1024 for s in after],'median_post_cleanup_pss_mib':statistics.median(s['process_memory_kib']['Pss_kib']/1024 for s in after),'post_cleanup_anonymous_pss_mib':[s['mapping_categories']['anonymous']['Pss_kib']/1024 for s in after],'post_cleanup_thread_count':[s['thread_count'] for s in after]})
result['limitations']=['Two sequential diagnostic runs, not counterbalanced','No pinned CPU load worker; eight-CPU affinity and shared nested KVM','Arena ownership inferred from mapping sensitivity, not proven allocation stacks','No matched latency, tail, snapshot, fork or density validation']
(root/'memory-retention-summary.json').write_text(json.dumps(result,indent=2)+'\n')
print('48 passing attempts, exact diagnostic source hashes, mapping teardown and cleanup verified.')