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
6. `SHELL` changes how a shell-form RUN runs (errexit stops a build the default shell finishes).
7. `RUN` here-documents (`cat <<EOF > file`, and a bare `RUN <<EOT` script) run.
8. `ADD` of a local tar or tar.gz unpacks it into the destination; a zip is copied whole.
9. A failing RUN fails the build at its step with a non-zero exit and no
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

        def shell_instruction():
            # `false; touch /marker` carries on under the default `sh -c` and
            # stops at `false` under `SHELL ["/bin/sh", "-ec"]`.
            line = "RUN false; touch /marker\n"
            for name, head in [("shell-default", ""), ("shell-errexit", 'SHELL ["/bin/sh", "-ec"]\n')]:
                (args.output / name).mkdir()
                (args.output / name / "Dockerfile").write_text("FROM alpine:3.20\n" + head + line)
            built, _ = hm("build", "-t", "shell-default", str(args.output / "shell-default"))
            assert built.returncode == 0, built.stderr[-600:]
            ran, _ = hm("run", "--template", "shell-default", "sh", "-c", "test -f /marker && echo marker")
            assert ran.stdout.strip() == "marker", ran
            stopped, _ = hm("build", "-t", "shell-errexit", str(args.output / "shell-errexit"))
            assert stopped.returncode != 0 and "build failed at step" in stopped.stderr, stopped
            return {"stopped": [l for l in stopped.stderr.splitlines() if "build failed" in l]}
        case("SHELL changes how a shell-form RUN runs (errexit stops a build the default shell finishes)", shell_instruction)

        def heredocs():
            doc = args.output / "heredoc"
            doc.mkdir()
            (doc / "Dockerfile").write_text(
                "FROM alpine:3.20\n"
                "ARG WHO=world\n"
                "RUN cat <<EOF > /note.txt\nhello $WHO\n  indented\nEOF\n"
                "RUN <<EOT\nset -e\necho from-script > /script.txt\nEOT\n"
                "ENV AFTER=yes\n")
            built, _ = hm("build", "-t", "heredoc", str(doc))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "heredoc", "sh", "-c", "cat /note.txt /script.txt; echo after=$AFTER")
            want = "hello world\n  indented\nfrom-script\nafter=yes\n"
            assert ran.stdout == want, (ran.stdout, ran.stderr[-300:])
            return {"stdout": ran.stdout}
        case("RUN here-documents: `RUN cat <<EOF > file` keeps its body and `RUN <<EOT` runs as a script", heredocs)

        def add_archive():
            import io, tarfile
            ctx = args.output / "addtar"
            ctx.mkdir()
            for name, mode in [("data.tar.gz", "w:gz"), ("plain.tar", "w")]:
                with tarfile.open(ctx / name, mode) as tar:
                    for member, body in [("inner/a.txt", b"alpha\n"), ("b.txt", b"beta\n")]:
                        info = tarfile.TarInfo(member)
                        info.size = len(body)
                        tar.addfile(info, io.BytesIO(body))
            (ctx / "site.zip").write_bytes(b"not really a zip\n")
            (ctx / "Dockerfile").write_text(
                "FROM alpine:3.20\n"
                "ADD data.tar.gz /srv/data\n"
                "ADD plain.tar /srv/plain/\n"
                "ADD site.zip /srv/site.zip\n")
            built, _ = hm("build", "-t", "addtar", str(ctx))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "addtar", "sh", "-c",
                        "cat /srv/data/inner/a.txt /srv/data/b.txt /srv/plain/inner/a.txt; "
                        "cat /srv/site.zip; ls /tmp/.hm-add-* 2>&1 | head -1")
            want = "alpha\nbeta\nalpha\nnot really a zip\n"
            assert ran.stdout.startswith(want) and "No such file" in ran.stdout + ran.stderr, (ran.stdout, ran.stderr[-300:])
            return {"stdout": ran.stdout}
        case("ADD unpacks a local tar and tar.gz into the destination, copies a zip whole, and leaves no temp file", add_archive)

        def add_bz2_xz():
            import io, tarfile
            ctx = args.output / "addcomp"
            ctx.mkdir()
            for name, mode in [("b.tar.bz2", "w:bz2"), ("x.tar.xz", "w:xz")]:
                with tarfile.open(ctx / name, mode) as tar:
                    body = (name + " body\n").encode()
                    info = tarfile.TarInfo("f.txt")
                    info.size = len(body)
                    tar.addfile(info, io.BytesIO(body))
            (ctx / "Dockerfile").write_text(
                "FROM alpine:3.20\nADD b.tar.bz2 /srv/b\nADD x.tar.xz /srv/x\n")
            built, _ = hm("build", "-t", "addcomp", str(ctx))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "addcomp", "sh", "-c", "cat /srv/b/f.txt /srv/x/f.txt")
            assert ran.stdout == "b.tar.bz2 body\nx.tar.xz body\n", (ran.stdout, ran.stderr[-300:])
            return {"stdout": ran.stdout}
        case("ADD unpacks a tar.bz2 and a tar.xz", add_bz2_xz)

        def copy_heredoc():
            doc = args.output / "copyheredoc"
            doc.mkdir()
            (doc / "Dockerfile").write_text(
                "FROM alpine:3.20\n"
                "ARG WHO=world\n"
                "COPY --chmod=755 <<EOF /usr/local/bin/greet\n#!/bin/sh\necho hello $WHO\nEOF\n"
                "COPY <<'RAW' <<-TWO /etc/conf/\nraw $WHO 'q'\nRAW\n\tindented\n\tTWO\n"
                "ENV AFTER=yes\n")
            built, _ = hm("build", "-t", "copyheredoc", str(doc))
            assert built.returncode == 0, built.stderr[-800:]
            ran, _ = hm("run", "--template", "copyheredoc", "sh", "-c",
                        "greet; stat -c %a /usr/local/bin/greet; cat /etc/conf/RAW /etc/conf/TWO; echo after=$AFTER")
            want = "hello world\n755\nraw $WHO 'q'\nindented\nafter=yes\n"
            assert ran.stdout == want, (ran.stdout, ran.stderr[-300:])
            return {"stdout": ran.stdout}
        case("COPY <<EOF writes each here-document as a file (expanded or raw, --chmod, several into a directory)", copy_heredoc)

        def run_mounts():
            ctx = args.output / "mounts"
            (ctx / "src").mkdir(parents=True)
            (ctx / "src" / "input.txt").write_text("from the context\n")
            (ctx / "Dockerfile").write_text(
                "FROM alpine:3.20\n"
                "RUN --mount=type=bind,source=src,target=/mnt/in cat /mnt/in/input.txt > /seen.txt\n"
                "RUN --mount=type=tmpfs,target=/scratch echo temp > /scratch/f && cp /scratch/f /seen-tmp.txt\n"
                "RUN --mount=type=cache,target=/cache echo c > /cache/x && cp /cache/x /seen-cache.txt\n")
            built, _ = hm("build", "-t", "mounts", str(ctx))
            assert built.returncode == 0, built.stderr[-800:]
            assert "type=cache" in built.stderr, built.stderr[-400:]
            ran, _ = hm("run", "--template", "mounts", "sh", "-c",
                        "cat /seen.txt /seen-tmp.txt /seen-cache.txt; ls -d /mnt/in /scratch /cache 2>&1")
            assert ran.stdout.startswith("from the context\ntemp\nc\n"), (ran.stdout, ran.stderr[-300:])
            assert ran.stdout.count("No such file") == 3, ran.stdout
            (ctx / "Dockerfile").write_text(
                "FROM alpine:3.20\nRUN --mount=type=tmpfs,target=/scratch sh -c 'exit 5'\n")
            failed, _ = hm("build", "-t", "mounts-fail", str(ctx))
            assert failed.returncode != 0 and "build failed at step" in failed.stderr, failed
            (ctx / "Dockerfile").write_text(
                "FROM alpine:3.20\nRUN --mount=type=secret,id=k true\n")
            refused, _ = hm("build", "-t", "mounts-secret", str(ctx))
            assert refused.returncode != 0 and "type=secret" in refused.stderr, refused
            return {"stdout": ran.stdout}
        case("RUN --mount: bind from the context, tmpfs and cache last for one RUN; a failing RUN still fails; secret is refused", run_mounts)

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
