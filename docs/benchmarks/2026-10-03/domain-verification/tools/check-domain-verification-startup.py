#!/usr/bin/env python3
"""Verify invalid DNS ownership policies refuse actual control-plane startup."""
import argparse
import hashlib
import json
import secrets
import socket
import subprocess
import tempfile
from pathlib import Path

def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def port():
    with socket.socket() as stream:
        stream.bind(('127.0.0.1',0));return stream.getsockname()[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--control-plane',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists(): raise ValueError('preserve previous evidence')
    binary=args.control_plane.resolve(strict=True)
    inputs={'control_plane':binary,'checker':Path(__file__).resolve()}
    report={'functional_only':True,'success':False,'checks':[],
            'artifact_sha256':{name:digest(path) for name,path in inputs.items()}}
    with tempfile.TemporaryDirectory(prefix='hm-dns-startup-') as scratch:
        root=Path(scratch);secret=secrets.token_hex(32)
        valid={'namespace':'fixture','resolver_url':'https://resolver.example.test/dns-query','secret_hex':secret}
        cases=[('missing-file',None,'could not read DNS verification policy'),
               ('invalid-json','not json','invalid DNS verification policy'),
               ('oversized-policy','x'*4097,'exceeds 4096 bytes'),
               ('short-key',json.dumps(dict(valid,secret_hex=secret[:-2])),'64 hex characters'),
               ('plaintext-resolver',json.dumps(dict(valid,resolver_url='http://resolver.example.test/dns-query')),'plain HTTPS'),
               ('namespace-mismatch',json.dumps(dict(valid,namespace='other')),'namespace must match'),
               ('missing-resolver-ca',json.dumps(dict(valid,resolver_ca_file=str(root/'missing-ca.pem'))),'could not read DNS resolver CA')]
        for name,raw,expected in cases:
            policy=root/(name+'.json')
            if raw is not None: policy.write_text(raw)
            command=[str(binary),'--store','memory:','--namespace','fixture','--port',str(port()),
                     '--proxy-port',str(port()),'--domain-verification-file',str(policy)]
            result=subprocess.run(command,env={'PATH':'/usr/local/bin:/usr/bin:/bin','RUST_LOG':'warn'},
                                  stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=10)
            output=result.stdout.decode(errors='replace')
            if result.returncode==0 or expected not in output or secret in output:
                raise ValueError(name+': startup refusal or credential redaction failed')
            report['checks'].append({'case':name,'exit_code':result.returncode,'expected_error_observed':True,'secret_not_printed':True})
    report['artifacts_unchanged']=all(digest(path)==report['artifact_sha256'][name] for name,path in inputs.items())
    report['success']=report['artifacts_unchanged'] and len(report['checks'])==7
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'success':report['success'],'startup_refusals':len(report['checks'])}))
    return 0 if report['success'] else 1

if __name__=='__main__': raise SystemExit(main())
