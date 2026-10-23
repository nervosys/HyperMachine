#!/usr/bin/env python3
"""A team's sandboxes use the team's Git credential, held only on the host, on real KVM.

Runs one clustered hv2-sandboxd (an in-process `memory:` store, a cluster
token, so a request may name its team) with `--team-git-credential
alpha=PREFIX/team/=VAR`, `beta=PREFIX/team/=VAR` (two different tokens for the
same prefix) and `--git-credential PREFIX/other/=VAR --git-credential-in-sandbox`
(a node-wide token). An owned HTTPS Git server records the Authorization of
every request it gets. Checks, from inside guests:

1. A sandbox of team alpha clones /team/ with alpha's token on every request,
   and beta's sandbox with beta's: never the other's.
2. A sandbox with no team gets no credential for /team/ (401), as the node has
   none for it.
3. /other/, which only the node-wide credential covers, is cloned by alpha, by
   beta and by a sandbox with no team, each with the node's token.
4. No token is anywhere in any guest: not in a file, not in a process
   environment.

Writes report.json and the daemon log into --output. Needs root, openssl,
cpio, git and curl on the host.
"""
import argparse, gzip, hashlib, ipaddress, json, os, re, shlex, shutil, signal, socket, ssl, subprocess
import tempfile, threading, time, urllib.request
from base64 import b64encode
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ALPHA = "ghp_" + "A1a" * 12
BETA = "ghp_" + "B2b" * 12
NODE = "ghp_" + "N3n" * 12
HOST = "git.example.test"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


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
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    require(os.geteuid() == 0 and Path("/dev/kvm").exists(), "root and KVM required")
    args.output.mkdir(parents=True, exist_ok=False)
    interfaces = json.loads(subprocess.check_output(["ip", "-j", "-4", "addr", "show", "scope", "global"]))
    address = str(ipaddress.IPv4Address(
        [a["local"] for i in interfaces for a in i["addr_info"] if "local" in a][0]))
    report = {"success": False, "cases": [], "host_cpus": os.cpu_count(),
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "git": subprocess.check_output(["git", "--version"], text=True).strip()}
    report["input_sha256"]["driver"] = digest(__file__)
    seen = []  # (path, authorization or None)
    work = Path(tempfile.mkdtemp(prefix="hm-git-in-sandbox-", dir="/var/tmp"))
    daemons = []
    server = None
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
        # Certificates.
        run = lambda *a: subprocess.run(a, check=True, capture_output=True, timeout=30)
        run("openssl", "req", "-x509", "-newkey", "ed25519", "-nodes", "-keyout", str(work / "root.key"),
            "-out", str(work / "root.pem"), "-days", "1", "-subj", "/CN=Owned fixture root")
        run("openssl", "req", "-new", "-newkey", "ed25519", "-nodes", "-keyout", str(work / "s.key"),
            "-out", str(work / "s.csr"), "-subj", "/CN=git", "-addext", f"subjectAltName=DNS:{HOST}",
            "-addext", "basicConstraints=critical,CA:FALSE")
        run("openssl", "x509", "-req", "-in", str(work / "s.csr"), "-CA", str(work / "root.pem"),
            "-CAkey", str(work / "root.key"), "-CAcreateserial", "-copy_extensions", "copy", "-days", "1",
            "-out", str(work / "s.pem"))
        (work / "root.pem").chmod(0o600)
        basic = lambda token: "Basic " + b64encode(f"bot:{token}".encode()).decode()
        accepted = {basic(ALPHA), basic(BETA), basic(NODE)}

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def do_GET(self):
                auth = self.headers.get("Authorization")
                seen.append((self.path, auth))
                needs = self.path.startswith("/team/") or self.path.startswith("/other/")
                if needs and auth not in accepted:
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
        sport = server.server_port
        base_url = f"https://{HOST}:{sport}"

        def start_daemon():
            api = port()
            proxy = port()
            log = (args.output / "daemon.log").open("wb")
            token = "c" * 64
            proc = subprocess.Popen(
                [str(args.daemon), "--network", "--port", str(api), "--proxy-port", str(proxy),
                 "--snapshot-store", str(work / "store"), "--tenant-reserved-cidr", address + "/32",
                 "--egress-upstream-ca", str(work / "root.pem"),
                 "--cluster-store", "memory:", "--cluster-namespace", "teamgit", "--node-id", "teamgit-node",
                 "--advertise-api", f"http://127.0.0.1:{api}", "--advertise-proxy", f"127.0.0.1:{proxy}",
                 "--team-git-credential", f"alpha={base_url}/team/=HM_GIT_ALPHA",
                 "--team-git-credential", f"beta={base_url}/team/=HM_GIT_BETA",
                 "--git-credential", f"{base_url}/other/=HM_GIT_NODE", "--git-credential-in-sandbox"],
                env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(initrd),
                         HV2_CLUSTER_TOKEN=token, HM_GIT_ALPHA=f"bot:{ALPHA}", HM_GIT_BETA=f"bot:{BETA}",
                         HM_GIT_NODE=f"bot:{NODE}", RUST_LOG="info"),
                stdout=log, stderr=log)
            daemons.append((proc, log))
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def call(method, path, body=None, team=None):
                headers = {"X-Hv2-Cluster-Token": token, "Content-Type": "application/json"}
                if team:
                    headers["x-hv2-sandbox-team"] = team
                req = urllib.request.Request(f"http://127.0.0.1:{api}{path}", method=method, headers=headers,
                                             data=None if body is None else json.dumps(body).encode())
                with opener.open(req, timeout=120) as r:
                    raw = r.read()
                    return json.loads(raw) if raw else None
            deadline = time.monotonic() + 60
            while True:
                require(proc.poll() is None, "the daemon exited")
                try:
                    if call("GET", "/sandboxes") == []:
                        break
                except OSError:
                    pass
                require(time.monotonic() < deadline, "daemon readiness")
                time.sleep(0.2)

            def sandbox(team):
                made = call("POST", "/sandboxes", {"templateID": "base", "timeout": 600,
                                                   "network": {"allowOut": [address + "/32"]}}, team=team)["sandboxID"]
                sh(call, made, f"echo '{address} {HOST}' >> /etc/hosts; echo 'hosts: files dns' > /etc/nsswitch.conf; mkdir -p /root")
                return made
            return call, sandbox

        def sh(call, sandbox, command, ok=True):
            r = call("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 60})
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

        call, make = start_daemon()
        boxes = {"alpha": make("alpha"), "beta": make("beta"), "none": make(None)}
        git = "HOME=/root GIT_TERMINAL_PROMPT=0 git"

        def clone_as(team, section, token):
            seen.clear()
            r = sh(call, boxes[team], f"{git} clone -q {base_url}/{section}/r.git /root/{section} "
                                      f"&& cat /root/{section}/hello.txt")
            require("hello from the private remote" in r["stdout"], r)
            sent = [a for p, a in seen if p.startswith(f"/{section}/")]
            require(len(sent) >= 4, f"only {len(sent)} requests to /{section}/")
            require(all(a == basic(token) for a in sent), (team, section, set(sent)))
            return len(sent)

        def teams_own():
            return {"alpha": clone_as("alpha", "team", ALPHA), "beta": clone_as("beta", "team", BETA)}
        case("each team's sandbox clones with its own team's credential", teams_own)

        def none_has_none():
            seen.clear()
            r = sh(call, boxes["none"], f"{git} clone -q {base_url}/team/r.git /root/team", ok=False)
            require(r["exit_code"] != 0, "it cloned")
            sent = [a for p, a in seen if p.startswith("/team/")]
            require(sent and all(a is None for a in sent), sent)
            return {"stderr": r["stderr"].strip()[-120:]}
        case("a sandbox with no team is given neither team's credential", none_has_none)

        def node_wide():
            return {t: clone_as(t, "other", NODE) for t in ["alpha", "beta", "none"]}
        case("what only the node's credential covers is cloned by every sandbox", node_wide)

        def hidden():
            for team, box in boxes.items():
                found = sh(call, box, f"grep -rlE '{ALPHA}|{BETA}|{NODE}' /etc /root /tmp /home 2>/dev/null; "
                                      f"grep -lE '{ALPHA}|{BETA}|{NODE}' /proc/[0-9]*/environ 2>/dev/null; true")
                require(found["stdout"].strip() == "", (team, found["stdout"]))
                for token in (ALPHA, BETA, NODE):
                    b64 = b64encode(f"bot:{token}".encode()).decode()
                    require(sh(call, box, f"grep -rl {b64} /etc /root /tmp /home 2>/dev/null; true")["stdout"].strip() == "",
                            (team, "base64"))
        case("no token is anywhere in any guest", hidden)

        report["success"] = True
    finally:
        for proc, log in daemons:
            if proc.poll() is None:
                proc.send_signal(signal.SIGTERM)
                try:
                    proc.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    proc.kill()
            log.close()
        if server:
            server.shutdown()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
