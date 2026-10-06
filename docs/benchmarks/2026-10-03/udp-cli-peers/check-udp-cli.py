#!/usr/bin/env python3
"""Check shipped CLI UDP peer isolation against an owned HTTP upgrade fixture."""
import argparse,json,os,signal,socket,subprocess,tempfile,threading,select
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cli',required=True)
args=parser.parse_args()
seen=[]
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self):
  assert self.headers.get('X-Api-Key')=='owned-udp-key'
  assert self.headers.get('Upgrade')=='hv2-udp/1'
  assert self.path=='/sandboxes/owned/ports/5353/udp'
  seen.append(self.client_address)
  self.send_response(101);self.send_header('Connection','upgrade');self.send_header('Upgrade','hv2-udp/1');self.end_headers()
  self.connection.settimeout(5)
  try:
   while True:
    prefix=self.rfile.read(2)
    if not prefix:break
    assert len(prefix)==2
    size=int.from_bytes(prefix,'big');assert size<=65507
    body=self.rfile.read(size);assert len(body)==size
    self.wfile.write(prefix+body);self.wfile.flush()
  except (OSError,ValueError):pass
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=False
thread=threading.Thread(target=server.serve_forever);thread.start()
process=None
try:
 with tempfile.TemporaryFile() as error:
  command=[args.cli,'sandbox','vm','--endpoint',f'http://127.0.0.1:{server.server_port}','udp','owned','--port','5353','--max-peers','2']
  process=subprocess.Popen(command,env=dict(os.environ,HV2_API_KEY='owned-udp-key'),stdout=subprocess.PIPE,stderr=error)
  assert select.select([process.stdout],[],[],5)[0],'missing listener announcement'
  ready=json.loads(process.stdout.readline());host,port=ready['listen'].rsplit(':',1);address=(host,int(port))
  peers=[socket.socket(socket.AF_INET,socket.SOCK_DGRAM) for _ in range(2)]
  try:
   for peer in peers:peer.bind(('127.0.0.1',0));peer.settimeout(5)
   for size in [0,4,65507]:
    messages=[bytes([n+1])*size for n in range(2)]
    for peer,message in zip(peers,messages):peer.sendto(message,address)
    for peer,message in zip(peers,messages):assert peer.recvfrom(65508)[0]==message,'peer payload mismatch'
   assert len(seen)==2,'peers did not receive separate sessions'
   third=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);third.settimeout(.3)
   try:
    third.sendto(b'limit',address)
    try:third.recvfrom(32);raise AssertionError('peer cap exceeded')
    except socket.timeout:pass
   finally:third.close()
  finally:
   for peer in peers:peer.close()
  process.send_signal(signal.SIGINT);assert process.wait(timeout=5)==0
  print(json.dumps({'peers':2,'sessions':len(seen),'payload_sizes':[0,4,65507],'peer_limit_refusal':True,'cli_reaped':True}))
finally:
 if process is not None and process.poll() is None:process.kill();process.wait(timeout=5)
 server.shutdown();server.server_close();thread.join(timeout=5);assert not thread.is_alive()
