#!/usr/bin/env python3
"""A real browser draws a machine's desktop through noVNC, on real KVM.

`check-machine-desktop-kvm.py` showed that a machine made with `desktop: true`
serves noVNC's page and the VNC server's greeting through a published port.
No browser drew the screen. This does: headless Chromium (driven by
Playwright) opens `/vnc.html` through the published port, noVNC connects,
and the canvas it draws is read back.

1. The run was on KVM: the daemon's log says `platform: Kvm` and never
   `platform: Tcg`.
2. The desktop installs and says it is up (the same machine as the older
   check: Ubuntu 24.04's image as published, both agents, 1280x800).
3. noVNC, in the browser, connects, and its canvas is the guest's size
   with something drawn on it: not one colour, and more than a sliver of
   it is not the background.
4. The canvas and the machine API's own screenshot are the same size and
   close: the mean absolute difference per channel is under a threshold.
5. Through the browser alone (a click and keys into the canvas), a command
   is typed into the guest's terminal. The canvas changes, the change lies
   inside the terminal's window, the browser's pointer is where the guest's
   X server has it, and the guest holds the file the command wrote.

No sound, clipboard or WebRTC is exercised: noVNC is a picture and input.

Needs `playwright` (importable) and a Chromium: give --browser-path, or let
Playwright find its own under PLAYWRIGHT_BROWSERS_PATH. Pillow is used to
read the PNGs. Writes report.json, the daemon's log, the install's log and
small PNGs into --output.
"""
import argparse, base64, hashlib, io, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

NAME = "desk-01"
W, H = 1280, 800
FRAME = 30  # pixels the window manager draws around a window
MAX_MEAN_DIFF = 2.0  # of 255, per channel, canvas against the API's screenshot


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


def decode(data):
    return Image.open(io.BytesIO(data)).convert("RGB")


def colours(image):
    found = image.getcolors(maxcolors=W * H) or []
    found.sort(reverse=True)
    total = image.width * image.height
    return {"distinct": len(found), "dominant_fraction": round(found[0][0] / total, 4),
            "dominant": list(found[0][1])}


def mean_diff(a, b):
    return round(sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3, 4)


