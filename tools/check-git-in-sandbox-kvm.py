#!/usr/bin/env python3
"""A sandbox's own `git` uses a credential only the host holds, on real KVM.

Runs an owned hv2-sandboxd with `--git-credential PREFIX=VAR
--git-credential-in-sandbox` and an owned HTTPS Git server (a bare repository
served over the dumb protocol; every request under /team/ and /other/ must
carry `Authorization: Basic base64(bot:<token>)`, /open/ needs none). The
guest is the busybox initrd plus the host's git and curl and their libraries.
Checks, from inside a guest and through the server's own record of what each
request carried:

1. `git clone https://.../team/r.git` works with no credential in the guest,
   and every request to /team/ carried exactly the node's credential.
2. The token is nowhere in the guest: not in any file under /etc /root /tmp
   /home, not in any process environment, not in the clone's .git/config
   (a control file holding it is found by the same search).
3. A request the guest signed itself, under the prefix, reaches the server
   with the node's credential instead.
4. Outside the prefix (/open/ and /other/) nothing is added: /open/ clones
   with no Authorization header at all, and /other/ is refused 401.
5. A path that climbs out of the prefix with `..` or an encoded dot is sent
   no credential.
6. A node started without the flag cannot clone /team/: 401 and no credential
   ever reached the server.

Writes report.json and the daemon logs into --output. Needs root, openssl,
cpio, git and curl on the host.
"""
import argparse, gzip, hashlib, ipaddress, json, os, re, shlex, shutil, signal, socket, ssl, subprocess
import tempfile, threading, time, urllib.request
from base64 import b64encode
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

TOKEN = "ghp_" + "Z9x" * 12
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
        expected = "Basic " + b64encode(f"bot:{TOKEN}".encode()).decode()

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def do_GET(self):
                auth = self.headers.get("Authorization")
                seen.append((self.path, auth))
                needs = self.path.startswith("/team/") or self.path.startswith("/other/")
                if needs and auth != expected:
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

        def start_daemon(tag, extra):
            api = port()
            log = (args.output / f"daemon-{tag}.log").open("wb")
            token = "c" * 64
            proc = subprocess.Popen(
                [str(args.daemon), "--network", "--port", str(api), "--proxy-port", str(port()),
                 "--snapshot-store", str(work / f"store-{tag}"), "--tenant-reserved-cidr", address + "/32",
                 "--egress-upstream-ca", str(work / "root.pem"),
                 "--git-credential", f"{base_url}/team/=HM_GIT_TOKEN", *extra],
                env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(initrd),
                         HV2_CLUSTER_TOKEN=token, HM_GIT_TOKEN=f"bot:{TOKEN}", RUST_LOG="info"),
                stdout=log, stderr=log)
            daemons.append((proc, log))
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def call(method, path, body=None):
                req = urllib.request.Request(f"http://127.0.0.1:{api}{path}", method=method,
                                             headers={"X-Hv2-Cluster-Token": token,
                                                      "Content-Type": "application/json"},
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

            def sandbox():
                made = call("POST", "/sandboxes", {"templateID": "base", "timeout": 600,
                                                   "network": {"allowOut": [address + "/32"]}})["sandboxID"]
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

        call, make = start_daemon("on", ["--git-credential-in-sandbox"])
        sb = make()
        git = "HOME=/root GIT_TERMINAL_PROMPT=0 git"
        state = {}

        def clone():
            seen.clear()
            r = sh(call, sb, f"{git} clone -q {base_url}/team/r.git /root/c1 && cat /root/c1/hello.txt")
            require("hello from the private remote" in r["stdout"], r)
            team = [a for p, a in seen if p.startswith("/team/")]
            require(len(team) >= 4, f"only {len(team)} requests to /team/")
            require(all(a == expected for a in team), "a /team/ request lacked the node's credential")
            return {"requests_to_team": len(team)}
        case("a clone from inside the sandbox uses the node's credential", clone)

        def hidden():
            sh(call, sb, f"echo {TOKEN} > /root/control")
            found = sh(call, sb, f"grep -rl {TOKEN} /etc /root /tmp /home 2>/dev/null; "
                                 f"grep -l {TOKEN} /proc/[0-9]*/environ 2>/dev/null; true")["stdout"].split()
            require(found == ["/root/control"], f"the token or its control: {found}")
            b64 = b64encode(f"bot:{TOKEN}".encode()).decode()
            require(sh(call, sb, f"grep -rl {b64} /etc /root /tmp /home 2>/dev/null; true")["stdout"].strip() == "",
                    "its base64 is in the guest")
            cfg = sh(call, sb, "cat /root/c1/.git/config")["stdout"]
            require("Authorization" not in cfg and "@" not in cfg, cfg)
        case("the token is nowhere in the guest", hidden)

        def replaced():
            seen.clear()
            r = sh(call, sb, f"curl -s --max-time 20 -o /dev/null -w '%{{http_code}}' -H 'Authorization: Bearer guest' "
                             f"{base_url}/team/r.git/HEAD")
            require(r["stdout"].strip() == "200", r)
            require(seen and all(a == expected for _, a in seen), seen)
        case("an Authorization the guest wrote is replaced, not sent", replaced)

        def outside():
            seen.clear()
            r = sh(call, sb, f"{git} clone -q {base_url}/open/r.git /root/c2 && cat /root/c2/hello.txt")
            require("hello" in r["stdout"], r)
            require(seen and all(a is None for _, a in seen), seen)
            seen.clear()
            r = sh(call, sb, f"curl -s --max-time 20 -o /dev/null -w '%{{http_code}}' {base_url}/other/r.git/HEAD")
            require(r["stdout"].strip() == "401", r)
            require(seen and all(a is None for _, a in seen), seen)
        case("outside the prefix nothing is added", outside)

        def climbing():
            for path in ["/team/../other/r.git/HEAD", "/team/%2e%2e/other/r.git/HEAD"]:
                seen.clear()
                r = sh(call, sb, f"curl -s --path-as-is --max-time 20 -o /dev/null -w '%{{http_code}}' {base_url}{path}")
                require(r["stdout"].strip() == "401", (path, r))
                require(seen and all(a is None for _, a in seen), (path, seen))
        case("a path that climbs out of the prefix is sent no credential", climbing)

        # A node without the flag.
        call2, make2 = start_daemon("off", [])
        sb2 = make2()
        # Not intercepted there, so the guest has to trust the server's root
        # itself for the request to get as far as the server.
        sh(call2, sb2, "printf '%s' " + shlex.quote((work / "root.pem").read_text())
           + " >> /etc/ssl/certs/ca-certificates.crt")

        def without():
            seen.clear()
            r = sh(call2, sb2, f"{git} clone -q {base_url}/team/r.git /root/c3", ok=False)
            require(r["exit_code"] != 0, "it cloned")
            require(seen and all(a is None for _, a in seen), seen)
            return {"stderr": r["stderr"].strip()[-160:]}
        case("without the flag the guest has no credential", without)

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
