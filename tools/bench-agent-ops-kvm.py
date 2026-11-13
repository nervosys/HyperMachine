#!/usr/bin/env python3
"""Timing of node-initiated guest steps: the guest agent's file operations against the shell's `tar`, on KVM.

The agent path makes one vsock round trip per file operation (several per file); the old shell path
did one `tar`. This measures how much slower that is on a tree of N small files (about 1 KiB each, about
50 files per directory, nested two levels, a few symlinks), for three paths:

repo     An environment's Git repository unpacked into a new sandbox (env_vars.rs unpack_by_agent against
         the tar shell script): time from POST /sandboxes to its answer. A baseline create whose repository
         holds one file is measured in the same round and subtracted.
archive  GET /sandboxes/{id}/checkpoints/{name}/archive?path= of that tree (checkpoint_files.rs
         build_archive against the ARCHIVE script): time of the request. The checkpoint itself is timed
         separately, not included.
copy     A template build whose one COPY step carries an upload of N files (builds.rs copy_by_agent against
         the tar script): time from POST .../builds/{id} to status ready. A baseline build with a one-file
         upload is subtracted. The built template's tree is then read back with the archive route.

Two daemons are started, one per image (the agent image: an agent with the file operations; the shell
image: an agent from before them, which makes the daemon fall back to the shell), and the runs alternate
between them, so a change in the host's load hits both. Every result is checked for completeness: the
archive of the tree holds exactly the files (byte for byte, with modes), directories and symlinks
generated, so a fast but wrong result cannot pass. A failure at some size (a 504 from the archive's 180 s
deadline, a 413, a build error) is recorded as the finding, with its message, and the run goes on.

Before each batch the Windows host's CPU load is read through powershell.exe (when this runs in WSL;
set HM_POWERSHELL to its path if it is not on PATH) and
the loadavg; the batch waits while the Windows load is over --max-load percent.

Writes report.json, summary.md and each daemon's log (trimmed) into --output.
"""
import argparse, hashlib, http.server, io, json, os, shutil, signal, socket, statistics, subprocess, tarfile
import tempfile, threading, time
import urllib.error, urllib.parse, urllib.request
from pathlib import Path

