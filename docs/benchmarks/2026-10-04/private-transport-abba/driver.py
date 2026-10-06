from pathlib import Path
import subprocess,json,hashlib
root=Path('/mnt/c/Users/user/dev/nervosys/os/HyperMachine');base=Path('/var/tmp/hm-private-transport-abba-v1');base.mkdir(exist_ok=False)
checker=root/'tools/check-udp-cluster-kvm.py';checker_hash=hashlib.sha256(checker.read_bytes()).hexdigest();results=[]
for index,kind in enumerate(['baseline','candidate','candidate','baseline']):
 binary='/var/tmp/hm-private-guest-gateway-node-v2' if kind=='baseline' else '/var/tmp/hm-private-transport-parallel-dev-node-v1'
 output=base/(str(index)+'-'+kind)
 command=['python3',str(checker),'--daemon',binary,'--control-plane','/var/tmp/hm-private-receiving-control-v1','--cli','/var/tmp/hm-discovery-cli-v1','--kernel','/var/tmp/hm-competitive/bzImage-known-uart-irq','--initrd','/var/tmp/hm-tcp-capacity-guest-v1.cpio.gz','--native-gateway','/var/tmp/hm-owner-adoption-gateway-v3','--output',str(output),'--tls','--mtls','--owner-context','--owner-port-api','--private-receiving-tcp','--private-guest-tcp','--private-cross-node','--private-guest-revocation','--private-transport-samples','32']
 assert hashlib.sha256(checker.read_bytes()).hexdigest()==checker_hash
 with output.with_suffix('.stdout').open('x') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=240)
 assert r.returncode==0,output.with_suffix('.stdout').read_text()[-4000:]
 report=json.loads((output/'report.json').read_text());assert len(report['checks'])==32 and report['guests_remaining']==0
 for key in ['daemon_reaped','secondary_node_reaped','control_and_redis_reaped','cli_reaped','native_gateway_reaped']:assert report[key]
 for path,digest in report['inputs_sha256'].items():assert hashlib.sha256(Path(path).read_bytes()).hexdigest()==digest,path
 rows=report['private_transport_benchmark']['rows'];assert len(rows)==136 and all(row['success'] for row in rows)
 assert rows==[json.loads(x) for x in (output/'private-transport-rows.jsonl').read_text().splitlines()]
 results.append({'index':index,'kind':kind,'report_path':str(output/'report.json'),'binary_sha256':hashlib.sha256(Path(binary).read_bytes()).hexdigest(),'summary':report['private_transport_benchmark']['summary']})
 (base/'cohorts.json').write_text(json.dumps(results,indent=2)+'\n')
 print('Completed',index,kind,'32 checks, 128 scored successes, complete cleanup',flush=True)
print(json.dumps(results,indent=2))
