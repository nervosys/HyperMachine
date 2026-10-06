#!/usr/bin/env python3
"""Owned local KVM owner-sharing functional gate; no performance comparison."""
import argparse, base64, hashlib, http.client, json, os, shlex, signal, socket, ssl, subprocess, tempfile, time, uuid
from pathlib import Path

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0)); return sock.getsockname()[1]
def stop(process):
    if process.poll() is None:
        process.terminate()
        try: process.wait(timeout=10)
        except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
    return process.poll() is not None

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ["daemon","control","cli","kernel","initrd","output"]: parser.add_argument("--"+name,type=Path,required=True)
    parser.add_argument("--redis-outage",action="store_true",help="verify fail-closed admission and AOF recovery during live KVM traffic")
    args=parser.parse_args(); args.output.mkdir(parents=True,exist_ok=False)
    inputs={name:getattr(args,name) for name in ["daemon","control","cli","kernel","initrd"]};inputs["driver"]=Path(__file__)
    report={"success":False,"purpose":"local KVM functional verification; no performance comparison","input_sha256":{k:digest(v) for k,v in inputs.items()},"cases":[],"cleanup_errors":[],"redis_outage_requested":args.redis_outage}
    processes=[]; handles=[]; guests=set(); api_url=None; context=None
    owner_key,other_key,legacy_key,token,secret=[uuid.uuid4().hex for _ in range(5)]
    env={"PATH":"/usr/local/bin:/usr/bin:/bin","RUST_LOG":"warn"}
    def case(name,action):
        row={"name":name,"success":False};report["cases"].append(row)
        action();row["success"]=True
    def api(method,path,body=None,key=None,status=200):
        conn=http.client.HTTPSConnection("127.0.0.1",api_port,context=context,timeout=40)
        try:
            conn.request(method,path,body=None if body is None else json.dumps(body),headers={"x-api-key":key or owner_key,"content-type":"application/json"})
            response=conn.getresponse();data=response.read(65537)
            assert len(data)<=65536,"API response limit exceeded"
            assert response.status==status,f"API {method} status={response.status} expected={status}"
            return json.loads(data) if data else None
        finally:conn.close()
    def command(guest,cmd):
        value=api("POST",f"/sandboxes/{guest}/exec",{"cmd":cmd,"timeout_secs":10})
        assert value.get("exit_code")==0 and not value.get("timed_out") and not value.get("truncated"),"guest command failed"
        return value["stdout"]
    def web(guest,expected=200,alias=None,password=None):
        conn=http.client.HTTPSConnection("127.0.0.1",proxy_port,context=context,timeout=20)
        headers={"host":alias or f"18084-{guest}.sandbox.test","x-hypermachine-user":"forged"}
        if password is not False:headers["authorization"]="Basic "+base64.b64encode(("alice:"+(secret if password is None else password)).encode()).decode()
        try:
            conn.request("GET","/owned-sharing",headers=headers);response=conn.getresponse();body=response.read(65537)
            assert len(body)<=65536,"guest response limit exceeded"
            assert response.status==expected,f"web status={response.status} expected={expected}"
            if expected==200:
                assert response.getheader("cache-control")=="private, no-store","guest cache policy"
                lines=body.lower().splitlines()
                assert [v.strip() for v in lines if v.startswith(b"x-hypermachine-user:")]==[b"x-hypermachine-user: alice"],"guest identity mismatch"
                assert not any(v.startswith(b"authorization:") for v in lines) and secret.encode() not in body and b"forged" not in body,"guest credential/identity leak"
            else:assert (response.getheader("www-authenticate") or "").startswith("Basic "),"challenge absent"
        finally:conn.close()
    def eventually(action,seconds=20):
        deadline=time.monotonic()+seconds
        while True:
            try:return action()
            except ssl.SSLCertVerificationError: raise
            except (OSError,AssertionError):
                if time.monotonic()>=deadline:raise
                time.sleep(.05)
    with tempfile.TemporaryDirectory(prefix="hm-owner-sharing-kvm-",dir="/var/tmp") as temporary:
        directory=Path(temporary);report["owned_directory"]=temporary
        def start(name,argv,environment=env):
            handle=(args.output/(name+".log")).open("wb");handles.append(handle)
            child=subprocess.Popen(argv,env=environment,stdin=subprocess.DEVNULL,stdout=handle,stderr=subprocess.STDOUT);processes.append((name,child));return child
        def run(argv):subprocess.run(argv,check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=20)
        def cli(command_args):
            result=subprocess.run([str(args.cli),"sandbox","vm","--endpoint",f"https://127.0.0.1:{api_port}","--api-ca-cert",str(directory/"ca.pem"),"--request-timeout","30"]+command_args,env={**env,"HV2_API_KEY":owner_key},capture_output=True,timeout=40)
            assert result.returncode==0,"sharing CLI failed"
            return json.loads(result.stdout)
        try:
            run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","2","-subj","/CN=owned-sharing-ca","-addext","basicConstraints=critical,CA:TRUE","-addext","keyUsage=critical,keyCertSign,cRLSign","-keyout",str(directory/"ca.key"),"-out",str(directory/"ca.pem")])
            for name,usage,san in [("node","serverAuth","DNS:tcp-node.test,IP:127.0.0.1"),("control","clientAuth","DNS:tcp-control.test"),("api","serverAuth","IP:127.0.0.1,DNS:localhost")]:
                run(["openssl","req","-newkey","rsa:2048","-nodes","-subj","/CN="+name,"-keyout",str(directory/(name+".key")),"-out",str(directory/(name+".csr"))])
                ext=directory/(name+".ext");ext.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage={usage}\nsubjectAltName={san}\n")
                run(["openssl","x509","-req","-in",str(directory/(name+".csr")),"-CA",str(directory/"ca.pem"),"-CAkey",str(directory/"ca.key"),"-CAcreateserial","-days","2","-extfile",str(ext),"-out",str(directory/(name+".pem"))])
            context=ssl.create_default_context(cafile=str(directory/"ca.pem"))
            node_context=ssl.create_default_context(cafile=str(directory/"ca.pem"))
            node_context.load_cert_chain(str(directory/"control.pem"),str(directory/"control.key"))
            ports=set()
            while len(ports)<5:ports.add(port())
            redis_port,node_port,node_proxy,api_port,proxy_port=list(ports);api_url=f"https://127.0.0.1:{api_port}"
            store=f"redis://127.0.0.1:{redis_port}";namespace="owner-sharing-"+uuid.uuid4().hex
            redis_args=["redis-server","--bind","127.0.0.1","--port",str(redis_port),"--save","","--appendonly","yes","--appendfsync","always","--dir",str(directory)]
            redis=start("redis",redis_args)
            def redis_ready():
                with socket.create_connection(("127.0.0.1",redis_port),timeout=.2):pass
            eventually(redis_ready,5)
            expiry=int(time.time())+1800
            policies=directory/"keys.json";policies.write_text(json.dumps([{"sha256":hashlib.sha256(key.encode()).hexdigest(),"expires_at":expiry,"scopes":["sandboxes"],"principal_id":owner} for key,owner in [(owner_key,"owner-a"),(other_key,"owner-b")]]))
            policy=directory/"web.json"
            def policy_write(delegation=True,expiry_value=expiry):
                policy.write_text(json.dumps([{"subject":"alice","sha256":hashlib.sha256(secret.encode()).hexdigest(),"expires_at":expiry_value,"sandboxes":[],"allow_owner_grants":delegation}]))
            policy_write()
            start("node",[str(args.daemon),"--port",str(node_port),"--proxy-port",str(node_proxy),"--memory-mb","1024","--cpu-cores","1","--capacity","4","--volume-dir",str(directory/"volumes"),"--snapshot-store",str(directory/"snapshots"),"--cluster-store",store,"--cluster-namespace",namespace,"--node-id","sharing-kvm-node","--advertise-api",f"https://127.0.0.1:{node_port}","--advertise-proxy",f"127.0.0.1:{node_proxy}","--mtls-ca",str(directory/"ca.pem"),"--mtls-cert",str(directory/"node.pem"),"--mtls-key",str(directory/"node.key")],{**env,"HV2_KERNEL":str(args.kernel),"HV2_INITRD":str(args.initrd),"HV2_CLUSTER_TOKEN":token})
            control_args=[str(args.control),"--store",store,"--namespace",namespace,"--port",str(api_port),"--proxy-port",str(proxy_port),"--api-keys-file",str(policies),"--api-tls-cert",str(directory/"api.pem"),"--api-tls-key",str(directory/"api.key"),"--web-access-file",str(policy),"--tls-cert",str(directory/"api.pem"),"--tls-key",str(directory/"api.key"),"--mtls-ca",str(directory/"ca.pem"),"--mtls-cert",str(directory/"control.pem"),"--mtls-key",str(directory/"control.key"),"--mtls-node-name","tcp-node.test"]
            control_env={**env,"HV2_API_KEY":legacy_key,"HV2_CLUSTER_TOKEN":token}
            control=start("control",control_args,control_env)
            def prepared():
                assert all(child.poll() is None for _,child in processes),"owned service exited"
                assert any("base" in row.get("aliases",[]) for row in api("GET","/templates",key=legacy_key)),"template not ready"
            eventually(prepared,60)
            created=api("POST","/v2/sandboxes",{"templateID":"base","timeout":600,"allowInternetAccess":False,"autoResume":{"enabled":True}},status=201)
            guest=created["sandboxID"];guests.add(guest)
            handler='#!/bin/sh\nfile=/tmp/owned-request-$$\n: > "$file"\nwhile IFS= read -r line; do\n [ "$line" = "$(printf \'\\r\')" ] && break\n printf \'%s\\n\' "$line" >> "$file"\ndone\nlength=$(wc -c < "$file")\nprintf \'HTTP/1.1 200 OK\\r\\nContent-Type: text/plain\\r\\nContent-Length: %s\\r\\nConnection: close\\r\\n\\r\\n\' "$length"\ncat "$file"\nrm -f "$file"\n'
            encoded=base64.b64encode(handler.encode()).decode()
            command(guest,f"printf %s {shlex.quote(encoded)} | busybox base64 -d > /tmp/owned-handler; chmod 700 /tmp/owned-handler; (while :; do busybox nc -l -p 18084 -e /tmp/owned-handler; done) >/tmp/owned-web.log 2>&1 &")
            path=f"/sandboxes/{guest}/web-sharing";request={"expectedRevision":None,"revision":str(uuid.uuid4()),"grants":[{"subject":"alice","expires_at":expiry}]}
            def unauthorized():
                for key in [other_key,legacy_key]:api("PUT",path,request,key=key,status=403)
                web(guest,401);web(guest,401,password=False);web(guest,401,password="wrong")
            case("owner-permissions-and-no-grant-denial",unauthorized)
            request_file=directory/"request.json";request_file.write_text(json.dumps(request))
            def grant_cli():
                for _ in range(2):assert cli(["web-sharing","replace",guest,"--request",str(request_file)])["revision"]==request["revision"]
                assert cli(["web-sharing","show",guest])["grants"]==request["grants"]
                eventually(lambda:web(guest))
            case("owner-cli-exact-retry-and-real-guest-identity",grant_cli)
            alias="owned-sharing.example.test";api("PUT",f"/sandboxes/{guest}/domains/{alias}",{"port":18084})
            case("custom-domain-uses-stored-grant",lambda:web(guest,alias=alias))
            forks=api("POST",f"/sandboxes/{guest}/fork",{"count":1,"timeout":600},status=201);child=forks[0]["sandbox"]["sandboxID"];guests.add(child)
            case("fork-does-not-inherit-parent-grant",lambda:web(child,401))
            def restart_control():
                assert stop(control)
                replacement=start("control-restarted",control_args,control_env)
                eventually(lambda:api("GET",path));eventually(lambda:web(guest));assert api("GET",path)["revision"]==request["revision"]
                return replacement
            row={"name":"shipped-control-plane-restart-preserves-active-grant","success":False};report["cases"].append(row);control=restart_control();row["success"]=True
            api("POST",f"/sandboxes/{guest}/pause",{},status=204)
            revoked_revision=str(uuid.uuid4())
            def revoke_paused():
                result=cli(["web-sharing","revoke",guest,"--expected-revision",request["revision"],"--revision",revoked_revision]);assert result["grants"]==[]
                web(guest,401);assert api("GET",f"/sandboxes/{guest}")["state"]=="paused","denied browser resumed guest"
                api("PUT",path,request,status=409)
            case("owner-revocation-does-not-wake-paused-guest",revoke_paused)
            def restart_revoked():
                nonlocal control
                assert stop(control);control=start("control-revoked-restarted",control_args,control_env)
                eventually(lambda:api("GET",path));web(guest,401)
                assert api("GET",path)["revision"]==revoked_revision
                assert api("GET",f"/sandboxes/{guest}")["state"]=="paused"
            case("shipped-control-plane-restart-preserves-revocation",restart_revoked)
            request={"expectedRevision":revoked_revision,"revision":str(uuid.uuid4()),"grants":[{"subject":"alice","expires_at":expiry}]}
            def regrant_resume():
                api("PUT",path,request);eventually(lambda:web(guest));assert api("GET",f"/sandboxes/{guest}")["state"]=="running"
            case("current-regrant-allows-auto-resume",regrant_resume)
            if args.redis_outage:
                def local_state():
                    conn=http.client.HTTPSConnection("127.0.0.1",node_port,context=node_context,timeout=10)
                    try:
                        conn.request("GET",f"/sandboxes/{guest}",headers={"x-hv2-cluster-token":token})
                        response=conn.getresponse();data=response.read(65537)
                        assert response.status==200 and len(data)<=65536,"local node detail refused"
                        return json.loads(data)["state"]
                    finally:conn.close()
                def kill_redis():
                    assert redis.poll() is None,"owned Redis not running"
                    redis.kill();redis.wait(timeout=10)
                def paused_outage():
                    api("POST",f"/sandboxes/{guest}/pause",{},status=204)
                    assert local_state()=="paused"
                    kill_redis()
                    timings=[]
                    for _ in range(2):
                        begin=time.monotonic();web(guest,401);elapsed=time.monotonic()-begin
                        assert elapsed<8,"store-backed admission exceeded functional outage deadline"
                        assert local_state()=="paused","Redis-unavailable browser request woke guest"
                        timings.append(round(elapsed*1000,3))
                    report["outage_denial_elapsed_ms"]=timings
                    report["no_wake_checked_via_independent_node_mtls"]=True
                case("redis-outage-denies-before-paused-guest-wake",paused_outage)
                def active_recovery():
                    nonlocal redis
                    redis=start("redis-active-recovered",redis_args);eventually(redis_ready,5)
                    eventually(lambda:api("GET",path))
                    assert api("GET",path)["revision"]==request["revision"],"AOF lost active revision"
                    assert local_state()=="paused"
                    eventually(lambda:web(guest));assert local_state()=="running"
                case("live-redis-aof-recovery-preserves-active-grant",active_recovery)
                def revoked_recovery():
                    nonlocal redis,request
                    api("POST",f"/sandboxes/{guest}/pause",{},status=204)
                    tombstone={"expectedRevision":request["revision"],"revision":str(uuid.uuid4()),"grants":[]}
                    api("PUT",path,tombstone);kill_redis();web(guest,401);assert local_state()=="paused"
                    redis=start("redis-revoked-recovered",redis_args);eventually(redis_ready,5)
                    eventually(lambda:api("GET",path))
                    assert api("GET",path)["revision"]==tombstone["revision"]
                    assert api("GET",path)["grants"]==[]
                    web(guest,401);assert local_state()=="paused"
                    api("PUT",path,request,status=409)
                    request={"expectedRevision":tombstone["revision"],"revision":str(uuid.uuid4()),"grants":[{"subject":"alice","expires_at":expiry}]}
                    api("PUT",path,request);eventually(lambda:web(guest));assert local_state()=="running"
                case("live-redis-restart-retains-revocation-and-refuses-stale-replay",revoked_recovery)
            def delegation_reload():
                policy_write(False);control.send_signal(signal.SIGHUP);eventually(lambda:web(guest,401),5)
                policy_write();control.send_signal(signal.SIGHUP);eventually(lambda:web(guest),5)
            case("operator-delegation-reload-controls-owner-grants",delegation_reload)
            expired={"expectedRevision":request["revision"],"revision":str(uuid.uuid4()),"grants":[{"subject":"alice","expires_at":1}]}
            case("expired-stored-grant-denies-real-guest",lambda:(api("PUT",path,expired),web(guest,401)))
            report["success"]=True
        except Exception as error:report["error"]=str(error)
        finally:
            if api_url is not None and context is not None:
                for guest in list(guests):
                    try:api("DELETE",f"/sandboxes/{guest}",status=204);guests.remove(guest)
                    except Exception as error:report["cleanup_errors"].append(str(error))
                try:report["final_inventory_empty"]=api("GET","/sandboxes")==[]
                except Exception as error:report["cleanup_errors"].append(str(error))
            report["processes"]=[{"name":name,"pid":child.pid,"stopped":stop(child)} for name,child in reversed(processes)]
            for handle in handles:handle.close()
            report["input_sha256_after"]={k:digest(v) for k,v in inputs.items()};report["inputs_unchanged"]=report["input_sha256_after"]==report["input_sha256"]
    report["owned_directory_removed"]=not Path(report["owned_directory"]).exists()
    report["success"]=report["success"] and not report["cleanup_errors"] and report.get("final_inventory_empty",False) and report["inputs_unchanged"] and report["owned_directory_removed"] and all(row["stopped"] for row in report["processes"])
    (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"success":report["success"],"cases":len(report["cases"]),"output":str(args.output),"error":report.get("error")},indent=2))
    return 0 if report["success"] else 1
if __name__=="__main__":raise SystemExit(main())
