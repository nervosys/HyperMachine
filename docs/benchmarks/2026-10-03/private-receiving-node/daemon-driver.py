import subprocess,os,hashlib,json,shutil
from pathlib import Path
iso=Path('/var/tmp/hm-egress-log-mA2CCL');env=os.environ.copy();env['CARGO_TARGET_DIR']='/var/tmp/hm-object-backup/target'
commands=[(['cargo','test','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'],'/var/tmp/hm-private-receiving-node-tests-final-v2.txt'),(['cargo','build','--locked','-p','hv2-sandboxd','--bin','hv2-sandboxd'],'/var/tmp/hm-private-receiving-node-build-v2.txt')]
for command,output in commands:
    with open(output,'wb') as log:result=subprocess.run(command,cwd=iso,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=240)
    assert result.returncode==0,Path(output).read_text()
    print(Path(output).read_text()[-1500:],flush=True)
assert '61 passed; 0 failed; 2 ignored' in Path(commands[0][1]).read_text()
source=Path('/var/tmp/hm-object-backup/target/debug/hv2-sandboxd');target=Path('/var/tmp/hm-private-receiving-node-v2');assert not target.exists();shutil.copy2(source,target)
report={'daemon_path':str(target),'daemon_sha256':hashlib.sha256(target.read_bytes()).hexdigest(),'commands':[c for c,p in commands],'daemon_passed':61,'daemon_failed':0,'daemon_ignored':2,'kvm_private_route_verified':False}
Path('/var/tmp/hm-private-receiving-node-inputs-v2.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report),flush=True)
