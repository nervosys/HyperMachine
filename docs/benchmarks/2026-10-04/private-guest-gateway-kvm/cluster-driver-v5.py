import os,socket,subprocess,tempfile,time,json
from pathlib import Path
s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close()
out=Path('/var/tmp/hm-private-gateway-contention-cluster-v5.txt')
report=Path('/var/tmp/hm-private-gateway-contention-cluster-v5.json')
with tempfile.TemporaryDirectory(prefix='hm-private-redis-') as directory:
    redis_log=open('/var/tmp/hm-private-gateway-contention-cluster-server-v5.txt','wb')
    redis=subprocess.Popen(['/usr/bin/redis-server','--bind','127.0.0.1','--port',str(port),'--save','','--appendonly','no','--dir',directory],stdout=redis_log,stderr=subprocess.STDOUT)
    try:
        deadline=time.monotonic()+5
        while True:
            assert redis.poll() is None,'Redis exited before readiness'
            try:
                with socket.create_connection(('127.0.0.1',port),.2):pass
                break
            except OSError:
                assert time.monotonic()<deadline,'Redis readiness timed out'
                time.sleep(.05)
        env=os.environ.copy();env['CARGO_TARGET_DIR']='/var/tmp/hm-object-backup/target';env['HV2_TEST_REDIS']=f'redis://127.0.0.1:{port}'
        command=['cargo','test','--locked','-p','hv2-cluster','--lib','--','--nocapture']
        with out.open('wb') as log:
            result=subprocess.run(command,cwd='/var/tmp/hm-egress-log-mA2CCL',env=env,stdout=log,stderr=subprocess.STDOUT,timeout=180)
        assert result.returncode==0,out.read_text()
        assert '116 passed; 0 failed' in out.read_text()
        assert out.read_text().count('private_route_contract completed')==2
        assert out.read_text().count('private_address_ledger_contract completed')==2
        assert out.read_text().count('private_source_router_contract completed')==2
        assert 'skipped: set HV2_TEST_REDIS' not in out.read_text()
    finally:
        redis.terminate()
        try:redis.wait(timeout=5)
        except subprocess.TimeoutExpired:redis.kill();redis.wait(timeout=5)
        redis_log.close()
        report.write_text(json.dumps({'owned_redis':True,'redis_exit_code':redis.returncode,'process_reaped':redis.poll() is not None,'command':command if 'command' in locals() else None,'test_exit_code':result.returncode if 'result' in locals() else None},indent=2)+'\n')
print(out.read_text())
print(report.read_text())
