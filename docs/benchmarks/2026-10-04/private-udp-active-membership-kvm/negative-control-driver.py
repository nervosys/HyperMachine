from pathlib import Path
import socket,threading,subprocess,selectors,json
output=Path('/var/tmp/hm-private-udp-live-negative-v1');output.mkdir()
marker=output/'revoked';sock=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);sock.bind(('127.0.0.1',0));sock.settimeout(.2);stop=threading.Event()
def echo():
 while not stop.is_set():
  try:data,peer=sock.recvfrom(65535);sock.sendto(data,peer)
  except socket.timeout:pass
thread=threading.Thread(target=echo);thread.start()
client=subprocess.Popen(['/var/tmp/hm-private-udp-live-client-v2','--live','127.0.0.1',str(sock.getsockname()[1]),str(marker)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
try:
 selector=selectors.DefaultSelector();selector.register(client.stdout,selectors.EVENT_READ)
 assert selector.select(3),'client did not become ready'
 ready=client.stdout.readline();assert json.loads(ready)=={'ready':True}
 marker.write_text('marker without any route removal')
 stdout,stderr=client.communicate(timeout=8)
 assert client.returncode==5 and not stdout.strip() and not stderr
 report={'ready':True,'marker_created_with_echo_still_active':True,'exit_code':client.returncode,'false_revocation_report':False,'client_reaped':client.poll() is not None}
 (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps(report))
finally:
 if client.poll() is None:client.kill();client.communicate()
 stop.set();thread.join(2);sock.close();assert not thread.is_alive()
