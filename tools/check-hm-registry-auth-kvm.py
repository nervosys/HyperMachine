#!/usr/bin/env python3
"""`hm sandbox vm run` and `build` against a registry that wants a password, on real KVM.

Runs an owned hv2-sandboxd with a guest kit, and a registry of this script's
own on localhost (the node speaks plain HTTP to localhost only). The registry
serves one tiny image (a single layer holding /srv/marker.txt) and answers 401
to any request without the right Basic credentials. No Docker anywhere. It
drives the real `hm` binary:

1. Without credentials `run` fails and the registry saw an unauthenticated
   request only.
2. With the wrong password `run` fails.
3. With `--registry-user` and `--registry-password-env` `run` pulls the image
   and prints the marker.
4. The password is in no daemon log line and no `GET /templates` answer, and
   was never in `hm`'s arguments.
5. `--registry-user` without `--registry-password-env` (and the reverse), and
   a variable that is unset, fail before the registry is contacted.
6. `build` of a Dockerfile `FROM` the private image works with the same flags,
   and fails without them.

Writes report.json, the daemon log and the registry's request log into --output.
"""
import argparse, gzip, hashlib, http.server, io, json, os, signal, socket, subprocess, tarfile, threading, time
import urllib.request
from base64 import b64encode
from pathlib import Path

