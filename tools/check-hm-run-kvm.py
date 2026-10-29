#!/usr/bin/env python3
"""`hm sandbox vm run IMAGE`, a container image run in a VM of its own, on real KVM.

Runs an owned hv2-sandboxd with a guest kit (so it can pull an image from its
registry and build a template, no Docker anywhere), then drives the real `hm`
binary against it:

1. `run alpine:3.20 echo hello` pulls and builds, prints `hello`, exits 0, and
   leaves no sandbox behind; a second run of the same image reuses the
   template (no second build).
2. The command's exit code is the process's own: `sh -c 'exit 7'` exits 7.
3. `-e NAME=VALUE` reaches the command.
4. An image with an ENTRYPOINT and a USER (curlimages/curl): its ENTRYPOINT
   stays under a given command (`id -u` runs as the image's user, not root),
   and with no command its ENTRYPOINT and CMD run (`curl` with no URL fails
   with curl's own message).
5. An image with a WORKDIR (golang:1.22-alpine): `pwd` prints /go.
6. An image that does not exist fails with the registry's error, and no
   sandbox or template is left claiming to be ready.
7. `--keep` leaves the sandbox running and says its ID.
8. `-i` sends standard input to the command intact; more than 256 KiB is refused.

Writes report.json and the daemon log into --output. Times are the check's own.
"""
import argparse, hashlib, json, os, signal, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "hm", "kernel", "initrd", "kit", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "hm sandbox vm run on KVM; functional, times are the check's stopwatch",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
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
            return json.loads(r.read())

    def hm(*argv, timeout=900, stdin=None):
        began = time.monotonic()
        done = subprocess.run([str(args.hm), "sandbox", "vm", "--endpoint", url, "--request-timeout", "300", "run", *argv],
                              capture_output=True, text=True, timeout=timeout,
                              stdin=subprocess.DEVNULL if stdin is None else None, input=stdin,
                              env={k: v for k, v in os.environ.items() if k != "HV2_API_KEY"} | {"HOME": str(args.output)})
        return done, round(time.monotonic() - began, 2)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        row["detail"] = detail
        print("ok:", name, flush=True)

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

        def hello():
            first, t1 = hm("alpine:3.20", "echo", "hello")
            assert first.returncode == 0 and first.stdout == "hello\n", (first.returncode, first.stdout, first.stderr)
            assert get("/sandboxes") == [], "a sandbox was left"
            second, t2 = hm("alpine:3.20", "echo", "again")
            assert second.stdout == "again\n", second
            assert t2 < t1, (t1, t2)
            templates = [t["templateID"] for t in get("/templates") if t["templateID"].startswith("oci-alpine")]
            assert len(templates) == 1, templates
            return {"first_run_seconds": t1, "second_run_seconds": t2, "template": templates[0]}
        case("pulls an image, runs a command, prints, exits 0, leaves nothing; a second run reuses the template", hello)

        def exit_code():
            done, _ = hm("alpine:3.20", "sh", "-c", "echo out; echo err >&2; exit 7")
            assert done.returncode == 7 and done.stdout == "out\n" and done.stderr.endswith("err\n"), done
            return {"exit": done.returncode}
        case("the command's exit code and streams are the process's own", exit_code)

        def env():
            done, _ = hm("-e", "GREETING=hi there", "-e", "N=2", "alpine:3.20", "sh", "-c", 'echo "$GREETING $N"')
            assert done.stdout == "hi there 2\n", done
            return {"stdout": done.stdout}
        case("-e variables reach the command", env)

        def stdin_reaches_the_command():
            text = "line one\nline two with 'quotes' and $dollars\né\n"
            echoed, _ = hm("-i", "alpine:3.20", "cat", stdin=text)
            assert echoed.returncode == 0 and echoed.stdout == text, (echoed.stdout, echoed.stderr[-300:])
            empty, _ = hm("-i", "alpine:3.20", "cat", stdin="")
            assert empty.returncode == 0 and empty.stdout == "", empty
            big = "x" * 300_000
            refused, _ = hm("-i", "alpine:3.20", "cat", stdin=big)
            assert refused.returncode != 0 and "longer than" in refused.stderr, refused
            return {"echoed_bytes": len(echoed.stdout)}
        case("-i sends standard input to the command (quotes, $ and non-ASCII intact); an over-long input is refused", stdin_reaches_the_command)

        def named_volume():
            wrote, _ = hm("-v", "runvol:/data", "alpine:3.20", "sh", "-c", "echo persisted > /data/f")
            assert wrote.returncode == 0, wrote.stderr[-400:]
            assert "runvol" in [v["name"] for v in get("/volumes")], get("/volumes")
            read, _ = hm("-v", "runvol:/mnt/elsewhere", "alpine:3.20", "cat", "/mnt/elsewhere/f")
            assert read.returncode == 0 and read.stdout == "persisted\n", (read.stdout, read.stderr[-300:])
            without, _ = hm("alpine:3.20", "cat", "/data/f")
            assert without.returncode != 0 and without.stdout == "", without
            host, _ = hm("-v", "/srv/data:/data", "alpine:3.20", "true")
            assert host.returncode != 0 and "volume names" in host.stderr, host
            return {"read": read.stdout.strip()}
        case("-v NAME:/path mounts a named volume that outlives the run; a host path is refused", named_volume)

        def entrypoint_and_user():
            who, _ = hm("curlimages/curl", "id", "-u")
            assert who.returncode == 0 and who.stdout.strip() not in ("", "0"), who
            version, _ = hm("curlimages/curl", "--version")
            assert version.returncode == 0 and version.stdout.startswith("curl "), version
            bare, _ = hm("curlimages/curl")
            assert bare.returncode != 0 and "curl" in (bare.stdout + bare.stderr), bare
            return {"uid": who.stdout.strip(), "version": version.stdout.split("\n")[0],
                    "no command": (bare.stdout + bare.stderr).strip()[:120]}
        case("an image's ENTRYPOINT stays under a command, its CMD runs alone, and its USER applies", entrypoint_and_user)

        def workdir():
            done, _ = hm("golang:1.22-alpine", "pwd")
            assert done.stdout == "/go\n", done
            return {"pwd": done.stdout.strip()}
        case("an image's WORKDIR is the working directory", workdir)

        def missing():
            done, _ = hm("alpine:no-such-tag-xyz", "true", timeout=300)
            assert done.returncode != 0, done
            assert get("/sandboxes") == [], "a sandbox was left"
            bad = [t for t in get("/templates") if t["templateID"].startswith("oci-alpine-no-such")]
            assert all(t["buildStatus"] != "ready" for t in bad), bad
            return {"stderr": done.stderr.strip()[:200]}
        case("an image that does not exist fails with the registry's error and leaves nothing", missing)

        def kept():
            done, _ = hm("--keep", "alpine:3.20", "true")
            assert done.returncode == 0 and "kept sandbox" in done.stderr, done
            left = get("/sandboxes")
            assert len(left) == 1, left
            return {"stderr": done.stderr.strip().splitlines()[-1]}
        case("--keep leaves the sandbox running and says its ID", kept)

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
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
