#!/usr/bin/env python3
"""How often does creating a sandbox from a template snapshot fail, on KVM?

One failure was seen (docs/benchmarks/2026-11-13/template-user-owner-kvm/restore-timeout-old-image-run0):
`unpacking the repository into /srv/app: making it: Timeout: the guest agent did not answer request 1 in
time`, right after a restore. This runs an owned `hv2-sandboxd` whose base template is the image under test,
optionally builds a template with a non-root USER (`--template user`: COPY /etc/passwd and /etc/group holding
`alice`, `USER alice`, COPY a file owned by alice, WORKDIR), then creates, execs in and deletes sandboxes from
it: `--count` of them, `--parallel` at a time (1 is sequential). Each create is
`POST /sandboxes {templateID, environment}`; with `--env repo` the environment holds a file and a Git
repository (served over plain HTTP on loopback via --git-remote), so the create makes its directories and
unpacks the repository through the guest agent, as the failing run did; `--env none` creates with nothing.

Records per create: latency (the POST's wall time), the HTTP status and the error text if it failed, and the
exec's result (a failure there is recorded as a failure of that iteration too). The daemon runs with debug logging for the restore and
agent paths; its log is kept whole (`--keep-log all`) or only the lines around each failure. Host load
(`/proc/loadavg`) is sampled before, during (every 2 s) and after, and the Windows host's CPU is read by the
caller. Writes report.json and daemon-log.txt into --output, with the number of `Kvm` and `Tcg` platform
lines (Tcg must be 0).
"""
import argparse, concurrent.futures, hashlib, http.server, io, json, os, shutil, signal, socket, statistics
import subprocess, tarfile, tempfile, threading, time, urllib.error, urllib.request
from pathlib import Path

PASSWD = "root:x:0:0:root:/root:/bin/sh\nalice:x:1000:1000::/srv/home:/bin/sh\n"
GROUP = "root:x:0:\nalice:x:1000:\n"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tar_of(name, payload):
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w") as tar:
        info = tarfile.TarInfo(name)
        info.mode = 0o644
        info.size = len(payload)
        tar.addfile(info, io.BytesIO(payload))
    return out.getvalue()


def percentile(values, p):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(round(p / 100 * (len(ordered) - 1))))]