USER = "robot"
PASSWORD = "p4ss-w0rd-Zq81"  # distinctive, so the leak checks can search for it
MARKER = "from-the-private-registry\n"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def make_image():
    """One layer with /srv/marker.txt, an OCI config, and a manifest."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as tar:
        for name, kind in [("srv", "dir"), ("srv/marker.txt", "file")]:
            info = tarfile.TarInfo(name)
            info.mtime = 0
            if kind == "dir":
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                tar.addfile(info)
            else:
                body = MARKER.encode()
                info.size = len(body)
                info.mode = 0o644
                tar.addfile(info, io.BytesIO(body))
    layer_tar = raw.getvalue()
    layer = gzip.compress(layer_tar, mtime=0)
    config = json.dumps({
        "architecture": "amd64", "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": [sha(layer_tar)]},
        "config": {"Cmd": ["/bin/sh"]},
    }).encode()
    manifest = json.dumps({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": sha(config), "size": len(config)},
        "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar+gzip", "digest": sha(layer), "size": len(layer)}],
    }).encode()
    return {"manifest": manifest, "blobs": {sha(layer): layer, sha(config): config}}


class Registry(http.server.BaseHTTPRequestHandler):
    image = None
    log = []
    log_lock = threading.Lock()
    want = "Basic " + b64encode(f"{USER}:{PASSWORD}".encode()).decode()

    def log_message(self, *args):
        pass

    def do_GET(self):
        authed = self.headers.get("Authorization") == self.want
        with self.log_lock:
            self.log.append({"path": self.path, "authorized": authed,
                             "sent_credentials": "Authorization" in self.headers})
        if not authed:
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="private"')
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        parts = self.path.split("/")
        if self.path == "/v2/":
            body, kind = b"{}", "application/json"
        elif "/manifests/" in self.path:
            body, kind = self.image["manifest"], "application/vnd.oci.image.manifest.v1+json"
        elif "/blobs/" in self.path and parts[-1] in self.image["blobs"]:
            body, kind = self.image["blobs"][parts[-1]], "application/octet-stream"
        else:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Docker-Content-Digest", sha(body))
        self.end_headers()
        self.wfile.write(body)

    do_HEAD = do_GET


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "hm", "kernel", "initrd", "kit", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "hm run/build against a registry that needs a password, on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)

    Registry.image = make_image()
    reg_port = port()
    registry = http.server.ThreadingHTTPServer(("127.0.0.1", reg_port), Registry)
    threading.Thread(target=registry.serve_forever, daemon=True).start()
    ref = f"localhost:{reg_port}/private/marker:1"

    api = port()
    url = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(args.kit),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def get(path):
        with urllib.request.urlopen(url + path, timeout=30) as r:
            return r.read().decode()

    argvs = []

    def hm(*argv, env_extra=None, timeout=900):
        full = [str(args.hm), "sandbox", "vm", "--endpoint", url, "--request-timeout", "300", *argv]
        argvs.append(full)
        env = {k: v for k, v in os.environ.items() if k not in ("HV2_API_KEY", "REG_PW")} | {"HOME": str(args.output)}
        env.update(env_extra or {})
        return subprocess.run(full, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL, env=env)

    def requests_seen():
        with Registry.log_lock:
            return len(Registry.log)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        row["detail"] = action()
        row["success"] = True
        print("ok:", name, flush=True)

    flags = ["--registry-user", USER, "--registry-password-env", "REG_PW"]
    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                get("/sandboxes")
                break
            except OSError:
                assert time.monotonic() < deadline, "daemon readiness"
                time.sleep(0.3)

        def no_credentials():
            before = requests_seen()
            done = hm("run", "--build-wait", "60", ref, "cat", "/srv/marker.txt")
            assert done.returncode != 0, done
            seen = Registry.log[before:]
            assert seen and not any(r["authorized"] for r in seen), seen
            return {"exit": done.returncode, "stderr": done.stderr.strip()[-300:], "registry_saw": len(seen)}
        case("without credentials, run fails and the registry saw no authorised request", no_credentials)

        def wrong_password():
            done = hm("run", "--build-wait", "60", *flags, ref, "cat", "/srv/marker.txt", env_extra={"REG_PW": "not-the-password"})
            assert done.returncode != 0 and MARKER.strip() not in done.stdout, done
            return {"exit": done.returncode, "stderr": done.stderr.strip()[-300:]}
        case("with the wrong password, run fails", wrong_password)

        def right_password():
            done = hm("run", "--build-wait", "120", *flags, ref, "cat", "/srv/marker.txt", env_extra={"REG_PW": PASSWORD})
            assert done.returncode == 0 and done.stdout == MARKER, (done.returncode, done.stdout, done.stderr[-500:])
            assert any(r["authorized"] and "/blobs/" in r["path"] for r in Registry.log), "no authorised blob request"
            return {"stdout": done.stdout}
        case("with --registry-user and --registry-password-env, run pulls the image and prints its marker", right_password)

        def nothing_leaks():
            listing = get("/templates")
            assert PASSWORD not in listing, "the password is in GET /templates"
            log.flush()
            assert PASSWORD.encode() not in (args.output / "daemon.log").read_bytes(), "the password is in the daemon log"
            assert not any(PASSWORD in a for argv in argvs for a in argv), "the password was in hm's arguments"
            return {"checked": ["GET /templates", "daemon.log", f"{len(argvs)} hm command lines"]}
        case("the password is in no daemon log line, no template listing and no argument", nothing_leaks)

        def bad_flags():
            before = requests_seen()
            rows = {}
            for label, extra, env in [
                ("user only", ["--registry-user", USER], {}),
                ("variable only", ["--registry-password-env", "REG_PW"], {"REG_PW": PASSWORD}),
                ("variable unset", flags, {}),
                ("variable empty", flags, {"REG_PW": ""}),
                ("bad variable name", ["--registry-user", USER, "--registry-password-env", "1-BAD"], {}),
            ]:
                done = hm("run", "--build-wait", "30", *extra, f"localhost:{reg_port}/private/other:1", "true", env_extra=env)
                assert done.returncode != 0, (label, done)
                assert PASSWORD not in done.stderr, label
                rows[label] = done.stderr.strip().splitlines()[-1][:160] if done.stderr.strip() else ""
            assert requests_seen() == before, "the registry was contacted"
            return rows
        case("a missing, empty or malformed flag set fails before the registry is contacted", bad_flags)

        def build_with_login():
            ctx = args.output / "ctx"
            ctx.mkdir()
            (ctx / "Dockerfile").write_text(f"FROM {ref}\nRUN cat /srv/marker.txt > /srv/copy.txt\nCMD [\"cat\", \"/srv/copy.txt\"]\n")
            denied = hm("build", "-t", "needs-login", str(ctx))
            assert denied.returncode != 0, denied
            built = hm("build", "-t", "with-login", *flags, str(ctx), env_extra={"REG_PW": PASSWORD})
            assert built.returncode == 0, built.stderr[-800:]
            ran = hm("run", "--template", "with-login")
            assert ran.returncode == 0 and ran.stdout == MARKER, (ran.stdout, ran.stderr[-400:])
            return {"without_flags": denied.stderr.strip()[-200:], "stdout": ran.stdout}
        case("build FROM the private image needs and uses the same flags", build_with_login)

        report["success"] = True
    finally:
        registry.shutdown()
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "registry-requests.json").write_text(json.dumps(Registry.log, indent=1) + "\n")
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