FILE_BYTES = 1024
PER_DIR = 50
LOG_KEEP = 400_000  # bytes of a daemon log kept from each end


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def content(k):
    return (hashlib.sha256(str(k).encode()).digest() * (FILE_BYTES // 32))[:FILE_BYTES]


def tree_spec(n):
    """The tree: {relative name: ("dir", mode) | ("file", mode, bytes) | ("link", target)}."""
    spec = {}
    for k in range(n):
        d = k // PER_DIR
        top, sub = f"d{d // 10:03d}", f"s{d % 10}"
        spec[top] = ("dir", 0o755)
        spec[f"{top}/{sub}"] = ("dir", 0o755)
        spec[f"{top}/{sub}/f{k:05d}.dat"] = ("file", 0o755 if k % 10 == 0 else 0o644, content(k))
    spec["link-file"] = ("link", "d000/s0/f00000.dat")
    spec["link-dir"] = ("link", "d000")
    spec["d000/s0/link-rel"] = ("link", "f00000.dat")
    return spec


def counts(spec):
    kinds = [v[0] for v in spec.values()]
    return {"files": kinds.count("file"), "dirs": kinds.count("dir"), "links": kinds.count("link")}


def ops_estimate(path, spec):
    """Agent operations by reading the code: repo and copy make 2 per directory, and for a file
    (copy) stat + write + set_attr or (repo) write + set_attr; archive 1 listing per directory
    (+ the root) and 1 read per file or link."""
    c = counts(spec)
    if path == "repo":
        return 2 * (c["files"] + c["dirs"]) + c["links"]
    if path == "copy":
        return 2 * c["dirs"] + 3 * c["files"] + 3 * c["links"]
    return c["dirs"] + 1 + c["files"] + c["links"]


def upload_tar(spec):
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w") as tar:
        root = tarfile.TarInfo("tree")
        root.type, root.mode = tarfile.DIRTYPE, 0o755
        tar.addfile(root)
        for name in sorted(spec):
            v = spec[name]
            info = tarfile.TarInfo("tree/" + name)
            if v[0] == "dir":
                info.type, info.mode = tarfile.DIRTYPE, v[1]
                tar.addfile(info)
            elif v[0] == "link":
                info.type, info.mode, info.linkname = tarfile.SYMTYPE, 0o777, v[1]
                tar.addfile(info)
            else:
                info.mode, info.size = v[1], len(v[2])
                tar.addfile(info, io.BytesIO(v[2]))
    return out.getvalue()


def check_archive(data, spec):
    """(ok, detail): the gzipped tar holds exactly `spec`, bytes and modes included."""
    tar = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
    got = {}
    for m in tar.getmembers():
        name = m.name.removeprefix("./").rstrip("/")
        if name in ("", "."):
            continue
        if m.isdir():
            got[name] = ("dir", m.mode & 0o777)
        elif m.issym():
            got[name] = ("link", m.linkname)
        elif m.isfile():
            got[name] = ("file", m.mode & 0o777, tar.extractfile(m).read())
        else:
            got[name] = ("other", m.type)
    missing = sorted(set(spec) - set(got))
    extra = sorted(set(got) - set(spec))
    wrong = []
    for name in sorted(set(spec) & set(got)):
        want, have = spec[name], got[name]
        if want[0] != have[0] or (want[0] == "file" and (want[2] != have[2] or want[1] != have[1])) \
                or (want[0] == "link" and want[1] != have[1]) or (want[0] == "dir" and want[1] != have[1]):
            wrong.append(name)
    ok = not (missing or extra or wrong)
    detail = {"members": len(got), "expected": len(spec), "missing": missing[:5], "missing_n": len(missing),
              "extra": extra[:5], "extra_n": len(extra), "wrong": wrong[:5], "wrong_n": len(wrong)}
    return ok, detail


def windows_load():
    ps = os.environ.get("HM_POWERSHELL") or shutil.which("powershell.exe")
    if not ps:
        return None
    try:
        done = subprocess.run([ps, "-NoProfile", "-Command",
                               "(Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average"],
                              capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL)
        return float(done.stdout.strip())
    except (ValueError, subprocess.SubprocessError, OSError) as error:
        print("windows load unreadable:", repr(error), flush=True)
        return None


def loadavg():
    return Path("/proc/loadavg").read_text().split()[:3]


def median_range(values):
    values = sorted(values)
    return {"median": statistics.median(values), "min": values[0], "max": values[-1], "n": len(values)}


class Node:
    """One daemon on one image."""

    def __init__(self, label, args, work, remotes):
        self.label, self.args = label, args
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        self.log_path = work / f"daemon-{label}.log"
        self.log = self.log_path.open("wb")
        (work / f"home-{label}").mkdir()
        (work / f"kit-{label}").mkdir()
        initrd = args.initrd_agent if label == "agent" else args.initrd_shell
        rust_log = "info,hv2_sandboxd::env_vars=debug,hv2_sandboxd::builds=debug,hv2_sandboxd::checkpoint_files=debug"
        argv = [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()),
                "--guest-kit", str(work / f"kit-{label}"), "--snapshot-store", str(work / f"snap-{label}"),
                "--memory-mb", str(args.memory_mb), "--cpu-cores", "1", "--capacity", "4"]
        for r in remotes:
            argv += ["--git-remote", r]
        self.proc = subprocess.Popen(
            argv, env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(initrd), RUST_LOG=rust_log,
                           HOME=str(work / f"home-{label}")),
            stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT)

    def request(self, method, path, body=None, timeout=900, raw=False, url=None, data=None):
        payload = data if data is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(url or self.base + path, method=method,
                                     headers={"content-type": "application/json"}, data=payload)
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            blob = response.read()
            if raw and response.status == 200:
                return response.status, blob, dict(response.headers)
            try:
                return response.status, (json.loads(blob) if blob else None), dict(response.headers)
            except ValueError:
                return response.status, blob.decode(errors="replace"), dict(response.headers)

    def ready(self):
        deadline = time.monotonic() + 240
        while True:
            assert self.proc.poll() is None, f"the {self.label} daemon exited"
            try:
                if self.request("GET", "/sandboxes", timeout=5)[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

    def stop(self):
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()

    # --- operations -----------------------------------------------------------------------------

    def create_sandbox(self, environment=None, template="base"):
        body = {"templateID": template, "timeout": 3000}
        if environment:
            body["environment"] = environment
        t0 = time.monotonic()
        status, made, _ = self.request("POST", "/sandboxes", body)
        took = time.monotonic() - t0
        return took, status, made

    def delete_sandbox(self, sandbox):
        try:
            self.request("DELETE", f"/sandboxes/{sandbox}", timeout=120)
        except OSError:
            pass

    def checkpoint(self, sandbox, name="one"):
        t0 = time.monotonic()
        status, value, _ = self.request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": name})
        return time.monotonic() - t0, status, value

    def archive(self, sandbox, path, name="one"):
        t0 = time.monotonic()
        status, data, headers = self.request(
            "GET", f"/sandboxes/{sandbox}/checkpoints/{name}/archive?" + urllib.parse.urlencode({"path": path}),
            raw=True)
        return time.monotonic() - t0, status, data, headers

    def build(self, name, spec):
        """Upload and build one COPY of the tree: (seconds of the build, status, reason)."""
        upload = upload_tar(spec)
        status, made, _ = self.request("POST", "/v3/templates", {"name": name})
        assert status == 202, (status, made)
        h = sha(upload)
        status, link, _ = self.request("GET", f"/templates/{name}/files/{h}")
        assert status == 201, (status, link)
        if not link["present"]:
            with urllib.request.urlopen(urllib.request.Request(link["url"], method="PUT", data=upload),
                                        timeout=300) as r:
                assert r.status == 200, r.status
        steps = [{"type": "COPY", "args": ["tree", "/opt/tree"], "filesHash": h}]
        t0 = time.monotonic()
        status, value, _ = self.request("POST", f"/v2/templates/{name}/builds/{made['buildID']}",
                                        {"fromTemplate": "base", "steps": steps,
                                         "force": True})  # force: no cached layer
        assert status == 202, (status, value)
        deadline = time.monotonic() + 1500
        while True:
            status, value, _ = self.request("GET", f"/templates/{name}/builds/{made['buildID']}/status")
            assert status == 200, (status, value)
            if value["status"] in ("ready", "error"):
                return time.monotonic() - t0, value["status"], value.get("reason") or {}
            assert time.monotonic() < deadline, ("build", name, value["status"])
            time.sleep(0.1)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for name in ["daemon", "kernel", "initrd-agent", "initrd-shell", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--sizes", default="100,1000,5000", help="files in the tree, comma separated")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--memory-mb", type=int, default=1024)
    parser.add_argument("--paths", default="repo,archive,copy")
    parser.add_argument("--max-load", type=float, default=30.0, help="Windows CPU percent above which a batch waits")
    parser.add_argument("--max-wait", type=float, default=900.0, help="seconds a batch waits for a quiet host")
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    sizes = [int(s) for s in args.sizes.split(",")]
    paths = args.paths.split(",")
    report = {"label": args.label, "sizes": sizes, "runs": args.runs, "paths": paths, "memory_mb": args.memory_mb,
              "input_sha256": {k: digest(getattr(args, k.replace("-", "_"))) for k in
                               ["daemon", "kernel", "initrd-agent", "initrd-shell"]},
              "host_load": [], "measurements": [], "host_clone_seconds": {}}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-bench-agent-ops-", dir="/var/tmp"))
    git_env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", HOME=str(work))
    identity = ["-c", "user.name=bench", "-c", "user.email=bench@example.invalid"]

    def git(*argv, cwd):
        done = subprocess.run(["git", *identity, *argv], cwd=cwd, env=git_env, capture_output=True, text=True)
        assert done.returncode == 0, (argv, done.stderr)

    # One repository per size (plus the one-file baseline), the tree under data/, served as a dumb
    # HTTP repository, packed so the clone's .git holds a few files, not one per object.
    served = work / "served"
    served.mkdir()
    specs = {n: tree_spec(n) for n in sorted(set(sizes + [1]))}
    for n, spec in specs.items():
        src = work / f"src{n}"
        (src / "data").mkdir(parents=True)
        git("init", "-q", "-b", "main", cwd=src)
        for name, v in sorted(spec.items()):
            at = src / "data" / name
            if v[0] == "dir":
                at.mkdir(parents=True, exist_ok=True)
            elif v[0] == "link":
                at.parent.mkdir(parents=True, exist_ok=True)
                os.symlink(v[1], at)
            else:
                at.parent.mkdir(parents=True, exist_ok=True)
                at.write_bytes(v[2])
                at.chmod(v[1])
        git("add", ".", cwd=src)
        git("commit", "-q", "-m", "tree", cwd=src)
        git("gc", "-q", cwd=src)
        shutil.copytree(src / ".git", served / f"app{n}.git")
        git("update-server-info", cwd=served / f"app{n}.git")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

    git_port = port()
    web = http.server.ThreadingHTTPServer(
        ("127.0.0.1", git_port), lambda *a, **k: Handler(*a, directory=str(served), **k))
    threading.Thread(target=web.serve_forever, daemon=True).start()
    remotes = {n: f"http://127.0.0.1:{git_port}/app{n}.git" for n in specs}

    # What the host alone spends cloning and packing the tree, common to both paths (informational).
    for n in sizes:
        spent = []
        for _ in range(3):
            dest = work / "hostclone"
            t0 = time.monotonic()
            git("clone", "-q", remotes[n], str(dest), cwd=work)
            spent.append(time.monotonic() - t0)
            shutil.rmtree(dest)
        report["host_clone_seconds"][n] = median_range(spent)

    nodes = {}
    try:
        for label in ["agent", "shell"]:
            nodes[label] = Node(label, args, work, list(remotes.values()))
        for node in nodes.values():
            node.ready()
        for n in specs:
            for node in nodes.values():
                status, value, _ = node.request("PUT", f"/environments/bench{n}", {
                    "repositories": [dict(kind="git", remote=remotes[n], path="/srv/app")]})
                assert status == 201, (status, value)

        def record(**row):
            report["measurements"].append(row)
            print(json.dumps({k: v for k, v in row.items() if k != "detail"}), flush=True)

        def batch_load(tag):
            waited = 0.0
            while True:
                reading = {"batch": tag, "windows_cpu_percent": windows_load(), "loadavg": loadavg(),
                           "waited_seconds": waited}
                busy = reading["windows_cpu_percent"] is not None and reading["windows_cpu_percent"] > args.max_load
                if not busy or waited >= args.max_wait:
                    reading["quiet"] = not busy
                    report["host_load"].append(reading)
                    print("load:", json.dumps(reading), flush=True)
                    return
                time.sleep(20)
                waited += 20

        def repo_run(node, size, run, base=False):
            """Create a sandbox from the environment, checkpoint it, archive and verify the tree."""
            n = 1 if base else size
            row = dict(path="repo", image=node.label, size=size, run=run, baseline=base)
            took, status, made = node.create_sandbox(environment=f"bench{n}")
            row.update(seconds=took, status=status)
            if status not in (200, 201):
                row.update(error=f"create answered {status}: {str(made)[:300]}", complete=False)
                record(**row)
                return
            sandbox = made["sandboxID"]
            try:
                if base:
                    row["complete"] = True
                else:
                    ck, st, value = node.checkpoint(sandbox)
                    row["checkpoint_seconds"] = ck
                    if st not in (200, 201, 204):
                        row.update(error=f"checkpoint answered {st}: {str(value)[:300]}", complete=False)
                        record(**row)
                        return
                    spent, st, data, headers = node.archive(sandbox, "/srv/app/data")
                    arow = dict(path="archive", image=node.label, size=size, run=run, baseline=base,
                                seconds=spent, status=st)
                    if st != 200:
                        arow.update(error=f"archive answered {st}: {str(data)[:300]}", complete=False)
                        row.update(error=f"unpack not verified, archive answered {st}", complete=False)
                    else:
                        ok, detail = check_archive(data, specs[n])
                        arow.update(complete=ok, detail=detail, archive_bytes=len(data),
                                    skipped_header=headers.get("x-hv2-archive-skipped"))
                        if not ok:
                            arow["error"] = "archive does not match the generated tree"
                        row["complete"] = ok  # the unpack is complete iff the tree read back is
                        row["detail"] = detail
                    if "archive" in paths:
                        record(**arow)
                    row.setdefault("complete", False)
                record(**row)
            finally:
                node.delete_sandbox(sandbox)

        def copy_run(node, size, run, base=False):
            n = 1 if base else size
            name = f"bench-{node.label}-{n}-{run}-{'b' if base else 'n'}"
            row = dict(path="copy", image=node.label, size=size, run=run, baseline=base)
            try:
                took, status, reason = node.build(name, specs[n])
            except AssertionError as error:
                row.update(error=f"build driving failed: {error}", complete=False)
                record(**row)
                return
            row.update(seconds=took, status=status)
            if status != "ready":
                row.update(error=f"build ended {status}: {json.dumps(reason)[:400]}", complete=False)
                record(**row)
                return
            took_s, st, made = node.create_sandbox(template=name)
            if st not in (200, 201):
                row.update(error=f"sandbox from the template answered {st}: {str(made)[:300]}", complete=False)
                record(**row)
                return
            sandbox = made["sandboxID"]
            try:
                _, st, value = node.checkpoint(sandbox)
                assert st in (200, 201, 204), (st, value)
                _, st, data, _h = node.archive(sandbox, "/opt/tree")
                if st != 200:
                    row.update(error=f"readback archive answered {st}: {str(data)[:300]}", complete=False)
                else:
                    ok, detail = check_archive(data, specs[n])
                    row.update(complete=ok, detail=detail)
                    if not ok:
                        row["error"] = "built tree does not match the generated tree"
            finally:
                node.delete_sandbox(sandbox)
                try:
                    node.request("DELETE", f"/templates/{name}", timeout=60)
                except OSError:
                    pass
            record(**row)

        for run in range(args.runs):
            order = ["agent", "shell"] if run % 2 == 0 else ["shell", "agent"]
            batch_load(f"run{run}-baselines")
            for label in order:
                for p in paths:
                    if p == "repo":
                        repo_run(nodes[label], 1, run, base=True)
                    elif p == "copy":
                        copy_run(nodes[label], 1, run, base=True)
            for si, size in enumerate(sizes):
                swap = (run + si) % 2 == 1
                batch_load(f"run{run}-size{size}")
                for label in (order[::-1] if swap else order):
                    if "repo" in paths or "archive" in paths:
                        repo_run(nodes[label], size, run)
                    if "copy" in paths:
                        copy_run(nodes[label], size, run)
    finally:
        for node in nodes.values():
            node.stop()
        web.shutdown()
        for label in nodes:
            text = (work / f"daemon-{label}.log").read_bytes()
            if len(text) > 2 * LOG_KEEP:
                text = text[:LOG_KEEP] + b"\n... trimmed ...\n" + text[-LOG_KEEP:]
            (args.output / f"daemon-{label}.log").write_bytes(text)
            full = (work / f"daemon-{label}.log").read_text(errors="replace")
            report.setdefault("log_lines", {})[label] = {
                "using agent op": full.count("using agent op"), "falling back to shell": full.count("falling back to shell"),
                "Steps unchanged (cache hit)": full.count("Steps unchanged"),
                "Kvm": full.count("Detected hypervisor platform: Kvm"), "Tcg": full.count("Detected hypervisor platform: Tcg")}
        report["summary"] = summarise(report, specs)
        (args.output / "report.json").write_text(json.dumps(report, indent=2, default=str) + "\n")
        (args.output / "summary.md").write_text(render(report))
        shutil.rmtree(work, ignore_errors=True)
    print((args.output / "summary.md").read_text())


def summarise(report, specs):
    rows = report["measurements"]
    out = {}
    for p in report["paths"]:
        for size in report["sizes"]:
            cell = {}
            for image in ["agent", "shell"]:
                mine = [r for r in rows if r["path"] == p and r["image"] == image and r["size"] == size
                        and not r.get("baseline")]
                ok = [r for r in mine if r.get("complete") and "seconds" in r and "error" not in r]
                cell[image] = {"runs": len(mine), "ok": len(ok), "errors": sorted({r["error"] for r in mine if "error" in r}),
                               "seconds": median_range([r["seconds"] for r in ok]) if ok else None}
                if p in ("repo", "copy"):
                    base = [r["seconds"] for r in rows if r["path"] == p and r["image"] == image and r.get("baseline")
                            and "seconds" in r and "error" not in r]
                    cell[image]["baseline"] = median_range(base) if base else None
                    if ok and base:
                        cell[image]["net_median"] = cell[image]["seconds"]["median"] - statistics.median(base)
                cell[image]["ops_estimate"] = ops_estimate(p, specs[size])
            out[f"{p}/{size}"] = cell
    return out


def fmt(m):
    return f"{m['median']:.2f} [{m['min']:.2f}-{m['max']:.2f}]" if m else "n/a"


def render(report):
    lines = []
    for p in report["paths"]:
        lines += [f"### {p}", "", "| files | agent s median [min-max] | shell s median [min-max] | ratio gross | "
                  "agent net s | shell net s | ratio net | agent ms/op (est.) | agent ok/runs | shell ok/runs |",
                  "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
        for size in report["sizes"]:
            c = report["summary"][f"{p}/{size}"]
            a, s = c["agent"], c["shell"]
            gross = f"{a['seconds']['median'] / s['seconds']['median']:.1f}x" if a["seconds"] and s["seconds"] else "n/a"
            an, sn = a.get("net_median"), s.get("net_median")
            net = f"{an / sn:.1f}x" if an is not None and sn else "n/a"
            small = [c2 for c2 in [report["summary"].get(f"{p}/{report['sizes'][0]}")] if c2]
            per_op = "n/a"
            if p == "archive" and a["seconds"]:
                per_op = f"{1000 * a['seconds']['median'] / a['ops_estimate']:.1f}"
            elif an is not None:
                per_op = f"{1000 * an / a['ops_estimate']:.1f}"
            lines.append(f"| {size} | {fmt(a['seconds'])} | {fmt(s['seconds'])} | {gross} | "
                         f"{'n/a' if an is None else f'{an:.2f}'} | {'n/a' if sn is None else f'{sn:.2f}'} | {net} | "
                         f"{per_op} | {a['ok']}/{a['runs']} | {s['ok']}/{s['runs']} |")
        errs = {(k, img): v[img]["errors"] for k, v in report["summary"].items() if k.startswith(p + "/")
                for img in ("agent", "shell") if v[img]["errors"]}
        for (k, img), e in errs.items():
            lines.append(f"\nErrors, {k} {img}: {e}")
        lines.append("")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