def run_cycles(args):
    """Run this driver once per cycle (a fresh daemon, a fresh build) and merge the reports."""
    import sys
    args.output.mkdir(parents=True, exist_ok=False)
    scratch = Path(tempfile.mkdtemp(prefix="hm-restore-cycles-", dir="/var/tmp"))
    merged = None
    logs = []
    for c in range(args.cycles):
        out = scratch / f"c{c:03d}"
        argv = [sys.executable, __file__, "--daemon", str(args.daemon), "--kernel", str(args.kernel),
                "--initrd", str(args.initrd), "--output", str(out), "--label", args.label,
                "--count", str(args.count), "--parallel", str(args.parallel), "--template", args.template,
                "--env", args.env, "--keep-log", args.keep_log, "--context-lines", str(args.context_lines),
                "--rust-log-extra", args.rust_log_extra, "--slow-ms", str(args.slow_ms)]
        if args.snapshots:
            argv.append("--snapshots")
        done = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            one = json.loads((out / "report.json").read_text())
        except OSError:
            print(f"cycle {c}: no report: {done.stdout[-500:]} {done.stderr[-500:]}", flush=True)
            continue
        for row in one["iterations"]:
            row["cycle"] = c
        if merged is None:
            merged = dict(one, cycles=args.cycles, per_cycle=[])
            merged["iterations"] = []
            merged["platform_lines"] = {"Kvm": 0, "Tcg": 0}
            merged["loadavg_max"] = 0.0
            merged["slow_creates"] = []
            merged["setup_errors"] = []
        merged["iterations"] += one["iterations"]
        merged["slow_creates"] += [dict(x, cycle=c) for x in one["slow_creates"]]
        for k in ("Kvm", "Tcg"):
            merged["platform_lines"][k] += one["platform_lines"][k]
        merged["loadavg_max"] = max(merged["loadavg_max"], one["loadavg_samples"]["max"] or 0.0)
        if "setup_error" in one:
            merged["setup_errors"].append({"cycle": c, "error": one["setup_error"],
                                           "loadavg_before": one.get("loadavg_before"),
                                           "snapshots": one.get("snapshots")})
        merged["per_cycle"].append({"cycle": c, "failures": one["failures"], "runs": one["runs"],
                                    "first_create_ms": one["iterations"][0].get("create_ms") if one["iterations"] else None})
        text = (out / "daemon-log.txt").read_text(errors="replace")
        if text:
            logs.append(f"=== cycle {c} ===" + chr(10) + text)
        print(f"cycle {c}: runs {one['runs']} failures {one['failures']}"
              + (" SETUP-ERROR" if "setup_error" in one else ""), flush=True)
    assert merged is not None, "no cycle produced a report"
    rows = merged["iterations"]
    ok = [r for r in rows if r["ok"]]
    bad = [r for r in rows if not r["ok"]]
    creates = [r["create_ms"] for r in ok]
    merged.update(runs=len(rows), failures=len(bad), failure_rows=bad,
                  create_ms={"n": len(creates), "min": min(creates) if creates else None,
                             "p50": percentile(creates, 50), "p90": percentile(creates, 90),
                             "p95": percentile(creates, 95), "p99": percentile(creates, 99),
                             "max": max(creates) if creates else None,
                             "mean": round(statistics.mean(creates), 1) if creates else None})
    first = [r["create_ms"] for r in ok if r["i"] == 0]
    merged["first_create_after_start_ms"] = {"n": len(first), "p50": percentile(first, 50),
                                              "max": max(first) if first else None}
    merged["setup_failures"] = len(merged["setup_errors"])
    merged["success"] = not bad and not merged["setup_errors"] and merged["platform_lines"]["Tcg"] == 0 and merged["platform_lines"]["Kvm"] >= 1
    (args.output / "daemon-log.txt").write_text(chr(10).join(logs))
    (args.output / "report.json").write_text(json.dumps(merged, indent=2) + chr(10))
    shutil.rmtree(scratch, ignore_errors=True)
    print(f"setup failures {merged['setup_failures']} of {args.cycles} cycles")
    print(f"runs {merged['runs']} failures {merged['failures']} create_ms {merged['create_ms']}")
    print("first creates:", merged["first_create_after_start_ms"])
    print("success:", merged["success"])


