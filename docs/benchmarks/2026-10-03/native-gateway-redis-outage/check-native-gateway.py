#!/usr/bin/env python3
"""Owned native gateway/Redis/mTLS fixture; no guest or competitor claims."""
import argparse, hashlib, json, os, platform, signal, socket, ssl, subprocess, tempfile, threading, time
from pathlib import Path

def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def command(args): return subprocess.run(args, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout

def certs(root):
    command(['openssl','req','-x509','-newkey','ed25519','-nodes','-keyout',str(root/'ca.key'),'-out',str(root/'ca.pem'),'-subj','/CN=owned-native-root','-days','1','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign'])
    for number, name, usage in [(2,'node','serverAuth'),(3,'gateway','clientAuth')]:
        command(['openssl','req','-new','-newkey','ed25519','-nodes','-keyout',str(root/f'{name}.key'),'-out',str(root/f'{name}.csr'),'-subj',f'/CN={name}'])
        (root/f'{name}.ext').write_text(f'basicConstraints=critical,CA:FALSE\nextendedKeyUsage={usage}\nsubjectAltName=DNS:hv2-node\n')
        command(['openssl','x509','-req','-in',str(root/f'{name}.csr'),'-CA',str(root/'ca.pem'),'-CAkey',str(root/'ca.key'),'-set_serial',str(number),'-out',str(root/f'{name}.pem'),'-days','1','-extfile',str(root/f'{name}.ext')])
    for path in root.glob('*.key'): path.chmod(0o600)

class Node:
    def __init__(self, root, token):
        self.context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); self.context.load_cert_chain(root/'node.pem',root/'node.key'); self.context.load_verify_locations(root/'ca.pem'); self.context.verify_mode=ssl.CERT_REQUIRED; self.context.set_alpn_protocols(['http/1.1'])
        self.listener=socket.socket(); self.listener.bind(('127.0.0.1',0)); self.listener.listen(16); self.listener.settimeout(.2); self.address=self.listener.getsockname()
        self.token=token; self.stopping=threading.Event(); self.lock=threading.Lock(); self.sockets=[]; self.threads=[]; self.errors=[]; self.protocols=[]
        self.thread=threading.Thread(target=self.accept); self.thread.start()
    def accept(self):
        while not self.stopping.is_set():
            try: raw,_=self.listener.accept()
            except socket.timeout: continue
            except OSError: break
            thread=threading.Thread(target=self.serve,args=(raw,)); self.threads.append(thread); thread.start()
    def serve(self, raw):
        tls=None
        try:
            raw.settimeout(2); tls=self.context.wrap_socket(raw,server_side=True)
            with self.lock: self.sockets.append(tls)
            assert tls.selected_alpn_protocol()=='http/1.1', 'unexpected node ALPN'
            header=b''
            while b'\r\n\r\n' not in header:
                part=tls.recv(4096)
                if not part: return
                header+=part; assert len(header)<=8192, 'oversized fixture request'
            fields=header.split(b'\r\n'); request=fields[0].decode(); headers={key.lower():value.strip() for line in fields[1:] if b':' in line for key,value in [line.split(b':',1)]}
            kind=request.split(' ')[1].rsplit('/',1)[1]
            assert request==f'GET /sandboxes/owned-vm/ports/8080/{kind} HTTP/1.1' and kind in ('tcp','udp'), 'wrong fixture target'
            assert headers.get(b'x-hv2-cluster-token')==self.token.encode(), 'wrong fixture credential'
            assert b'x-api-key' not in headers, 'API credential leaked to node'
            protocol=f'hv2-{kind}/1'; assert headers.get(b'upgrade')==protocol.encode(), 'wrong fixture upgrade'
            with self.lock: self.protocols.append(kind)
            tls.sendall(f'HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: {protocol}\r\n\r\n'.encode())
            tls.settimeout(.2)
            while not self.stopping.is_set():
                try: data=tls.recv(65536)
                except socket.timeout: continue
                if not data: break
                tls.sendall(data)
        except (OSError,ssl.SSLError): pass
        except Exception as error:
            with self.lock: self.errors.append(type(error).__name__)
        finally:
            if tls is not None:
                with self.lock:
                    if tls in self.sockets: self.sockets.remove(tls)
                tls.close()
            raw.close()
    def close(self):
        self.stopping.set(); self.listener.close()
        with self.lock:
            for connection in self.sockets:
                try: connection.shutdown(socket.SHUT_RDWR)
                except OSError: pass
                connection.close()
        self.thread.join(3)
        for thread in self.threads: thread.join(3)
        assert not self.thread.is_alive() and all(not t.is_alive() for t in self.threads), 'owned node threads did not stop'

