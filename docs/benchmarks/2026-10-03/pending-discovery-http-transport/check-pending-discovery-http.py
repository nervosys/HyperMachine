#!/usr/bin/env python3
"""Check discovery response boundaries against an owned HTTP node and Redis."""
import argparse,hashlib,json,os,secrets,socket,subprocess,tempfile,threading,time
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import urllib.request,urllib.error

def port():
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));return sock.getsockname()[1]

def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--control',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
 args=parser.parse_args();assert not args.output.exists()
 admin,cluster=secrets.token_hex(32),secrets.token_hex(32)
 response=[200,b'{}'];observations=[]
 class Node(BaseHTTPRequestHandler):
  protocol_version='HTTP/1.1'
  def log_message(self,*args):pass
  def do_GET(self):
   observations.append({'cluster_authenticated':self.headers.get('x-hv2-cluster-token')==cluster,'client_key_absent':'x-api-key' not in self.headers})
   mode=response[2] if len(response)>2 else 'normal'
   self.send_response(response[0]);self.send_header('Content-Type','application/json');self.send_header('Connection','close')
   if mode in ('chunked','bad-chunk'):self.send_header('Transfer-Encoding','chunked')
   else:self.send_header('Content-Length',str(len(response[1])+(10 if mode=='truncated' else 0)))
   self.end_headers();self.close_connection=True
   try:
    if mode=='chunked':
     for offset in range(0,len(response[1]),97):
      chunk=response[1][offset:offset+97]
      self.wfile.write(f'{len(chunk):x}\r\n'.encode()+chunk+b'\r\n')
     self.wfile.write(b'0\r\n\r\n')
    elif mode=='bad-chunk':self.wfile.write(b'zz\r\n')
    else:self.wfile.write(response[1])
   except (BrokenPipeError,ConnectionResetError):pass
 server=ThreadingHTTPServer(('127.0.0.1',0),Node);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
 control=redis=None;checks=[];rp,cp=port(),port();namespace=secrets.token_hex(8)
 opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 def request(path,key=admin):
  try:
   with opener.open(urllib.request.Request(f'http://127.0.0.1:{cp}'+path,headers={'X-Api-Key':key}),timeout=5) as result:return result.status,result.read()
  except urllib.error.HTTPError as error:return error.code,error.read()
 with tempfile.TemporaryDirectory(prefix='hm-discovery-http-') as temporary:
  with (Path(temporary)/'process.log').open('wb') as log:
   try:
    redis=subprocess.Popen(['redis-server','--bind','127.0.0.1','--port',str(rp),'--save','','--appendonly','no'],stdout=log,stderr=log)
    deadline=time.monotonic()+10
    while True:
     try:
      with socket.create_connection(('127.0.0.1',rp),timeout=.2):break
     except OSError:
      assert redis.poll() is None and time.monotonic()<deadline;time.sleep(.02)
    node={'id':'fixture-node','api':f'http://127.0.0.1:{server.server_port}','proxy':'127.0.0.1:1','capacity':1,'running':0,'heartbeat_ms':int(time.time()*1000),'version':'fixture'}
    subprocess.run(['redis-cli','-p',str(rp),'SET',f'hv2:{namespace}:node:fixture-node',json.dumps(node)],check=True,capture_output=True)
    control=subprocess.Popen([str(args.control),'--store',f'redis://127.0.0.1:{rp}','--namespace',namespace,'--port',str(cp),'--proxy-port',str(port())],env=dict(os.environ,HV2_API_KEY=admin,HV2_CLUSTER_TOKEN=cluster),stdout=log,stderr=log)
    deadline=time.monotonic()+10
    while True:
     try:
      if request('/cluster/nodes')[0]==200:break
     except OSError:pass
     assert control.poll() is None and time.monotonic()<deadline;time.sleep(.02)
    path='/cluster/nodes/fixture-node/registrations/pending'
    single={'registrations':[{'sandboxID':'sbx-001','kind':'unnamed'}],'nextCursor':None}
    cases=[('empty',{'registrations':[],'nextCursor':None},200),('single',single,200),
     ('capability',dict(single,envdAccessToken='fixture-capability-marker'),502),
     ('row-capability',{'registrations':[dict(single['registrations'][0],envdAccessToken='fixture-capability-marker')],'nextCursor':None},502),
     ('missing-cursor',{'registrations':[]},502),('wrong-kind',{'registrations':[{'sandboxID':'sbx-001','kind':'unknown'}],'nextCursor':None},502),
     ('duplicate',{'registrations':single['registrations']*2,'nextCursor':None},502),
     ('too-many',{'registrations':[{'sandboxID':f'sbx-{i:03}','kind':'unnamed'} for i in range(33)],'nextCursor':None},502),
     ('short-cursor',dict(single,nextCursor='sbx-001'),502),('invalid-json',b'{',502),('oversized',b' '*20000,502)]
    for name,payload,expected in cases:
     response[:]=[200,payload if isinstance(payload,bytes) else json.dumps(payload).encode()]
     status,body=request(path);assert status==expected,(name,status)
     assert b'fixture-capability-marker' not in body
     checks.append({'case':name,'status':status})
    response[:]=[200,json.dumps(single).encode()]
    assert request(path+'?after=sbx-001')[0]==502;checks.append({'case':'exclusive-cursor','status':502})
    for name,mode,payload,expected in [('chunked-valid','chunked',json.dumps(single).encode(),200),
      ('chunked-oversized','chunked',b' '*20000,502),('truncated-body','truncated',json.dumps(single).encode(),502),
      ('invalid-chunk','bad-chunk',b'',502)]:
     response[:]=[200,payload,mode]
     status,body=request(path);assert status==expected,(name,status)
     checks.append({'case':name,'status':status})
    response[:]=[200,json.dumps(single).encode()]
    before=len(observations);assert request(path,'wrong-key')[0]==401;assert len(observations)==before
    checks.append({'case':'unauthorized-no-node-call','status':401})
    assert all(row['cluster_authenticated'] and row['client_key_absent'] for row in observations)
   finally:
    for process in (control,redis):
     if process is not None:
      process.terminate()
      try:process.wait(timeout=10)
      except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
    server.shutdown();server.server_close();thread.join(timeout=5);assert not thread.is_alive()
 report={'checks':checks,'node_calls':len(observations),'authenticated_node_requests':True,'client_keys_not_forwarded':True,'processes_reaped':True,'control_sha256':hashlib.sha256(args.control.read_bytes()).hexdigest(),'checker_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'scope':'Owned loopback HTTP/Redis response-boundary fixture, not KVM/TLS or competitor performance'}
 with args.output.open('x') as stream:json.dump(report,stream,indent=2);stream.write('\n')
 print(json.dumps(report,indent=2))
if __name__=='__main__':main()
