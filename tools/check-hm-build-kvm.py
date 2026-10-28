#!/usr/bin/env python3
"""`hm sandbox vm build`, a Dockerfile built into a template, on real KVM.

Runs an owned hv2-sandboxd with a guest kit (no Docker anywhere), writes a
build context, and drives the real `hm` binary:

1. A Dockerfile with ARG, ENV, WORKDIR, COPY (a file and a directory),
   RUN, USER, ENTRYPOINT and CMD builds; `run --template` then runs it: the
   ENTRYPOINT script sees the CMD as its arguments, ENV, the working
   directory, the user from USER, and the ARG's default written by a RUN.
2. A command after the template replaces CMD and keeps the ENTRYPOINT.
3. `--build-arg` changes what the build wrote.
4. `.dockerignore` keeps ignored files out of the guest, and COPY . copies the
   rest.
5. Building the same thing again is quicker (the node's step cache).
6. A failing RUN fails the build at its step with a non-zero exit and no
   template; COPY --from an image (not a stage) is refused before anything is sent.
   Multi-stage builds are in check-hm-multistage-kvm.py.

Writes report.json and the daemon log into --output. Times are the check's own.
"""
import argparse, hashlib, json, os, signal, socket, subprocess, time
import urllib.request
from pathlib import Path

DOCKERFILE = """\
FROM alpine:3.20
ARG GREETING=hello
ENV APP_ENV=prod
WORKDIR /srv/app
COPY app.sh /srv/app/app.sh
COPY data/ /srv/app/data/
RUN chmod +x app.sh && echo "$GREETING" > greeting.txt
RUN adduser -D -u 4242 runner && chown -R runner /srv/app
USER runner
ENTRYPOINT ["/srv/app/app.sh"]
CMD ["default-arg"]
"""

APP = """#!/bin/sh
echo "args=$* env=$APP_ENV user=$(id -un) cwd=$(pwd) greeting=$(cat greeting.txt) data=$(ls data | tr '\\n' ,)"
"""


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
    report = {"success": False, "purpose": "hm sandbox vm build on KVM; functional, times are the check's stopwatch",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    ctx = args.output / "context"
    (ctx / "data").mkdir(parents=True)
    (ctx / "Dockerfile").write_text(DOCKERFILE)
    (ctx / "app.sh").write_text(APP)
    (ctx / "data" / "one.txt").write_text("1\n")
    (ctx / "data" / "skip.txt").write_text("ignored\n")
    (ctx / "debug.log").write_text("noise\n")
    (ctx / ".dockerignore").write_text("*.log\ndata/skip.txt\nDockerfile\n")

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

    def hm(*argv, timeout=1500):
        began = time.monotonic()
        done = subprocess.run([str(args.hm), "sandbox", "vm", "--endpoint", url, "--request-timeout", "300", *argv],
                              capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL,
                              env={k: v for k, v in os.environ.items() if k != "HV2_API_KEY"} | {"HOME": str(args.output)})
        return done, round(time.monotonic() - began, 2)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        row["detail"] = action()
        row["success"] = True
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

        def builds_and_runs():
            built, t1 = hm("build", "-t", "demo-app", str(ctx))
            assert built.returncode == 0 and built.stdout.strip() == "demo-app", (built.returncode, built.stdout, built.stderr[-800:])
            ran, _ = hm("run", "--template", "demo-app")
            want = "args=default-arg env=prod user=runner cwd=/srv/app greeting=hello data=one.txt,\n"
            assert ran.returncode == 0 and ran.stdout == want, (ran.stdout, ran.stderr[-500:])
            return {"build_seconds": t1, "stdout": ran.stdout}
        case("a Dockerfile builds, and the template runs its ENTRYPOINT with CMD, ENV, WORKDIR, USER and COPY", builds_and_runs)

        def command_replaces_cmd():
            ran, _ = hm("run", "--template", "demo-app", "other", "args")
            assert ran.stdout.startswith("args=other args env=prod user=runner"), ran
            return {"stdout": ran.stdout}
        case("a command replaces CMD and the ENTRYPOINT stays", command_replaces_cmd)

        def build_arg():
            built, _ = hm("build", "-t", "demo-howdy", "--build-arg", "GREETING=howdy", str(ctx))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "demo-howdy")
            assert "greeting=howdy" in ran.stdout, ran
            return {"stdout": ran.stdout}
        case("--build-arg changes what the build wrote", build_arg)

        def dockerignore():
            plain = args.output / "plain"
            plain.mkdir()
            (plain / "Dockerfile").write_text("FROM alpine:3.20\nCOPY . /ctx/\n")
            (plain / "keep.txt").write_text("kept\n")
            (plain / "debug.log").write_text("noise\n")
            (plain / "sub").mkdir()
            (plain / "sub" / "secret.key").write_text("s\n")
            (plain / ".dockerignore").write_text("*.log\nsub\n")
            built, _ = hm("build", "-t", "plain-ctx", str(plain))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "plain-ctx", "sh", "-c", "cd /ctx && find . -type f | sort")
            files = ran.stdout.split()
            assert "./keep.txt" in files and not any(f.endswith(".log") or "secret" in f for f in files), files
            return {"files": files}
        case(".dockerignore keeps ignored files out and COPY . copies the rest", dockerignore)

        def cached():
            # A context no earlier case built, with a step that takes a while,
            # so the first build cannot be a cache hit.
            slow = args.output / "slow"
            slow.mkdir()
            (slow / "Dockerfile").write_text("FROM alpine:3.20\nRUN sleep 4 && echo slow > /slow.txt\n")
            built, first = hm("build", "-t", "slow-one", str(slow))
            assert built.returncode == 0, built.stderr[-800:]
            built, second = hm("build", "-t", "slow-two", str(slow))
            assert built.returncode == 0, built.stderr[-800:]
            assert second + 3 < first, (first, second)
            return {"first_seconds": first, "second_seconds": second}
        case("building the same thing again is quicker", cached)

        def failures():
            bad = args.output / "bad"
            bad.mkdir()
            (bad / "Dockerfile").write_text("FROM alpine:3.20\nRUN echo ok\nRUN echo boom >&2; exit 3\n")
            failed, _ = hm("build", "-t", "will-fail", str(bad))
            assert failed.returncode != 0 and "build failed at step" in failed.stderr and "boom" in failed.stderr, failed
            assert "will-fail" not in [t["templateID"] for t in get("/templates") if t.get("buildStatus") == "ready"]
            multi = args.output / "multi"
            multi.mkdir()
            (multi / "Dockerfile").write_text("FROM alpine:3.20\nCOPY --from=nginx:latest /x /x\n")
            refused, _ = hm("build", "-t", "never", str(multi))
            assert refused.returncode != 0 and "not an earlier stage" in refused.stderr, refused
            return {"failed_step": [l for l in failed.stderr.splitlines() if "build failed" in l],
                    "refused": refused.stderr.strip()[:160]}
        case("a failing RUN fails the build at its step, and COPY --from an image is refused", failures)

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