def keep(image, path, scale=2):
    """Save a PNG, small: the shown screen, not a full-size copy."""
    image.resize((image.width // scale, image.height // scale), Image.LANCZOS).save(path, optimize=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "firmware", "image", "guest-agent", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--browser-path", type=Path, help="a Chromium executable; default: Playwright's own")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "a real browser draws a machine's desktop through noVNC on KVM; "
                         "no sound, clipboard or WebRTC",
              "input_sha256": {k: digest(getattr(args, k.replace("-", "_")))
                               for k in ["daemon", "kernel", "initrd", "firmware", "image", "guest-agent"]},
              "cases": [], "timings_seconds": {}}
    report["input_sha256"]["driver"] = digest(__file__)
    if args.browser_path:
        report["input_sha256"]["browser"] = digest(args.browser_path)
    work = Path(tempfile.mkdtemp(prefix="hm-desktop-browser-", dir="/var/tmp"))
    (work / "images").mkdir()
    subprocess.run(["cp", "--sparse=always", str(args.image), str(work / "images" / "distro.raw")], check=True)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon.log"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--image-dir", str(work / "images"),
         "--firmware", str(args.firmware), "--guest-agent", str(args.guest_agent),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    timings = report["timings_seconds"]
    clock = time.monotonic()

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

    def api_screenshot():
        status, kind, data = request("GET", f"/machines/{NAME}/desktop/screenshot", raw=True)
        assert status == 200 and kind == "image/png", (status, kind, data[:200])
        return decode(data)

    def until_ready(seconds):
        deadline = time.monotonic() + seconds
        while True:
            status, value = request("GET", f"/machines/{NAME}/desktop")
            if status == 200 and value["ready"]:
                return value
            assert status in (200, 409), (status, value)
            assert time.monotonic() < deadline, f"the desktop was not up in {seconds} s: {value}"
            time.sleep(10)

    browser_process = {}
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

        def installed():
            started = time.monotonic()
            status, made = request("POST", "/machines", {
                "name": NAME, "image": "distro.raw", "cpuCount": 2, "memoryMB": 2048, "diskGiB": 10,
                "network": {"allowInternetAccess": True}, "guestAgent": True,
                "userData": "#cloud-config\nusers: [default]\n",
                "agents": ["claude", "codex"], "desktop": True})
            assert status == 201 and made["desktop"] == {"width": W, "height": H}, (status, made)
            value = until_ready(1800)
            timings["desktop_ready"] = round(time.monotonic() - started, 1)
            (args.output / "desktop-install.log").write_text(out("cat /var/log/hv2-desktop.log", 30))
            units = out("systemctl is-active hv2-desktop hv2-desktop-wm hv2-desktop-terminal "
                        "hv2-desktop-vnc hv2-desktop-web").split()
            assert units == ["active"] * 5, units
            time.sleep(5)
            windows = out("DISPLAY=:1 xdotool search --class xterm").split()
            assert len(windows) == 1, windows
            assert (value["width"], value["height"], value["webPort"]) == (W, H, 6080), value
            return {"units": units, "desktop": value, "terminal_window": windows[0]}
        case("the desktop installs and says it is up", installed)

        status, published = request("POST", f"/machines/{NAME}/ports", {"guestPort": 6080})
        assert status in (200, 201), (status, published)
        host_port = published["hostPort"]
        report["published_port"] = host_port

        from playwright.sync_api import sync_playwright
        shots = {}
        with sync_playwright() as pw:
            launch = {"headless": True, "args": ["--no-sandbox"]}
            if args.browser_path:
                launch["executable_path"] = str(args.browser_path)
            browser = pw.chromium.launch(**launch)
            report["browser"] = {"version": browser.version, "executable": args.browser_path and "given"}
            context = browser.new_context(viewport={"width": 1500, "height": 950})
            page = context.new_page()
            console = []
            page.on("console", lambda m: console.append(f"{m.type}: {m.text}"))
            page.on("pageerror", lambda e: console.append(f"pageerror: {e}"))

            def canvas_png():
                data = page.evaluate("document.querySelector('canvas').toDataURL('image/png')")
                assert data.startswith("data:image/png;base64,"), data[:40]
                return decode(base64.b64decode(data.split(",", 1)[1]))

            def connected():
                started = time.monotonic()
                page.goto(f"http://127.0.0.1:{host_port}/vnc.html?autoconnect=true&resize=off&reconnect=false",
                          wait_until="load", timeout=60000)
                page.wait_for_selector("canvas", state="attached", timeout=60000)
                page.wait_for_function(
                    f"(() => {{ const c = document.querySelector('canvas'); return c && c.width === {W} && c.height === {H}; }})()",
                    timeout=60000)
                # noVNC has drawn the first frames when the canvas is not black.
                deadline = time.monotonic() + 60
                while True:
                    image = canvas_png()
                    if colours(image)["distinct"] > 1 or time.monotonic() > deadline:
                        break
                    time.sleep(1)
                timings["browser_connected_and_drawn"] = round(time.monotonic() - started, 1)
                status_text = page.evaluate(
                    "(document.querySelector('#noVNC_status') || {}).textContent || ''")
                page.screenshot(path=str(work / "page.png"))
                keep(Image.open(work / "page.png").convert("RGB"), args.output / "browser-page.png")
                return {"canvas_size": [image.width, image.height], "status_text": status_text.strip(),
                        "title": page.title()}
            case("noVNC connects in a browser and its canvas is the guest's size", connected)

            def drawn():
                image = canvas_png()
                shots["first"] = image
                keep(image, args.output / "canvas-first.png")
                found = colours(image)
                # Not one colour, and not nearly all of it one colour.
                assert found["distinct"] >= 4, found
                assert found["dominant_fraction"] < 0.995, found
                return found
            case("the canvas is not blank", drawn)

            def similar():
                first = shots["first"]
                reference = api_screenshot()
                assert reference.size == first.size == (W, H), (reference.size, first.size)
                difference = mean_diff(first, reference)
                differing = ImageChops.difference(first, reference).convert("L").point(
                    lambda v: 255 if v > 32 else 0).histogram()[255]
                keep(reference, args.output / "api-screenshot.png")
                assert difference < MAX_MEAN_DIFF, difference
                return {"size": [W, H], "mean_abs_diff_per_channel_of_255": difference,
                        "threshold": MAX_MEAN_DIFF, "pixels_differing_by_over_32": differing,
                        "api_colours": colours(reference)}
            case("the canvas and the API's own screenshot are the same size and close", similar)

            def interacted():
                geometry = out("DISPLAY=:1 xdotool getwindowgeometry --shell "
                               f"{report['cases'][0]['detail']['terminal_window']}")
                g = dict(line.split("=") for line in geometry.splitlines())
                region = (int(g["X"]), int(g["Y"]), int(g["X"]) + int(g["WIDTH"]), int(g["Y"]) + int(g["HEIGHT"]))
                box = page.evaluate("(() => { const r = document.querySelector('canvas').getBoundingClientRect();"
                                    " return [r.x, r.y, r.width, r.height]; })()")
                assert abs(box[2] - W) < 2 and abs(box[3] - H) < 2, box  # shown at 1:1
                target = (region[0] + 200, region[1] + 100)
                px, py = box[0] + target[0], box[1] + target[1]
                started = time.monotonic()
                page.mouse.move(px - 40, py - 30)
                page.mouse.move(px, py, steps=5)
                page.mouse.click(px, py)
                page.keyboard.type("echo typed-in-the-browser > /tmp/browser-typed; echo done-by-browser",
                                   delay=25)
                page.keyboard.press("Enter")
                time.sleep(3)
                timings["browser_interaction"] = round(time.monotonic() - started, 1)
                second = canvas_png()
                keep(second, args.output / "canvas-after-typing.png")
                diff = ImageChops.difference(shots["first"], second).convert("L").point(lambda v: 255 if v > 32 else 0)
                changed = diff.histogram()[255]
                bbox = diff.getbbox()
                assert changed > 200 and bbox, (changed, bbox)
                # The window manager's frame (title bar, border) is outside the
                # client area xdotool reports, and repaints when focus moves.
                frame = (region[0] - FRAME, region[1] - FRAME, region[2] + FRAME, region[3] + FRAME)
                inside = (bbox[0] >= frame[0] and bbox[1] >= frame[1]
                          and bbox[2] <= frame[2] and bbox[3] <= frame[3])
                assert inside, {"changed_bbox": bbox, "terminal_window_with_frame": frame}
                pointer = out("DISPLAY=:1 xdotool getmouselocation --shell")
                where = dict(line.split("=") for line in pointer.splitlines())
                assert (int(where["X"]), int(where["Y"])) == target, (where, target)
                wrote = out("cat /tmp/browser-typed")
                assert wrote == "typed-in-the-browser", wrote
                # The API's screenshot sees the same screen the browser now draws.
                after = api_screenshot()
                keep(after, args.output / "api-screenshot-after-typing.png")
                again = mean_diff(second, after)
                assert again < MAX_MEAN_DIFF, again
                return {"terminal_window": list(region), "clicked_at": list(target),
                        "pixels_changed": changed, "changed_bbox": list(bbox),
                        "changed_bbox_inside_terminal_window_and_frame": inside, "frame_allowance_px": FRAME,
                        "guest_pointer": [int(where["X"]), int(where["Y"])],
                        "file_in_guest": wrote, "canvas_vs_api_after_typing": again}
            case("typing into the canvas changes it, inside the terminal, and reaches the guest", interacted)

            report["browser_console"] = console[-20:]
            context.close()
            browser.close()

        def on_kvm():
            text = log_path.read_text(errors="replace")
            kvm, tcg = text.count("platform: Kvm"), text.count("platform: Tcg")
            assert kvm > 0 and tcg == 0, (kvm, tcg)
            return {"platform_Kvm": kvm, "platform_Tcg": tcg}
        log.flush()
        case("it ran on KVM, not TCG", on_kvm)
        timings["total"] = round(time.monotonic() - clock, 1)
        report["success"] = all(c["success"] for c in report["cases"])
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
