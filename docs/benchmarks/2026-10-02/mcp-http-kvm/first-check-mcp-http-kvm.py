#!/usr/bin/env python3
"""Owned HTTPS MCP -> control-plane TLS -> node mTLS -> real KVM lifecycle."""
import argparse
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


def require(value, message):
    if not value: raise ValueError(message)


def digest(path):
    with path.open('rb') as stream: return hashlib.file_digest(stream, 'sha256').hexdigest()


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0)); return listener.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['daemon','control-plane','cli','kernel','initrd','output']: parser.add_argument('--'+name,type=Path,required=True)
    args = parser.parse_args(); require(not args.output.exists(),'preserve existing evidence')
    args.output.mkdir(parents=True); os.umask(0o077)
    paths = {name:getattr(args,name.replace('-','_')).resolve(strict=True) for name in ['daemon','control-plane','cli','kernel','initrd']}
    paths.update(coordinator=Path(__file__),client=Path(__file__).with_name('e2e-mcp-sandbox.py'),cancellation=Path(__file__).with_name('mcp_cancellation_check.py'))
    report = {'success':False,'performance_comparison':False,'transport':'official MCP client -> verified HTTPS fixture proxy -> MCP loopback -> verified control-plane TLS -> node mTLS -> KVM',
              'artifact_sha256':{name:digest(path) for name,path in paths.items()},'cleanup_errors':[],'processes_stopped':[],'checks':[]}
    processes=[]; bridge=None; thread=None
    api_key,cluster_token,mcp_token,observer = [secrets.token_urlsafe(32) for _ in range(4)]
    environment = {'PATH':'/usr/local/bin:/usr/bin:/bin','RUST_LOG':'warn'}
    with tempfile.TemporaryDirectory(prefix='hm-mcp-http-kvm-',dir='/var/tmp') as scratch:
        root=Path(scratch)
        def run(command): subprocess.run(command,check=True,capture_output=True,timeout=20)
        def start(name,command,env):
            with (args.output/(name+'.log')).open('wb') as log:
                process=subprocess.Popen(command,env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
            processes.append((name,process));return process
        def alive(): require(all(process.poll() is None for _,process in processes),'owned service exited')
        try:
            run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','2','-subj','/CN=HM owned MCP CA',
                 '-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign','-keyout',str(root/'ca.key'),'-out',str(root/'ca.pem')])
            for name,usage,san in [('node','serverAuth','DNS:mcp-node.test,IP:127.0.0.1'),('control','clientAuth','DNS:mcp-control.test'),('api','serverAuth','IP:127.0.0.1,DNS:localhost')]:
                run(['openssl','req','-newkey','rsa:2048','-nodes','-subj','/CN='+name,'-keyout',str(root/(name+'.key')),'-out',str(root/(name+'.csr'))])
                (root/(name+'.ext')).write_text('basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage='+usage+'\nsubjectAltName='+san+'\n')
                run(['openssl','x509','-req','-in',str(root/(name+'.csr')),'-CA',str(root/'ca.pem'),'-CAkey',str(root/'ca.key'),'-CAcreateserial','-days','2','-extfile',str(root/(name+'.ext')),'-out',str(root/(name+'.pem'))])
            context=ssl.create_default_context(cafile=str(root/'ca.pem'))
            ports=[]
            while len(ports)<6:
                value=port()
                if value not in ports:ports.append(value)
            redis_port,node_port,node_proxy,api_port,api_proxy,mcp_port=ports
            api_url=f'https://127.0.0.1:{api_port}';store=f'redis://127.0.0.1:{redis_port}';namespace='mcp-'+secrets.token_hex(12)
            start('redis',['redis-server','--bind','127.0.0.1','--port',str(redis_port),'--save','','--appendonly','no','--dir',str(root)],environment)
            deadline=time.monotonic()+10
            while True:
                alive()
                try:
                    with socket.create_connection(('127.0.0.1',redis_port),timeout=.2):break
                except OSError:
                    require(time.monotonic()<deadline,'Redis readiness timeout');time.sleep(.05)
            policies=root/'keys.json';policies.write_text(json.dumps([{'sha256':hashlib.sha256(observer.encode()).hexdigest(),'expires_at':int(time.time())+3600,'scopes':['inventory']}]))
            start('node',[str(paths['daemon']),'--port',str(node_port),'--proxy-port',str(node_proxy),'--memory-mb','1024','--cpu-cores','1','--capacity','4',
                  '--volume-dir',str(root/'volumes'),'--snapshot-store',str(root/'snapshots'),'--cluster-store',store,'--cluster-namespace',namespace,'--node-id','mcp-node',
                  '--advertise-api',f'https://127.0.0.1:{node_port}','--advertise-proxy',f'127.0.0.1:{node_proxy}','--mtls-ca',str(root/'ca.pem'),'--mtls-cert',str(root/'node.pem'),'--mtls-key',str(root/'node.key')],
                  dict(environment,HV2_KERNEL=str(paths['kernel']),HV2_INITRD=str(paths['initrd']),HV2_CLUSTER_TOKEN=cluster_token))
            start('control',[str(paths['control-plane']),'--store',store,'--namespace',namespace,'--port',str(api_port),'--proxy-port',str(api_proxy),'--api-keys-file',str(policies),
                  '--api-tls-cert',str(root/'api.pem'),'--api-tls-key',str(root/'api.key'),'--mtls-ca',str(root/'ca.pem'),'--mtls-cert',str(root/'control.pem'),'--mtls-key',str(root/'control.key'),'--mtls-node-name','mcp-node.test'],
                  dict(environment,HV2_API_KEY=api_key,HV2_CLUSTER_TOKEN=cluster_token))
            def api(method,path,key=api_key):
                request=urllib.request.Request(api_url+path,method=method,headers={'x-api-key':key})
                with urllib.request.urlopen(request,context=context,timeout=20) as response:return json.loads(response.read())
            deadline=time.monotonic()+60
            while True:
                alive()
                try:
                    if any('base' in row.get('aliases',[]) for row in api('GET','/templates')):break
                except OSError:pass
                require(time.monotonic()<deadline,'prepared template readiness timeout');time.sleep(.05)
            start('mcp',[str(paths['cli']),'sandbox','vm','--endpoint',api_url,'--api-ca-cert',str(root/'ca.pem'),'mcp-http','--listen',f'127.0.0.1:{mcp_port}','--allow-origin','https://agent.example.test'],
                  dict(environment,HV2_API_KEY=api_key,HM_MCP_TOKEN=mcp_token))
            deadline=time.monotonic()+10
            while True:
                alive()
                try:
                    with socket.create_connection(('127.0.0.1',mcp_port),timeout=.2):break
                except OSError:
                    require(time.monotonic()<deadline,'MCP listener readiness timeout');time.sleep(.05)
            class Proxy(BaseHTTPRequestHandler):
                def log_message(self,*args):pass
                def forward(self):
                    if self.path!='/mcp':self.send_error(404);return
                    length=int(self.headers.get('Content-Length','0'))
                    if length<0 or length>1024*1024:self.send_error(413);return
                    connection=http.client.HTTPConnection('127.0.0.1',mcp_port,timeout=150)
                    try:
                        headers={name:value for name,value in self.headers.items() if name.lower() not in ['host','connection','content-length']}
                        connection.request(self.command,'/mcp',self.rfile.read(length),headers)
                        response=connection.getresponse();body=response.read()
                        self.send_response(response.status)
                        for name,value in response.getheaders():
                            if name.lower() not in ['connection','content-length','transfer-encoding']:self.send_header(name,value)
                        self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                    finally:connection.close()
                do_POST=forward;do_GET=forward;do_DELETE=forward
            bridge=ThreadingHTTPServer(('127.0.0.1',0),Proxy);bridge.daemon_threads=True
            tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(root/'api.pem',root/'api.key');bridge.socket=tls.wrap_socket(bridge.socket,server_side=True)
            thread=threading.Thread(target=bridge.serve_forever,daemon=True);thread.start();url=f'https://127.0.0.1:{bridge.server_port}/mcp'
            def probe(headers):
                request=urllib.request.Request(url,method='POST',headers={'Content-Type':'application/json','Accept':'application/json, text/event-stream',**headers},data=b'{"jsonrpc":"2.0","id":1,"method":"ping"}')
                try:response=urllib.request.urlopen(request,context=context,timeout=5)
                except urllib.error.HTTPError as error:response=error
                with response:return response.status
            require(probe({})==401,'HTTPS missing credential accepted');report['checks'].append('https_authentication_refused')
            require(probe({'Authorization':'Bearer '+mcp_token,'Origin':'https://untrusted.example.test'})==403,'HTTPS invalid origin accepted');report['checks'].append('https_origin_refused')
            result=subprocess.run([sys.executable,str(paths['client']),'--binary',str(paths['cli']),'--api-url',api_url,'--environment','owned WSL KVM; verified MCP/API TLS and node mTLS','--http-url',url,'--http-ca',str(root/'ca.pem')],
                    env=dict(environment,HV2_API_KEY=api_key,HM_MCP_TOKEN=mcp_token),capture_output=True,timeout=180)
            require(result.returncode==0,'official MCP client failed: '+result.stderr.decode(errors='replace')[-2000:])
            client_report=json.loads(result.stdout);require(client_report['success'],'client lifecycle failed');report['client']=client_report;report['checks'].append('official_https_client_real_kvm_lifecycle')
            require(api('GET','/sandboxes')==[],'sandbox inventory not empty');report['checks'].append('control_plane_inventory_empty')
            report['success']=True
        except Exception as error:
            message=str(error)
            for secret in [api_key,cluster_token,mcp_token,observer]:message=message.replace(secret,'[redacted]')
            report['error']=message
        finally:
            if bridge:bridge.shutdown();bridge.server_close()
            if thread:thread.join(timeout=5);require(not thread.is_alive(),'TLS proxy thread did not stop')
            for name,process in reversed(processes):
                try:
                    if process.poll() is None:
                        process.terminate()
                        try:process.wait(timeout=10)
                        except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
                except Exception as error:report['cleanup_errors'].append(type(error).__name__)
                report['processes_stopped'].append({'name':name,'exit_code':process.poll()})
            report['artifacts_unchanged']=all(digest(path)==report['artifact_sha256'][name] for name,path in paths.items())
            report['success']=report['success'] and report['artifacts_unchanged'] and not report['cleanup_errors'] and all(row['exit_code']==0 for row in report['processes_stopped'])
            (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            print(json.dumps(report))
    return 0 if report['success'] else 1


if __name__=='__main__':raise SystemExit(main())
