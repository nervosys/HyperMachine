#!/usr/bin/env python3
"""A machine's desktop, and computer use on it, on real KVM.

A machine from a distribution's image is created with `guestAgent`, both
coding agents and `desktop`. Its first boot installs a virtual display with a
window manager and a terminal, noVNC for a browser, and `hv2-computer`,
which acts on the display and is given to each agent as an MCP server. This
reaches the Internet (the distribution's archive and npm), so it is for a
run by hand.

1. What cannot be made is refused, each with why.
2. The desktop installs and says it is up, at the size asked for.
3. A screenshot is a PNG of that size, with something drawn on it.
4. Clicking puts the focus where it clicked: of two terminals, the one
   clicked takes what is typed, and Return runs it.
5. Moving, dragging and scrolling leave the pointer where they say; what
   is not an action, or not on the screen, is refused with why.
6. noVNC, through a port the machine publishes, serves its page and, over
   a WebSocket, the VNC server's greeting.
7. Each coding agent is given computer use: Claude Code connects to the
   MCP server for root and for the default user, Codex lists it, and the
   server, run as that user, lists its tools and takes a screenshot.
8. Started again, the desktop comes back.

No key is given, so no agent uses the desktop here: it is shown that the
agents are given it and that the server works, not what a model does with
it.

Writes report.json, the node's log, the install's log and two screenshots
into --output.
"""
import argparse, base64, hashlib, json, os, shutil, signal, socket, struct, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "desk-01"
USER = "ubuntu"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def png_size(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n", data[:16]
    return struct.unpack(">II", data[16:24])


def websocket_greeting(port_number, path="/websockify"):
    """The first binary message a WebSocket server sends, over a handshake
    done by hand: a VNC server's is its protocol version."""
    key = base64.b64encode(os.urandom(16)).decode()
    with socket.create_connection(("127.0.0.1", port_number), timeout=20) as sock:
        sock.sendall((f"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port_number}\r\nUpgrade: websocket\r\n"
                      f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
                      f"Sec-WebSocket-Protocol: binary\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = sock.recv(4096)
            assert chunk, head
            head += chunk
        status_line = head.split(b"\r\n", 1)[0].decode()
        rest = head.split(b"\r\n\r\n", 1)[1]
        while len(rest) < 2:
            rest += sock.recv(4096)
        length = rest[1] & 0x7F
        offset = 2
        if length == 126:
            while len(rest) < 4:
                rest += sock.recv(4096)
            length = struct.unpack(">H", rest[2:4])[0]
            offset = 4
        while len(rest) < offset + length:
            rest += sock.recv(4096)
        return status_line, rest[0], rest[offset:offset + length]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "firmware", "image", "guest-agent", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a machine's desktop and computer use on KVM; "
                         "no key, no agent using it, no timing claim",
              "input_sha256": {k: digest(getattr(args, k.replace("-", "_")))
                               for k in ["daemon", "kernel", "initrd", "firmware", "image", "guest-agent"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-desktop-check-", dir="/var/tmp"))
    (work / "images").mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(work / "images" / "distro.raw")], check=True)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--image-dir", str(work / "images"),
         "--firmware", str(args.firmware), "--guest-agent", str(args.guest_agent),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=120, raw=False):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            data = response.read()
            if raw:
                return response.status, response.headers.get("content-type"), data
            try:
                return response.status, json.loads(data) if data else None
            except ValueError:
                return response.status, data.decode(errors="replace")

    def out(cmd, seconds=60):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": seconds},
                                timeout=seconds + 30)
        assert status == 200, (status, value)
        return value["stdout"].strip()

    def act(body, want=200):
        status, value = request("POST", f"/machines/{NAME}/desktop/actions", body)
        assert status == want, (body, status, value)
        return value

    def screenshot(label):
        status, kind, data = request("GET", f"/machines/{NAME}/desktop/screenshot", raw=True)
        assert status == 200 and kind == "image/png", (status, kind, data[:200])
        (args.output / f"{label}.png").write_bytes(data)
        return data

    def until_ready(seconds):
        deadline = time.monotonic() + seconds
        while True:
            status, value = request("GET", f"/machines/{NAME}/desktop")
            if status == 200 and value["ready"]:
                return value
            assert status in (200, 409), (status, value)
            assert time.monotonic() < deadline, f"the desktop was not up in {seconds} s: {value}"
            time.sleep(10)

    try:
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)

        def refusals():
            refused = {}
            for label, body in [
                ("a desktop without the guest agent", {"name": "x1", "image": "distro.raw", "desktop": True}),
                ("desktop false", {"name": "x2", "image": "distro.raw", "guestAgent": True, "desktop": False}),
                ("a screen too small", {"name": "x3", "image": "distro.raw", "guestAgent": True,
                                        "desktop": {"width": 320, "height": 200}}),
                ("an odd width", {"name": "x4", "image": "distro.raw", "guestAgent": True,
                                  "desktop": {"width": 1281, "height": 800}}),
            ]:
                status, value = request("POST", "/machines", body)
                assert status == 400, (label, status, value)
                refused[label] = value["message"]
            # A machine without one has none to show.
            status, plain = request("POST", "/machines", {"name": "plain", "image": "distro.raw", "start": False})
            assert status == 201 and plain["desktop"] is None, (status, plain)
            status, value = request("GET", "/machines/plain/desktop")
            assert status == 404, (status, value)
            refused["a machine without a desktop"] = value["message"]
            assert request("DELETE", "/machines/plain")[0] == 204
            return refused
        case("what cannot be made is refused, each with why", refusals)

        def installed():
            status, made = request("POST", "/machines", {
                "name": NAME, "image": "distro.raw", "cpuCount": 2, "memoryMB": 2048, "diskGiB": 10,
                "network": {"allowInternetAccess": True}, "guestAgent": True,
                "userData": "#cloud-config\nusers: [default]\n",
                "agents": ["claude", "codex"], "desktop": True})
            assert status == 201 and made["desktop"] == {"width": 1280, "height": 800}, (status, made)
            started = time.monotonic()
            value = until_ready(1800)
            (args.output / "desktop-install.log").write_text(out("cat /var/log/hv2-desktop.log", 30))
            (args.output / "desktop-journal.log").write_text(
                out("journalctl -u 'hv2-desktop*' --no-pager -n 200", 30))
            units = out("systemctl is-active hv2-desktop hv2-desktop-wm hv2-desktop-terminal "
                        "hv2-desktop-vnc hv2-desktop-web").split()
            assert units == ["active"] * 5, units
            # The terminal is on the screen, and has stayed there.
            time.sleep(5)
            windows = out("DISPLAY=:1 xdotool search --class xterm").split()
            assert len(windows) == 1, windows
            restarts = out("systemctl show -p NRestarts --value hv2-desktop-terminal")
            assert restarts == "0", restarts
            assert (value["width"], value["height"], value["webPort"]) == (1280, 800, 6080), value
            return {"seconds_until_ready": round(time.monotonic() - started), "units": units, "desktop": value}
        case("the desktop installs and says it is up, at the size asked for", installed)

        def pictured():
            data = screenshot("first")
            assert png_size(data) == (1280, 800), png_size(data)
            # The terminal is drawn on it: a screen of one colour compresses
            # to almost nothing.
            assert len(data) > 4000, len(data)
            return {"bytes": len(data), "size": list(png_size(data))}
        case("a screenshot is a PNG of that size, with something drawn on it", pictured)

        def clicked():
            out("rm -f /tmp/clicked-*; DISPLAY=:1 setsid xterm -geometry 60x16+760+440 -T second "
                ">/dev/null 2>&1 < /dev/null &")
            time.sleep(3)
            # The desktop's own terminal is at 40, 40; the second is lower right.
            for (x, y), text in [((300, 200), "first"), ((960, 560), "second"), ((300, 200), "first-again")]:
                assert act({"action": "click", "x": x, "y": y}) == {"cursor": [x, y]}
                act({"action": "type", "text": f"echo {text} > /tmp/clicked-{text}"})
                act({"action": "key", "keys": "Return"})
            time.sleep(2)
            files = {name: out(f"cat /tmp/clicked-{name}") for name in ["first", "second", "first-again"]}
            assert files == {"first": "first", "second": "second", "first-again": "first-again"}, files
            screenshot("after-typing")
            return {"typed_into": files}
        case("a click puts the focus where it clicked, and what is typed runs there", clicked)

        def moved():
            assert act({"action": "move", "x": 100, "y": 700}) == {"cursor": [100, 700]}
            assert act({"action": "drag", "x": 50, "y": 50, "toX": 400, "toY": 300}) == {"cursor": [400, 300]}
            assert act({"action": "scroll", "x": 640, "y": 400, "direction": "down",
                        "amount": 2}) == {"cursor": [640, 400]}
            assert act({"action": "cursor"}) == {"cursor": [640, 400]}
            refused = {}
            for label, body in [
                ("off the screen", {"action": "click", "x": 1280, "y": 10}),
                ("not a key", {"action": "key", "keys": "Return; rm -rf /"}),
                ("no such action", {"action": "fly"}),
                ("nothing to type", {"action": "type", "text": ""}),
                ("not a direction", {"action": "scroll", "x": 1, "y": 1, "direction": "sideways"}),
            ]:
                refused[label] = act(body, want=400)["message"]
            status, value = request("POST", f"/machines/{NAME}/desktop/actions", [1, 2])
            assert status == 400, (status, value)
            return {"refused": refused}
        case("moving, dragging and scrolling leave the pointer where they say; the rest is refused", moved)

        def served():
            status, published = request("POST", f"/machines/{NAME}/ports", {"guestPort": 6080})
            assert status in (200, 201), (status, published)
            host_port = published["hostPort"]
            with urllib.request.urlopen(f"http://127.0.0.1:{host_port}/vnc.html", timeout=30) as page:
                html = page.read().decode(errors="replace")
                assert page.status == 200 and "noVNC" in html, html[:300]
            status_line, opcode, greeting = websocket_greeting(host_port)
            assert status_line.startswith("HTTP/1.1 101"), status_line
            assert opcode == 0x82 and greeting.startswith(b"RFB 003."), (opcode, greeting)
            return {"hostPort": host_port, "page_bytes": len(html), "greeting": greeting.decode().strip()}
        case("noVNC, through a published port, serves its page and the VNC server's greeting", served)

        def given():
            found = {}
            for who in ["root", USER]:
                listed = out(f"su - {who} -c 'claude mcp list' 2>&1", 120)
                line = next((l for l in listed.splitlines() if l.startswith("computer:")), "")
                assert "/usr/local/bin/hv2-computer mcp" in line and "Connected" in line, listed
                found[f"claude, {who}"] = line
                listed = out(f"su - {who} -c 'codex mcp list' 2>&1", 60)
                line = next((l for l in listed.splitlines() if l.startswith("computer ")), "")
                assert "/usr/local/bin/hv2-computer" in line, listed
                found[f"codex, {who}"] = " ".join(line.split())
            # The server itself, as the default user: its tools, and a picture.
            lines = "\n".join(json.dumps(m) for m in [
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
                {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                 "params": {"name": "screenshot", "arguments": {}}},
            ]) + "\n"
            status, run = request("POST", f"/machines/{NAME}/runs",
                                  {"cmd": "/usr/local/bin/hv2-computer mcp", "user": USER, "stdin": lines})
            assert status == 201, (status, run)
            deadline = time.monotonic() + 60
            while True:
                status, run = request("GET", f"/machines/{NAME}/runs/{run['runID']}")
                if run["state"] != "running":
                    break
                assert time.monotonic() < deadline, run
                time.sleep(0.5)
            replies = {r["id"]: r for r in map(json.loads, run["stdout"]["text"].splitlines())}
            tools = [t["name"] for t in replies[2]["result"]["tools"]]
            assert tools == ["screenshot", "move", "click", "drag", "type", "key", "scroll", "cursor_position"], tools
            image = replies[3]["result"]["content"][0]
            assert image["type"] == "image" and image["mimeType"] == "image/png", image.keys()
            assert png_size(base64.b64decode(image["data"])) == (1280, 800)
            found["server, as the default user"] = {"tools": tools, "screenshot_bytes": len(image["data"])}
            return found
        case("each coding agent is given computer use, and the server works for the default user", given)

        def again():
            status, value = request("POST", f"/machines/{NAME}/stop", timeout=180)
            assert status == 200 and value["state"] == "stopped", (status, value)
            status, value = request("POST", f"/machines/{NAME}/start", timeout=180)
            assert status == 200 and value["state"] == "running", (status, value)
            started = time.monotonic()
            until_ready(600)
            deadline = time.monotonic() + 120
            while True:
                status, kind, data = request("GET", f"/machines/{NAME}/desktop/screenshot", raw=True)
                if status == 200:
                    break
                assert time.monotonic() < deadline, (status, data[:300])
                time.sleep(3)
            assert png_size(data) == (1280, 800)
            return {"seconds_until_a_screenshot": round(time.monotonic() - started)}
        case("started again, the desktop comes back", again)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