def snapshot_summary(path):
    """What a template snapshot captured of the interrupt hardware: read from its JSON header
    (magic, u32 version, u32 length, header), so a restore that hangs can be compared with what it
    was restored from. LAPIC register page: ISR at 0x100, IRR at 0x200 (one u32 per 16 bytes)."""
    import struct
    with open(path, "rb") as f:
        magic = f.read(8)
        if magic != b"HV2SNAP" + bytes(1):
            return {"error": "not a snapshot"}
        _version, length = struct.unpack("<II", f.read(8))
        header = json.loads(f.read(length))
    out = {"file": os.path.basename(str(path)), "vm_name": header.get("vm_name")}

    def vectors(page, base):
        found = []
        for word in range(8):
            value = struct.unpack_from("<I", page, base + 16 * word)[0]
            found += [word * 32 + bit for bit in range(32) if value >> bit & 1]
        return found
    machine = header.get("machine") or {}
    if machine.get("ioapic"):
        io = bytes(machine["ioapic"])
        out["ioapic_irr"] = hex(struct.unpack_from("<I", io, 16)[0])
        out["ioapic_gsi5"] = hex(struct.unpack_from("<Q", io, 24 + 5 * 8)[0])
    if machine.get("pic_master"):
        pm = machine["pic_master"]
        out["pic_master_irr_imr"] = [hex(pm[1]), hex(pm[2])]
    for v in header.get("vcpus", [])[:1]:
        page = bytes(v.get("lapic", []))
        if len(page) >= 0x300:
            out["lapic_irr_vectors"] = vectors(page, 0x200)
            out["lapic_isr_vectors"] = vectors(page, 0x100)
            out["lapic_lvt_timer"] = hex(struct.unpack_from("<I", page, 0x320)[0])
        msrs = {m["index"]: m["value"] for m in v.get("msrs", [])}
        out["tsc"] = msrs.get(0x10)
        out["tsc_deadline"] = msrs.get(0x6E0)
        out["run_state"] = v.get("run_state")
    out["devices"] = [{"name": d.get("name"), "interrupt_status": d["transport"]["interrupt_status"],
                       "status": d["transport"]["status"]} for d in header.get("devices", [])]
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--label", default="")
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--parallel", type=int, default=1)
    parser.add_argument("--template", choices=["base", "user"], default="base")
    parser.add_argument("--env", choices=["repo", "none"], default="repo")
    parser.add_argument("--keep-log", choices=["all", "failures"], default="failures")
    parser.add_argument("--context-lines", type=int, default=60)
    parser.add_argument("--snapshots", action="store_true",
                        help="record what the template snapshots hold of the interrupt hardware (always done "
                             "when setup fails)")
    parser.add_argument("--cycles", type=int, default=1,
                        help="restart the daemon (and rebuild the template) this many times, --count creates "
                             "each: a create right after a build is the case the one failure was")
    parser.add_argument("--slow-ms", type=float, default=2500.0,
                        help="a create slower than this is a near miss: its log lines are kept too")
    parser.add_argument("--rust-log-extra", default="hv2_core::devices::virtio_vsock=debug,hv2_agent=debug")
    args = parser.parse_args()
    if not os.access("/dev/kvm", os.R_OK | os.W_OK):
        raise SystemExit("/dev/kvm is missing or not accessible (modprobe kvm_amd / run as root): "
                         "without it the daemon falls back to Tcg and this measures nothing")
    if args.cycles > 1:
        return run_cycles(args)
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "label": args.label, "count": args.count, "parallel": args.parallel,
              "template": args.template, "env": args.env,
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]}}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-restore-loop-", dir="/var/tmp"))
    kit = work / "kit"
    kit.mkdir()
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=check", "-c", "user.email=check@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env, capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)
        return done.stdout.strip()

    src = work / "src"
    src.mkdir()
    git("init", "-q", "-b", "main", cwd=src)
    (src / "src").mkdir()
    (src / "README.md").write_text("on main\n")
    (src / "src" / "app.py").write_text("print('main')\n")
    git("add", ".", cwd=src)
    git("commit", "-q", "-m", "main", cwd=src)
    served = work / "served"
    served.mkdir()
    shutil.copytree(src / ".git", served / "app.git")
    git("update-server-info", cwd=served / "app.git")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

    git_port = port()
    web = http.server.ThreadingHTTPServer(
        ("127.0.0.1", git_port), lambda *a, **k: Handler(*a, directory=str(served), **k))
    threading.Thread(target=web.serve_forever, daemon=True).start()
    remote = f"http://127.0.0.1:{git_port}/app.git"

    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    rust_log = ("info,hv2_sandboxd::env_vars=debug,hv2_sandboxd::builds=debug,hv2_sandboxd=debug,"
                + args.rust_log_extra)
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(kit),
         "--git-remote", remote, "--memory-mb", "512", "--cpu-cores", "1",
         "--capacity", str(max(6, args.parallel * 2))],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG=rust_log),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300, data=None):
        payload = data if data is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(base + path, method=method,
                                     headers={"content-type": "application/json"}, data=payload)
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            try:
                return response.status, json.loads(raw) if raw else None
            except ValueError:
                return response.status, raw.decode(errors="replace")

    def loadavg():
        return Path("/proc/loadavg").read_text().split()[0]

    loads = []
    stop_sampling = threading.Event()

    def sample_load():
        while not stop_sampling.wait(2):
            loads.append(float(loadavg()))
    sampler = threading.Thread(target=sample_load, daemon=True)

    results = []
    lock = threading.Lock()
    try:
        deadline = time.monotonic() + 240
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        report["loadavg_before"] = loadavg()

        template = "base"
        if args.template == "user":
            name = "tuser"
            status, made = request("POST", "/v3/templates", {"name": name})
            assert status == 202, (status, made)
            uploads = {"passwd": tar_of("passwd", PASSWD.encode()), "group": tar_of("group", GROUP.encode()),
                       "owned": tar_of("owned.txt", b"owned\n")}
            hashes = {}
            for label, payload in uploads.items():
                h = hashlib.sha256(payload).hexdigest()
                hashes[label] = h
                status, link = request("GET", f"/templates/{name}/files/{h}")
                assert status == 201, (status, link)
                if not link["present"]:
                    with urllib.request.urlopen(urllib.request.Request(link["url"], method="PUT", data=payload),
                                                timeout=60) as r:
                        assert r.status == 200, r.status
            steps = [
                {"type": "COPY", "args": ["passwd", "/etc/passwd"], "filesHash": hashes["passwd"]},
                {"type": "COPY", "args": ["group", "/etc/group"], "filesHash": hashes["group"]},
                {"type": "USER", "args": ["alice"]},
                {"type": "COPY", "args": ["owned.txt", "/srv/work/owned.txt", "alice"], "filesHash": hashes["owned"]},
                {"type": "WORKDIR", "args": ["/srv/work/sub"]},
            ]
            status, value = request("POST", f"/v2/templates/{name}/builds/{made['buildID']}",
                                    {"fromTemplate": "base", "steps": steps})
            assert status == 202, (status, value)
            t_end = time.monotonic() + 600
            while True:
                status, value = request("GET", f"/templates/{name}/builds/{made['buildID']}/status")
                assert status == 200, (status, value)
                if value["status"] in ("ready", "error"):
                    break
                assert time.monotonic() < t_end, "build"
                time.sleep(0.5)
            report["build"] = {"status": value["status"], "reason": value.get("reason")}
            assert value["status"] == "ready", value
            template = name
        report["template_id"] = template

        environment = None
        if args.env == "repo":
            status, value = request("PUT", "/environments/rl", {
                "files": [{"path": "/etc/app/config.toml", "content": "port = 9090\n"}],
                "repositories": [dict(kind="git", remote=remote, path="/srv/app")]})
            assert status == 201, (status, value)
            environment = "rl"

        def one(i):
            row = {"i": i, "ok": False}
            wall = time.time()
            started = time.monotonic()
            body = {"templateID": template, "timeout": 600}
            if environment:
                body["environment"] = environment
            sandbox = None
            try:
                status, made = request("POST", "/sandboxes", body, timeout=120)
                row["create_ms"] = round((time.monotonic() - started) * 1000, 1)
                row["create_status"] = status
                if status not in (200, 201):
                    row["error"] = made if isinstance(made, str) else json.dumps(made)
                    return row
                sandbox = made["sandboxID"]
                row["sandbox"] = sandbox
                t = time.monotonic()
                status, value = request("POST", f"/sandboxes/{sandbox}/exec",
                                        {"argv": ["/bin/busybox", "echo", "hi"], "timeout_secs": 30})
                row["exec_ms"] = round((time.monotonic() - t) * 1000, 1)
                row["exec_status"] = status
                if status != 200 or "hi" not in json.dumps(value):
                    row["error"] = "exec: " + (value if isinstance(value, str) else json.dumps(value))
                    return row
                row["ok"] = True
            except Exception as error:
                row["error"] = f"{type(error).__name__}: {error}"
            finally:
                if sandbox:
                    try:
                        request("DELETE", f"/sandboxes/{sandbox}", timeout=60)
                    except Exception as error:
                        row["delete_error"] = repr(error)
                row["wall_start"] = wall
                row["total_ms"] = round((time.monotonic() - started) * 1000, 1)
                with lock:
                    results.append(row)
                    if not row["ok"]:
                        print(f"FAIL iteration {i}: {row.get('error')}", flush=True)
            return row

        sampler.start()
        t0 = time.monotonic()
        if args.parallel <= 1:
            for i in range(args.count):
                one(i)
        else:
            with concurrent.futures.ThreadPoolExecutor(args.parallel) as pool:
                list(pool.map(one, range(args.count)))
        report["elapsed_s"] = round(time.monotonic() - t0, 1)
    except Exception as error:
        # Setup (the daemon, the template build) failing is a finding too: kept, with the whole log.
        report["setup_error"] = f"{type(error).__name__}: {error}"[:4000]
        print("SETUP FAIL:", report["setup_error"][:300], flush=True)
    finally:
        stop_sampling.set()
        if args.snapshots or "setup_error" in report:
            import glob
            report["snapshots"] = []
            for snap in sorted(glob.glob(f"/tmp/hv2-sandboxd-{daemon.pid}-*/**/*.snap", recursive=True)):
                try:
                    report["snapshots"].append(snapshot_summary(snap))
                except Exception as inner:
                    report["snapshots"].append({"file": snap, "error": repr(inner)})
        report["loadavg_after"] = loadavg()
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        web.shutdown()
        lines = log_path.read_text(errors="replace").splitlines()
        count = lambda needle: sum(1 for l in lines if needle in l)
        report["platform_lines"] = {"Kvm": count("Detected hypervisor platform: Kvm"),
                                    "Tcg": count("Detected hypervisor platform: Tcg")}
        results.sort(key=lambda r: r["i"])
        ok = [r for r in results if r["ok"]]
        bad = [r for r in results if not r["ok"]]
        creates = [r["create_ms"] for r in ok]
        report["slow_creates"] = [{"i": r["i"], "sandbox": r.get("sandbox"), "create_ms": r["create_ms"]}
                                  for r in ok if r["create_ms"] >= args.slow_ms]
        report["runs"] = len(results)
        report["failures"] = len(bad)
        report["failure_rows"] = bad
        report["create_ms"] = {
            "n": len(creates),
            "min": min(creates) if creates else None,
            "p50": percentile(creates, 50), "p90": percentile(creates, 90), "p95": percentile(creates, 95),
            "p99": percentile(creates, 99), "max": max(creates) if creates else None,
            "mean": round(statistics.mean(creates), 1) if creates else None}
        report["loadavg_samples"] = {"n": len(loads), "max": max(loads) if loads else None,
                                     "mean": round(statistics.mean(loads), 2) if loads else None}
        report["iterations"] = results
        # The log: whole, or the lines around the first line naming each failed sandbox.
        keep = set()
        if args.keep_log == "all" or "setup_error" in report:
            keep = set(range(len(lines)))
        else:
            for r in bad + [r for r in ok if r["create_ms"] >= args.slow_ms]:
                ids = [r.get("sandbox")] if r.get("sandbox") else []
                hits = [n for n, l in enumerate(lines) if ids and ids[0] in l]
                if not hits:
                    # a create that never got an id: around the timestamp's second
                    stamp = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(r["wall_start"]))
                    hits = [n for n, l in enumerate(lines) if stamp in l][:1]
                for n in hits[:1] + hits[-1:]:
                    keep.update(range(max(0, n - args.context_lines), min(len(lines), n + args.context_lines)))
        kept = [lines[n] for n in sorted(keep)]
        log_path.write_text("\n".join(kept) + ("\n" if kept else ""))
        report["daemon_log_lines"] = {"total": len(lines), "kept": len(kept)}
        report["success"] = report["failures"] == 0 and "setup_error" not in report and report["platform_lines"]["Tcg"] == 0 \
            and report["platform_lines"]["Kvm"] >= 1
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print(f"runs {report['runs']} failures {report['failures']} create_ms {report['create_ms']}")
    print("success:", report["success"])


if __name__ == "__main__":
    main()
