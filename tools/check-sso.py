#!/usr/bin/env python3
"""Single sign-on with the shipped binaries, against an owned OIDC provider.

Starts a fixture OpenID Connect provider over TLS (an owned CA; RS256 ID tokens
signed by the openssl CLI) and the real `hv2-control-plane` with its `--sso-*`
options and API TLS, then checks:

1. With SSO on, an anonymous API request is refused.
2. A browser sign-in -- control plane, provider, callback -- ends in a
   session cookie the API admits; `/auth/session` names the member.
3. A cookie-authenticated change without this control plane's Origin is
   refused.
4. A provider identity that is not a member gets no session.
5. `hm sandbox vm login --no-browser`, with this script as the browser,
   stores a session the CLI then uses with no HV2_API_KEY; `logout` forgets
   it and the next call is refused.
6. A SIGHUP reload that removes the member ends the browser session.

No KVM is needed: the control plane runs on a memory store with no nodes.
Writes report.json and the services' logs into --output.
"""
import argparse, base64, hashlib, http.client, http.server, json, os, signal, socket, ssl
import subprocess, tempfile, threading, time, urllib.parse, uuid
from pathlib import Path


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Provider:
    """The fixture provider's state: who signs in next, and pending codes."""

    def __init__(self, directory, issuer, client_id, client_secret, redirect):
        self.directory, self.issuer = directory, issuer
        self.client_id, self.client_secret, self.redirect = client_id, client_secret, redirect
        self.email = "alice@example.com"
        self.codes = {}
        key = directory / "idp-signing.pem"
        subprocess.run(["openssl", "genrsa", "-out", str(key), "2048"], check=True, capture_output=True)
        modulus = subprocess.run(["openssl", "rsa", "-in", str(key), "-noout", "-modulus"],
                                 check=True, capture_output=True, text=True).stdout.strip().split("=", 1)[1]
        self.key = key
        self.jwk = {"kty": "RSA", "kid": "fixture-rsa", "use": "sig", "alg": "RS256",
                    "n": b64url(bytes.fromhex(modulus)), "e": "AQAB"}

    def sign(self, claims):
        header = b64url(json.dumps({"alg": "RS256", "kid": "fixture-rsa", "typ": "JWT"}).encode())
        payload = b64url(json.dumps(claims).encode())
        signing_input = f"{header}.{payload}".encode()
        signature = subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(self.key)],
                                   input=signing_input, check=True, capture_output=True).stdout
        return f"{header}.{payload}.{b64url(signature)}"


