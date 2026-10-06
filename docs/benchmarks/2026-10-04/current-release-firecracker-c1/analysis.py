import json,math,statistics,sys
from pathlib import Path

def require(value,message):
    if not value:raise ValueError(message)

def analyze(path):
    j=json.loads(path.read_text());c=j['concurrency'];pairs=j['pairs']
    require(j['driver_cpu_affinity']==list(range(8)), 'affinity mismatch')
    require(j['cpu_count']==1 and j['memory_mb']==1024,'guest resource mismatch')
    require(j['memory_idle_seconds']==5 and j['cold_start_concurrency'] is None,'measurement configuration mismatch')
    require(j['hypermachine_template_preflight']['snapshot'] is False and '--no-template' in j['daemon_argv'],'benchmark did not confirm cold creation')
    require(j['artifacts_unchanged'] and j['remaining_sandbox_count']==0 and j['daemon_exit_code']==0 and not j['cleanup_errors'],'cleanup or identity failure')
    require(len(j['batches'])==pairs*2,'batch count mismatch')
    result={'concurrency':c,'pairs':pairs,'profile_success':j['success'],'engines':{}}
    for engine in ['hypermachine','firecracker']:
        batches=[b for b in j['batches'] if b['engine']==engine]
        require(sorted(b['pair'] for b in batches)==list(range(pairs)),'missing/duplicate pair')
        rows=[]
        for b in batches:
            require(b['concurrency']==c and sorted(x['index'] for x in b['samples'])==list(range(c)),'attempt coverage mismatch')
            rows.extend(b['samples'])
        passed=[x for x in rows if x['success'] and x['cleanup_success']]
        latency=sorted(x['ready_ms'] for x in passed)
        require(all(isinstance(x,(float,int)) and math.isfinite(x) and x>0 for x in latency),'invalid latency')
        require(len(latency)==j['ready_ms'][engine]['n'] if latency else j['ready_ms'][engine] is None,'summary count mismatch')
        good_batches=[b for b in batches if b['success'] and b['all_guests_validated_while_held']]
        held=[b['idle_process_memory_kib']['Pss_kib']/1024 for b in good_batches]
        incremental=[b['incremental_idle_process_memory_kib']['Pss_kib']/1024 for b in good_batches]
        require(all(math.isfinite(x) and x>=0 for x in held),'invalid PSS')
        result['engines'][engine]={'passed':len(passed),'attempted':len(rows),
          'ready_ms':({f'p{p}':latency[math.ceil(p/100*len(latency))-1] for p in [50,95,99]} if latency else None),
          'median_held_pss_mib':statistics.median(held) if held else None,
          'median_incremental_pss_mib':statistics.median(incremental) if incremental else None,
          'memory_batches':len(good_batches),'failures':[{'pair':x['pair'],'index':x['index'],'error':x.get('error'),'phase':x.get('failure_phase'),'cleanup_error':x.get('cleanup_error')} for x in rows if not(x['success'] and x['cleanup_success'])]}
        if latency:
            require(all(result['engines'][engine]['ready_ms'][f'p{p}']==j['ready_ms'][engine][f'p{p}'] for p in [50,95,99]),'quantile mismatch')
    return result

if __name__=='__main__':
    print(json.dumps([analyze(Path(p)) for p in sys.argv[1:]],indent=2))