def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--gateway',required=True); parser.add_argument('--output',required=True); args=parser.parse_args()
    binary=Path(args.gateway).resolve(); output=Path(args.output).resolve(); output.mkdir(parents=True,exist_ok=False)
    hashes={'gateway':sha(binary),'checker':sha(__file__)}; checks=[]; gateway=None; redis=None; node=None; opened=[]; handles=[]
    with tempfile.TemporaryDirectory(prefix='hm-native-owned-') as temporary:
        root=Path(temporary); certs(root); token=os.urandom(24).hex(); namespace='owned-native'; sock=root/'redis.sock'; prefix=f'hv2:{namespace}'
        env=os.environ.copy(); env.update(HV2_STORE_URL=f'redis+unix://{sock}',HV2_CLUSTER_TOKEN=token)
        cli=[str(binary),'--bind-ip','127.0.0.1','--namespace',namespace,'--mtls-ca',str(root/'ca.pem'),'--mtls-cert',str(root/'gateway.pem'),'--mtls-key',str(root/'gateway.key'),'--max-ports','2','--max-sessions','4','--tcp-connections','2','--udp-peers','2','--poll-ms','20']
        def r(*values): return command(['redis-cli','-s',str(sock),'--raw',*map(str,values)]).decode().strip()
        def launch(index):
            nonlocal gateway
            handle=(output/f'gateway-{index}.log').open('wb'); handles.append(handle); gateway=subprocess.Popen(cli,env=env,stdout=handle,stderr=subprocess.STDOUT)
        def stop(signum):
            nonlocal gateway
            if gateway is not None:
                gateway.send_signal(signum); assert gateway.wait(timeout=8)==0, 'gateway did not stop gracefully'; gateway=None
        def connect(address):
            deadline=time.monotonic()+5
            while True:
                if gateway.poll() is not None: raise RuntimeError('owned gateway exited before binding')
                try: client=socket.create_connection(address,.3); client.settimeout(3); opened.append(client); return client
                except OSError:
                    if time.monotonic()>deadline: raise TimeoutError('gateway binding readiness')
                    time.sleep(.02)
        def echo_tcp(client,payload):
            errors=[]
            def send():
                try: client.sendall(payload)
                except OSError: errors.append('send failed')
            sender=threading.Thread(target=send); sender.start()
            received=b''
            try:
                while len(received)<len(payload):
                    data=client.recv(min(65536,len(payload)-len(received)))
                    if not data: raise EOFError('owned TCP stream ended')
                    received+=data
            finally: sender.join(5)
            assert not sender.is_alive() and not errors and received==payload, 'TCP payload mismatch'
        def echo_udp(peer,address,payload):
            deadline=time.monotonic()+5
            while True:
                peer.sendto(payload,address)
                try:
                    data,source=peer.recvfrom(65508)
                    assert source==address and data==payload, 'UDP payload mismatch'; return
                except socket.timeout:
                    if time.monotonic()>deadline: raise TimeoutError('UDP readiness/recovery')
        def closed(client):
            try: assert client.recv(1)==b'', 'stale TCP session remained open'
            except (ConnectionResetError,BrokenPipeError): pass
        def refused(address):
            deadline=time.monotonic()+5
            while True:
                try: probe=socket.create_connection(address,.1); probe.close()
                except OSError: return
                if time.monotonic()>deadline: raise TimeoutError('stale listener remained bound')
                time.sleep(.02)
        try:
            checked=subprocess.run(cli+['--check'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
            assert checked.returncode==0 and not sock.exists(), 'offline configuration check contacted store'; checks.append('offline configuration check without store')
            redis_log=(output/'redis.log').open('wb'); handles.append(redis_log)
            redis=subprocess.Popen(['redis-server','--port','0','--protected-mode','yes','--appendonly','yes','--appendfsync','always','--save','','--dir',str(root),'--unixsocket',str(sock),'--unixsocketperm','700'],stdout=redis_log,stderr=subprocess.STDOUT)
            deadline=time.monotonic()+5
            while True:
                try:
                    if sock.exists() and r('PING')=='PONG': break
                except subprocess.CalledProcessError: pass
                if redis.poll() is not None or time.monotonic()>deadline: raise TimeoutError('owned Redis readiness')
                time.sleep(.02)
            node=Node(root,token)
            probe=socket.socket(); probe.bind(('127.0.0.1',0)); address=probe.getsockname(); probe.close()
            now=int(time.time()*1000)
            node_row={'id':'node','api':f'https://127.0.0.1:{node.address[1]}','proxy':'127.0.0.1:1','capacity':1,'running':1,'heartbeat_ms':now,'version':'owned-fixture'}
            sandbox={'sandbox_id':'owned-vm','node_id':'node','template_id':'base','started_at_ms':now,'end_at_ms':now+120000,'cpu_count':1,'memory_mb':128,'metadata':{},'envd_version':'fixture','descriptor':{},'paused':False,'portable':False}
            allocation={'sandbox_id':'owned-vm','machine_port':8080,'public_port':address[1],'owner_id':'owned-principal','protocol':'both'}
            r('SET',prefix+':node:node',json.dumps(node_row),'EX',120); r('SADD',prefix+':nodes','node'); r('SET',prefix+':sandbox:owned-vm',json.dumps(sandbox)); r('SADD',prefix+':sandboxes','owned-vm')
            def reservation():
                encoded=json.dumps(allocation,separators=(',',':'))
                r('EVAL',"redis.call('HSET',KEYS[1],ARGV[1],ARGV[3]);redis.call('HSET',KEYS[2],ARGV[2],ARGV[3]);return 1",2,prefix+':public-ports',prefix+':ports:owned-vm',address[1],8080,encoded)
            reservation(); launch(1); client=connect(address); echo_tcp(client,bytes(range(256))*4096); checks.append('exact 1 MiB TCP via executable Redis and mutual TLS')
            peers=[]
            for _ in range(2):
                peer=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); peer.bind(('127.0.0.1',0)); peer.settimeout(.5); peers.append(peer); opened.append(peer)
                for payload in [b'',b'\0\xff\r\n',b'x'*65507]: echo_udp(peer,address,payload)
            checks.append('two UDP peers preserve empty binary and maximum datagrams')
            allocation['protocol']='udp'; reservation(); closed(client); refused(address); echo_udp(peers[0],address,b'protocol-update'); checks.append('same-port UDP update closes old TCP')
            allocation['protocol']='both'; reservation(); client=connect(address); echo_tcp(client,b'both-restored')
            sandbox['paused']=True; r('SET',prefix+':sandbox:owned-vm',json.dumps(sandbox)); closed(client); refused(address)
            assert json.loads(r('HGET',prefix+':public-ports',address[1]))['public_port']==address[1]; checks.append('pause releases listeners and retains reservation')
            sandbox['paused']=False; r('SET',prefix+':sandbox:owned-vm',json.dumps(sandbox)); client=connect(address); echo_tcp(client,b'resumed'); echo_udp(peers[0],address,b'resumed'); checks.append('resume rebinds same public port')
            r('DEL',prefix+':public-ports'); r('SET',prefix+':public-ports','owned-type-fault'); closed(client); refused(address)
            r('DEL',prefix+':public-ports'); reservation(); client=connect(address); echo_tcp(client,b'fault-recovered'); checks.append('corrupt Redis snapshot closes sessions and correction recovers')
            # Stop only the owned store, leaving the gateway/node processes live.
            client.settimeout(8); redis.terminate(); assert redis.wait(timeout=5)==0, 'owned Redis did not exit cleanly'
            closed(client); refused(address); assert gateway.poll() is None, 'gateway exited during store outage'
            checks.append('Redis process outage closes active sessions and listeners without gateway exit')
            restart_log=(output/'redis-restarted.log').open('wb'); handles.append(restart_log)
            redis=subprocess.Popen(['redis-server','--port','0','--protected-mode','yes','--appendonly','yes','--appendfsync','always','--save','','--dir',str(root),'--unixsocket',str(sock),'--unixsocketperm','700'],stdout=restart_log,stderr=subprocess.STDOUT)
            deadline=time.monotonic()+5
            while True:
                try:
                    if sock.exists() and r('PING')=='PONG': break
                except subprocess.CalledProcessError: pass
                if redis.poll() is not None or time.monotonic()>deadline: raise TimeoutError('owned Redis restart readiness')
                time.sleep(.02)
            assert json.loads(r('HGET',prefix+':public-ports',address[1]))==allocation, 'AOF reservation changed'
            client=connect(address); echo_tcp(client,b'store-restarted'); echo_udp(peers[0],address,b'store-restarted')
            checks.append('same gateway process recovers exact reserved port after owned AOF Redis restart')
            stop(signal.SIGTERM); closed(client); checks.append('SIGTERM graceful exit and live-session closure')
            launch(2); client=connect(address); echo_tcp(client,b'restarted'); echo_udp(peers[0],address,b'restarted'); checks.append('gateway process restart retains same reserved port')
            r('EVAL',"redis.call('DEL',KEYS[1],KEYS[2]);redis.call('SREM',KEYS[3],ARGV[1]);redis.call('HDEL',KEYS[4],ARGV[2]);return 1",4,prefix+':sandbox:owned-vm',prefix+':ports:owned-vm',prefix+':sandboxes',prefix+':public-ports','owned-vm',address[1])
            closed(client); refused(address); assert r('HLEN',prefix+':public-ports')=='0'; checks.append('owned sandbox/index removal closes gateway listeners')
            stop(signal.SIGINT); checks.append('SIGINT graceful exit')
            assert not node.errors and set(node.protocols)=={'tcp','udp'}, 'owned TLS node validation failed'
        finally:
            if gateway is not None and gateway.poll() is None: gateway.terminate(); gateway.wait(timeout=8)
            for client in opened: client.close()
            if node is not None: node.close()
            if redis is not None and redis.poll() is None: redis.terminate(); redis.wait(timeout=5)
            for handle in handles: handle.close()
        assert all(token not in path.read_text(errors='replace') for path in output.glob('*.log')), 'credential appeared in fixture logs'
    assert hashes=={'gateway':sha(binary),'checker':sha(__file__)}, 'fixture inputs changed during run'
    report={'checks':checks,'inputs_sha256':hashes,'environment':platform.platform(),'scope':'owned executable Redis mutual-TLS echo fixture; no KVM or competitor performance evidence','cleanup':{'gateway_reaped':True,'redis_reaped':True,'node_threads_joined':True,'private_fixture_directory_removed':True}}
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps(report,indent=2))
if __name__=='__main__': main()