def provider_handler(provider):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, status, body=None, headers=()):
            data = b"" if body is None else json.dumps(body).encode()
            self.send_response(status)
            for name, value in headers:
                self.send_header(name, value)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            url = urllib.parse.urlsplit(self.path)
            query = dict(urllib.parse.parse_qsl(url.query))
            if url.path == "/.well-known/openid-configuration":
                self.reply(200, {"issuer": provider.issuer,
                                 "authorization_endpoint": provider.issuer + "/authorize",
                                 "token_endpoint": provider.issuer + "/token",
                                 "jwks_uri": provider.issuer + "/jwks"})
            elif url.path == "/jwks":
                self.reply(200, {"keys": [provider.jwk]})
            elif url.path == "/authorize":
                assert query["client_id"] == provider.client_id and query["redirect_uri"] == provider.redirect
                assert query["code_challenge_method"] == "S256"
                code = uuid.uuid4().hex
                provider.codes[code] = (query["nonce"], query["code_challenge"])
                location = f"{provider.redirect}?code={code}&state={urllib.parse.quote(query['state'])}"
                self.reply(302, headers=[("Location", location)])
            else:
                self.reply(404)

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            form = dict(urllib.parse.parse_qsl(self.rfile.read(length).decode()))
            expected = "Basic " + base64.b64encode(f"{provider.client_id}:{provider.client_secret}".encode()).decode()
            if self.path != "/token" or self.headers.get("Authorization") != expected:
                return self.reply(401)
            pending = provider.codes.pop(form.get("code"), None)
            if not pending or b64url(hashlib.sha256(form["code_verifier"].encode()).digest()) != pending[1]:
                return self.reply(400)
            now = int(time.time())
            token = provider.sign({"iss": provider.issuer, "aud": provider.client_id, "sub": "sub-" + provider.email,
                                   "email": provider.email, "email_verified": True, "iat": now, "exp": now + 300,
                                   "nonce": pending[0]})
            self.reply(200, {"id_token": token, "access_token": "unused", "token_type": "Bearer"})
    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["control", "cli", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of SSO with the shipped binaries",
              "input_sha256": {"control": digest(args.control), "cli": digest(args.cli), "driver": digest(__file__)},
              "cases": []}
    processes, handles = [], []

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    with tempfile.TemporaryDirectory(prefix="hm-sso-check-", dir="/var/tmp") as temporary:
        directory = Path(temporary)

        def openssl(argv):
            subprocess.run(["openssl"] + argv, check=True, capture_output=True, timeout=30)

        try:
            openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=owned-sso-ca",
                     "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                     "-keyout", str(directory / "ca.key"), "-out", str(directory / "ca.pem")])
            for name in ["idp", "api"]:
                openssl(["req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=" + name,
                         "-keyout", str(directory / f"{name}.key"), "-out", str(directory / f"{name}.csr")])
                ext = directory / f"{name}.ext"
                ext.write_text("basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\n"
                               "extendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1,DNS:localhost\n")
                openssl(["x509", "-req", "-in", str(directory / f"{name}.csr"), "-CA", str(directory / "ca.pem"),
                         "-CAkey", str(directory / "ca.key"), "-CAcreateserial", "-days", "2",
                         "-extfile", str(ext), "-out", str(directory / f"{name}.pem")])
            context = ssl.create_default_context(cafile=str(directory / "ca.pem"))

            idp_port, api_port, proxy_port = port(), port(), port()
            issuer = f"https://127.0.0.1:{idp_port}"
            api = f"https://127.0.0.1:{api_port}"
            redirect = f"{api}/auth/callback"
            secret = uuid.uuid4().hex
            provider = Provider(directory, issuer, "hypermachine", secret, redirect)
            server = http.server.ThreadingHTTPServer(("127.0.0.1", idp_port), provider_handler(provider))
            server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            server_context.load_cert_chain(str(directory / "idp.pem"), str(directory / "idp.key"))
            server.socket = server_context.wrap_socket(server.socket, server_side=True)
            threading.Thread(target=server.serve_forever, daemon=True).start()

            (directory / "client-secret").write_text(secret + "\n")
            (directory / "session-key").write_bytes(os.urandom(32))
            members = directory / "members.json"

            def write_members(emails):
                members.write_text(json.dumps([{"email": e, "scopes": ["sandboxes", "inventory"],
                                                "principal_id": e.split("@")[0], "team_id": "red"} for e in emails]))
            write_members(["alice@example.com"])
            handle = (args.output / "control.log").open("wb")
            handles.append(handle)
            control = subprocess.Popen(
                [str(args.control), "--store", "memory:", "--port", str(api_port), "--proxy-port", str(proxy_port),
                 "--api-tls-cert", str(directory / "api.pem"), "--api-tls-key", str(directory / "api.key"),
                 "--sso-issuer", issuer, "--sso-client-id", "hypermachine",
                 "--sso-client-secret-file", str(directory / "client-secret"), "--sso-redirect-url", redirect,
                 "--sso-members-file", str(members), "--sso-session-key-file", str(directory / "session-key"),
                 "--sso-provider-ca", str(directory / "ca.pem")],
                env={"PATH": "/usr/bin:/bin", "RUST_LOG": "info"}, stdout=handle, stderr=subprocess.STDOUT)
            processes.append(control)

            def request(method, url, headers=None, body=None):
                parts = urllib.parse.urlsplit(url)
                conn = http.client.HTTPSConnection(parts.hostname, parts.port, context=context, timeout=30)
                try:
                    path = parts.path + ("?" + parts.query if parts.query else "")
                    conn.request(method, path, body=body, headers=headers or {})
                    response = conn.getresponse()
                    return response.status, response.getheaders(), response.read()
                finally:
                    conn.close()

            def header(headers, name):
                return [v for k, v in headers if k.lower() == name.lower()]

            def cookie(headers, name):
                for value in header(headers, "set-cookie"):
                    key, _, rest = value.partition("=")
                    if key == name:
                        return rest.split(";", 1)[0]
                return None

            deadline = time.monotonic() + 30
            while True:
                assert control.poll() is None, "the control plane exited; see control.log"
                try:
                    status, _, _ = request("GET", f"{api}/health")
                    if status == 200:
                        break
                except OSError:
                    pass
                assert time.monotonic() < deadline, "control plane readiness"
                time.sleep(0.1)

            def browser_sign_in(start):
                """Follow a sign-in from `start` to the control plane's callback."""
                status, headers, _ = request("GET", start)
                assert status == 303, status
                pending = cookie(headers, "__Host-hm_login")
                status, headers, _ = request("GET", header(headers, "location")[0])
                assert status == 302, status
                status, headers, body = request("GET", header(headers, "location")[0],
                                                {"Cookie": f"__Host-hm_login={pending}"})
                return status, headers, body

            def anonymous():
                status = request("GET", f"{api}/sandboxes")[0]
                assert status == 401, f"anonymous request answered {status}"
                return {"status": status}
            case("nobody is anonymous with SSO on", anonymous)

            def browser():
                status, headers, _ = browser_sign_in(f"{api}/auth/login?returnTo=/sandboxes")
                assert status == 303 and header(headers, "location") == ["/sandboxes"], status
                session = cookie(headers, "__Host-hm_session")
                assert session and session.startswith("hms1.")
                set_cookie = [v for v in header(headers, "set-cookie") if v.startswith("__Host-hm_session=")][0]
                for attribute in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"]:
                    assert attribute in set_cookie, attribute
                state["cookie"] = {"Cookie": f"__Host-hm_session={session}"}
                status, _, body = request("GET", f"{api}/auth/session", state["cookie"])
                who = json.loads(body)
                assert status == 200 and who["email"] == "alice@example.com" and who["teamID"] == "red", who
                status, _, body = request("GET", f"{api}/sandboxes", state["cookie"])
                assert status == 200 and json.loads(body) == [], (status, body)
                return {"email": who["email"], "cookie_attributes": set_cookie.split(";", 1)[1].strip()}
            state = {}
            case("a browser sign-in ends in a session the API admits", browser)

            def origin():
                body = json.dumps({"templateID": "base"})
                plain = request("POST", f"{api}/sandboxes", {**state["cookie"], "Content-Type": "application/json"}, body)[0]
                foreign = request("POST", f"{api}/sandboxes", {**state["cookie"], "Content-Type": "application/json",
                                                               "Origin": "https://8080-sbx.example"}, body)[0]
                own = request("POST", f"{api}/sandboxes", {**state["cookie"], "Content-Type": "application/json",
                                                           "Origin": f"https://127.0.0.1:{api_port}"}, body)[0]
                assert plain == 403 and foreign == 403 and own not in (401, 403), (plain, foreign, own)
                return {"no_origin": plain, "sandbox_origin": foreign, "own_origin": own}
            case("a cookie change needs this control plane's Origin", origin)

            def outsider():
                provider.email = "mallory@example.com"
                try:
                    status, headers, _ = browser_sign_in(f"{api}/auth/login")
                finally:
                    provider.email = "alice@example.com"
                assert status == 403 and cookie(headers, "__Host-hm_session") is None, status
                return {"status": status}
            case("a provider identity that is not a member gets no session", outsider)

            def cli():
                home = directory / "cli-home"
                home.mkdir()
                env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config")}
                base = [str(args.cli), "sandbox", "vm", "--endpoint", api, "--api-ca-cert", str(directory / "ca.pem")]
                login = subprocess.Popen(base + ["login", "--no-browser", "--wait", "60"], env=env,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                url = None
                while url is None:
                    line = login.stderr.readline()
                    assert line, "the CLI printed no sign-in URL"
                    if line.strip().startswith("https://"):
                        url = line.strip()
                status, headers, _ = browser_sign_in(url)
                assert status == 303, status
                to_cli = header(headers, "location")[0]
                assert to_cli.startswith("http://127.0.0.1:"), to_cli
                parts = urllib.parse.urlsplit(to_cli)
                conn = http.client.HTTPConnection(parts.hostname, parts.port, timeout=10)
                conn.request("GET", parts.path + "?" + parts.query)
                landed = conn.getresponse().status
                conn.close()
                out, err = login.communicate(timeout=30)
                assert login.returncode == 0 and landed == 200, (login.returncode, err)
                signed_in = json.loads(out)
                listed = subprocess.run(base + ["list"], env=env, capture_output=True, text=True, timeout=30)
                assert listed.returncode == 0 and json.loads(listed.stdout) == [], listed.stderr
                stored = home / ".config" / "hypermachine" / "sessions.json"
                mode = oct(stored.stat().st_mode & 0o777)
                assert mode == "0o600", mode
                out = subprocess.run(base + ["logout"], env=env, capture_output=True, text=True, timeout=30)
                assert out.returncode == 0 and json.loads(out.stdout)["signedOut"] is True, out.stderr
                after = subprocess.run(base + ["list"], env=env, capture_output=True, text=True, timeout=30)
                assert after.returncode != 0 and "401" in after.stderr, (after.returncode, after.stderr)
                return {"email": signed_in["email"], "session_file_mode": mode, "after_logout": "401"}
            case("hm sandbox vm login keeps a session the CLI uses, and logout forgets it", cli)

            def removal():
                write_members(["someone-else@example.com"])
                control.send_signal(signal.SIGHUP)
                deadline = time.monotonic() + 10
                while request("GET", f"{api}/sandboxes", state["cookie"])[0] != 401:
                    assert time.monotonic() < deadline, "the reload did not end the session"
                    time.sleep(0.1)
                return {"status": 401}
            case("removing the member ends the session on reload", removal)
            report["success"] = True
        finally:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
            for handle in handles:
                handle.close()
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
