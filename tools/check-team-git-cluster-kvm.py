#!/usr/bin/env python3
"""A team's Git credential reaches only that team's sandboxes, through a real control plane, on KVM.

Runs an owned Redis, one hv2-sandboxd node and the hv2-control-plane over
verified TLS and node mTLS, with an API key policy of a red key, a blue key
and the legacy administrator, and an owned HTTPS Git server that records the
Authorization of every request. The node holds one credential per team for
the same prefix (`--team-git-credential red=...`, `blue=...`); no team
credential is given to the administrator. Then, through the control plane's
API only (so the team is the control plane's, from the API key, not a header
this script sends):

1. A sandbox the red key created clones /team/ with red's token on every
   request, and blue's with blue's: never the other's.
2. A sandbox the administrator created (no team) is refused (401) and sent no
   credential.
3. No token is anywhere in any guest: not in a file, not in a process
   environment.

Writes report.json and each service's log into --output. Needs root, Redis,
openssl, cpio, git and curl on the host.
"""
import argparse, shlex, gzip, hashlib, http.client, ipaddress, json, os, re, shutil, socket, ssl, subprocess
import tempfile, threading, time, uuid
from base64 import b64encode
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

RED = "ghp_" + "R1r" * 12
BLUE = "ghp_" + "B2b" * 12
HOST = "git.example.test"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def build_initrd(base, out):
    """The busybox initrd plus the host's git, git-remote-https and curl, with their libraries."""
    wanted = {"/usr/bin/git": "/usr/bin/git", "/usr/bin/curl": "/bin/curl",
              "/usr/lib/git-core/git-remote-http": "/usr/lib/git-core/git-remote-http",
              "/usr/lib/git-core/git-remote-https": "/usr/lib/git-core/git-remote-https"}
    libs = set()
    for binary in ["/usr/bin/git", "/usr/bin/curl", "/usr/lib/git-core/git-remote-http"]:
        listing = subprocess.run(["ldd", binary], capture_output=True, text=True, check=True).stdout
        require("not found" not in listing, f"{binary} has a missing dependency")
        libs |= set(re.findall(r"(?:=>\s+|^\s*)(/\S+)", listing, re.MULTILINE))
    with tempfile.TemporaryDirectory(prefix="hm-git-initrd-") as directory:
        root = Path(directory)
        subprocess.run(["cpio", "-id", "--no-absolute-filenames", "--quiet"],
                       input=gzip.decompress(Path(base).read_bytes()), cwd=root, check=True, capture_output=True)
        for source, destination in list(wanted.items()) + [(p, p) for p in sorted(libs)]:
            target = root / destination.lstrip("/")
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.is_symlink() or target.exists():
                target.unlink()
            shutil.copyfile(source, target)
            target.chmod(0o755)
        names = sorted(["."] + [str(p.relative_to(root)) for p in root.rglob("*")])
        packed = subprocess.run(["cpio", "--null", "-o", "-H", "newc", "-R", "0:0", "--quiet"],
                                input=b"\0".join(n.encode() for n in names) + b"\0", cwd=root,
                                check=True, capture_output=True).stdout
    Path(out).write_bytes(gzip.compress(packed, compresslevel=6))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "control", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    require(os.geteuid() == 0 and Path("/dev/kvm").exists(), "root and KVM required")
    args.output.mkdir(parents=True, exist_ok=False)
    interfaces = json.loads(subprocess.check_output(["ip", "-j", "-4", "addr", "show", "scope", "global"]))
    address = str(ipaddress.IPv4Address(
        [a["local"] for i in interfaces for a in i["addr_info"] if "local" in a][0]))
    report = {"success": False, "cases": [], "host_cpus": os.cpu_count(),
              "purpose": "functional verification of team Git credentials through a control plane; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "control", "kernel", "initrd"]}}
    report["input_sha256"]["driver"] = digest(__file__)
    keys = {name: uuid.uuid4().hex for name in ["red", "blue", "admin", "token"]}
    env = {"PATH": "/usr/local/bin:/usr/bin:/bin", "RUST_LOG": "warn"}
    processes, handles = [], []
    state = {}
    seen = []  # (path, authorization or None)
    server = None

    def api(method, path, body=None, key="red", status=(200, 201)):
        conn = http.client.HTTPSConnection("127.0.0.1", state["api_port"], context=state["context"], timeout=120)
        try:
            conn.request(method, path, body=None if body is None else json.dumps(body),
                         headers={"x-api-key": keys[key], "content-type": "application/json"})
            response = conn.getresponse()
            data = response.read(1 << 20)
            wanted = (status,) if isinstance(status, int) else status
            require(response.status in wanted, f"{key} {method} {path}: {response.status}: {data[:300]!r}")
            return json.loads(data) if data else None
        finally:
            conn.close()

    def sh(box, key, command, ok=True):
        r = api("POST", f"/sandboxes/{box}/exec", {"cmd": command, "timeout_secs": 60}, key=key)
        if ok:
            require(r["exit_code"] == 0, f"{command!r} exited {r['exit_code']}: {r['stderr'][:300]}")
        return r

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        detail = action()
        row["seconds"] = round(time.monotonic() - began, 3)
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def eventually(action, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            try:
                return action()
            except (OSError, AssertionError):
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)

    work = Path(tempfile.mkdtemp(prefix="hm-team-git-cluster-", dir="/var/tmp"))
    try:
        initrd = work / "initrd.cpio.gz"
        build_initrd(args.initrd, initrd)
        report["input_sha256"]["combined_initrd"] = digest(initrd)
        # A repository, served as static files.
        src, bare = work / "src", work / "r.git"
        for command in [["git", "init", "-q", str(src)],
                        ["git", "-C", str(src), "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q",
                         "--allow-empty", "-m", "first"]]:
            subprocess.run(command, check=True, capture_output=True)
        (src / "hello.txt").write_text("hello from the private remote\n")
        subprocess.run(["git", "-C", str(src), "add", "."], check=True)
        subprocess.run(["git", "-C", str(src), "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q",
                        "-m", "file"], check=True)
        subprocess.run(["git", "clone", "-q", "--bare", str(src), str(bare)], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(bare), "update-server-info"], check=True)

        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)

        # The cluster's certificates.
        openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-teams-ca",
                 "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                 "-keyout", str(work / "ca.key"), "-out", str(work / "ca.pem")])
        for name, usage, san in [("node", "serverAuth", "DNS:tcp-node.test,IP:127.0.0.1"),
                                 ("control", "clientAuth", "DNS:tcp-control.test"),
                                 ("api", "serverAuth", "IP:127.0.0.1,DNS:localhost")]:
            openssl(["req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                     "-keyout", str(work / (name + ".key")), "-out", str(work / (name + ".csr"))])
            ext = work / (name + ".ext")
            ext.write_text(f"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                           f"extendedKeyUsage={usage}\nsubjectAltName={san}\n")
            openssl(["x509", "-req", "-in", str(work / (name + ".csr")), "-CA", str(work / "ca.pem"),
                     "-CAkey", str(work / "ca.key"), "-CAcreateserial", "-days", "2", "-extfile", str(ext),
                     "-out", str(work / (name + ".pem"))])
        state["context"] = ssl.create_default_context(cafile=str(work / "ca.pem"))
        # The Git server's root and certificate.
        openssl(["req", "-x509", "-newkey", "ed25519", "-nodes", "-keyout", str(work / "root.key"),
                 "-out", str(work / "root.pem"), "-days", "1", "-subj", "/CN=Owned fixture root"])
        openssl(["req", "-new", "-newkey", "ed25519", "-nodes", "-keyout", str(work / "s.key"),
                 "-out", str(work / "s.csr"), "-subj", "/CN=git", "-addext", f"subjectAltName=DNS:{HOST}",
                 "-addext", "basicConstraints=critical,CA:FALSE"])
        openssl(["x509", "-req", "-in", str(work / "s.csr"), "-CA", str(work / "root.pem"),
                 "-CAkey", str(work / "root.key"), "-CAcreateserial", "-copy_extensions", "copy", "-days", "1",
                 "-out", str(work / "s.pem")])
        (work / "root.pem").chmod(0o600)
        basic = lambda token: "Basic " + b64encode(f"bot:{token}".encode()).decode()
        accepted = {basic(RED), basic(BLUE)}

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def do_GET(self):
                auth = self.headers.get("Authorization")
                seen.append((self.path, auth))
                if self.path.startswith("/team/") and auth not in accepted:
                    body = b"no"
                    self.send_response(401)
                    self.send_header("WWW-Authenticate", 'Basic realm="x"')
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return
                path = self.path.split("?", 1)[0]
                rel = path.split("/", 2)[2] if path.count("/") >= 2 else ""
                rel = rel.removeprefix("r.git/")
                target = (bare / rel).resolve()
                if bare.resolve() not in target.parents or not target.is_file():
                    self.send_response(404)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                data = target.read_bytes()
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        class Server(ThreadingHTTPServer):
            request_queue_size = 32
            daemon_threads = True

        server = Server((address, 0), Handler)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(work / "s.pem", work / "s.key")
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base_url = f"https://{HOST}:{server.server_port}"

        def start(name, argv, environment):
            handle = (args.output / (name + ".log")).open("wb")
            handles.append(handle)
            child = subprocess.Popen(argv, env=environment, stdin=subprocess.DEVNULL, stdout=handle,
                                     stderr=subprocess.STDOUT)
            processes.append(child)
            return child

        ports = set()
        while len(ports) < 5:
            ports.add(port())
        redis_port, node_port, node_proxy, api_port, proxy_port = list(ports)
        state["api_port"] = api_port
        store = f"redis://127.0.0.1:{redis_port}"
        namespace = "teamgit-" + uuid.uuid4().hex
        start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "",
                        "--appendonly", "no", "--dir", str(work)], env)

        def redis_ready():
            with socket.create_connection(("127.0.0.1", redis_port), timeout=0.2):
                pass
        eventually(redis_ready, 5)
        expiry = int(time.time()) + 1800
        scopes = ["sandboxes", "inventory", "volumes", "templates", "events"]
        policy = [
            {"sha256": hashlib.sha256(keys["red"].encode()).hexdigest(), "expires_at": expiry,
             "scopes": scopes, "principal_id": "red-a", "team_id": "red"},
            {"sha256": hashlib.sha256(keys["blue"].encode()).hexdigest(), "expires_at": expiry,
             "scopes": scopes, "principal_id": "blue-a", "team_id": "blue"},
        ]
        (work / "keys.json").write_text(json.dumps(policy))
        start("node", [str(args.daemon), "--port", str(node_port), "--proxy-port", str(node_proxy),
                       "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8", "--network",
                       "--tenant-reserved-cidr", address + "/32", "--egress-upstream-ca", str(work / "root.pem"),
                       "--volume-dir", str(work / "volumes"), "--snapshot-store", str(work / "snapshots"),
                       "--cluster-store", store, "--cluster-namespace", namespace, "--node-id", "teamgit-node",
                       "--advertise-api", f"https://127.0.0.1:{node_port}",
                       "--advertise-proxy", f"127.0.0.1:{node_proxy}",
                       "--team-git-credential", f"red={base_url}/team/=HM_GIT_RED",
                       "--team-git-credential", f"blue={base_url}/team/=HM_GIT_BLUE",
                       "--mtls-ca", str(work / "ca.pem"), "--mtls-cert", str(work / "node.pem"),
                       "--mtls-key", str(work / "node.key")],
              {**env, "HV2_KERNEL": str(args.kernel), "HV2_INITRD": str(initrd), "HV2_CLUSTER_TOKEN": keys["token"],
               "HM_GIT_RED": f"bot:{RED}", "HM_GIT_BLUE": f"bot:{BLUE}"})
        start("control", [str(args.control), "--store", store, "--namespace", namespace, "--port", str(api_port),
                          "--proxy-port", str(proxy_port), "--api-keys-file", str(work / "keys.json"),
                          "--api-tls-cert", str(work / "api.pem"), "--api-tls-key", str(work / "api.key"),
                          "--tls-cert", str(work / "api.pem"), "--tls-key", str(work / "api.key"),
                          "--mtls-ca", str(work / "ca.pem"), "--mtls-cert", str(work / "control.pem"),
                          "--mtls-key", str(work / "control.key"), "--mtls-node-name", "tcp-node.test"],
              {**env, "HV2_API_KEY": keys["admin"], "HV2_CLUSTER_TOKEN": keys["token"]})

        def prepared():
            require(all(child.poll() is None for child in processes), "an owned service exited")
            templates = api("GET", "/templates", key="admin")
            require(any(row.get("templateID") == "base" and row.get("snapshot") for row in templates),
                    "template not ready")
        eventually(prepared, 300)

        def sandbox(key):
            made = api("POST", "/v2/sandboxes", {"templateID": "base", "timeout": 600,
                                                 "network": {"allowOut": [address + "/32"]}}, key=key)["sandboxID"]
            sh(made, key, f"echo '{address} {HOST}' >> /etc/hosts; echo 'hosts: files dns' > /etc/nsswitch.conf; "
                          "mkdir -p /root")
            return made

        boxes = {team: (team, sandbox(team)) for team in ["red", "blue", "admin"]}
        git = "HOME=/root GIT_TERMINAL_PROMPT=0 git"

        def clone_as(team, token):
            key, box = boxes[team]
            seen.clear()
            r = sh(box, key, f"{git} clone -q {base_url}/team/r.git /root/c && cat /root/c/hello.txt")
            require("hello from the private remote" in r["stdout"], r)
            sent = [a for p, a in seen if p.startswith("/team/")]
            require(len(sent) >= 4, f"only {len(sent)} requests to /team/")
            require(all(a == basic(token) for a in sent), (team, set(sent)))
            return len(sent)

        def teams_own():
            return {"red": clone_as("red", RED), "blue": clone_as("blue", BLUE)}
        case("each team's sandbox, made through the control plane, clones with its own credential", teams_own)

        def administrator():
            key, box = boxes["admin"]
            # Not intercepted for this sandbox (nothing is held for it), so the
            # guest has to trust the server's root itself for a request to get
            # as far as the server.
            sh(box, key, "printf '%s' " + shlex.quote((work / "root.pem").read_text())
               + " >> /etc/ssl/certs/ca-certificates.crt")
            seen.clear()
            r = sh(box, key, f"{git} clone -q {base_url}/team/r.git /root/c", ok=False)
            require(r["exit_code"] != 0, "it cloned")
            sent = [a for p, a in seen if p.startswith("/team/")]
            require(sent and all(a is None for a in sent), sent)
            return {"stderr": r["stderr"].strip()[-120:]}
        case("a sandbox with no team is given neither team's credential", administrator)

        def hidden():
            for team, (key, box) in boxes.items():
                found = sh(box, key, f"grep -rlE '{RED}|{BLUE}' /etc /root /tmp /home 2>/dev/null; "
                                     f"grep -lE '{RED}|{BLUE}' /proc/[0-9]*/environ 2>/dev/null; true")
                require(found["stdout"].strip() == "", (team, found["stdout"]))
                for token in (RED, BLUE):
                    b64 = b64encode(f"bot:{token}".encode()).decode()
                    require(sh(box, key, f"grep -rl {b64} /etc /root /tmp /home 2>/dev/null; true")["stdout"].strip() == "",
                            (team, "base64"))
        case("no token is anywhere in any guest", hidden)

        report["success"] = True
    finally:
        for child in reversed(processes):
            stop(child)
        for handle in handles:
            handle.close()
        if server:
            server.shutdown()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
