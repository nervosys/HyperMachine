#!/usr/bin/env python3
"""`hm sandbox vm build` of a multi-stage Dockerfile, on real KVM.

Runs an owned hv2-sandboxd with a guest kit (no Docker anywhere), writes a build
context, and drives the real `hm` binary:

1. A two-stage Dockerfile builds. `COPY --from=NAME` takes a directory and a
   glob out of the first stage, `COPY --from=0` takes a file by number; the
   template runs the copied script (its mode kept), sees the copied files, and
   does not have the first stage's working directory.
2. Building it again is quicker: the files read out of a stage make the same
   bytes, so the node's step cache is hit.
3. A stage that was only read from is not left behind as a template.
4. A chain of three stages, each copying from the one before.
5. COPY --from a path the stage does not have fails the build, naming the path,
   and builds no template; FROM an earlier stage is refused.

Writes report.json and the daemon log into --output. Times are the check's own.
"""
import argparse, hashlib, json, os, signal, socket, subprocess, time
import urllib.request
from pathlib import Path

DOCKERFILE = """\
FROM alpine:3.20 AS build
WORKDIR /src
RUN mkdir -p /src/out/conf
COPY tool.sh /src/out/tool.sh
RUN chmod +x out/tool.sh && echo built-in-stage > out/artifact.txt && echo cfg > out/conf/app.conf \\
    && echo only-in-build > /src/build-only.txt && sleep 4

FROM alpine:3.20
COPY --from=build /src/out/ /opt/app/
COPY --from=build /src/out/*.txt /opt/txt/
COPY --from=0 /src/build-only.txt /opt/from-zero.txt
RUN sleep 4 && echo final > /opt/final.txt
ENTRYPOINT ["/opt/app/tool.sh"]
"""

TOOL = """#!/bin/sh
echo "tool says $(cat /opt/app/artifact.txt) conf=$(cat /opt/app/conf/app.conf) zero=$(cat /opt/from-zero.txt) glob=$(ls /opt/txt | tr '\\n' ,) final=$(cat /opt/final.txt) stage=$(test -e /src && echo leaked || echo clean)"
"""

CHAIN = """\
FROM alpine:3.20 AS a
RUN echo from-a > /a.txt
FROM alpine:3.20 AS b
COPY --from=a /a.txt /b.txt
RUN echo and-b >> /b.txt
FROM alpine:3.20
COPY --from=b /b.txt /c.txt
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
    report = {"success": False, "purpose": "hm sandbox vm build of a multi-stage Dockerfile on KVM; functional, times are the check's stopwatch",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    ctx = args.output / "context"
    ctx.mkdir(parents=True)
    (ctx / "Dockerfile").write_text(DOCKERFILE)
    (ctx / "tool.sh").write_text(TOOL)

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

    def dockerfile_dir(name, text):
        directory = args.output / name
        directory.mkdir()
        (directory / "Dockerfile").write_text(text)
        return directory

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
            built, first = hm("build", "-t", "ms-app", str(ctx))
            assert built.returncode == 0 and built.stdout.strip() == "ms-app", (built.returncode, built.stdout, built.stderr[-1200:])
            ran, _ = hm("run", "--template", "ms-app")
            want = "tool says built-in-stage conf=cfg zero=only-in-build glob=artifact.txt, final=final stage=clean\n"
            assert ran.returncode == 0 and ran.stdout == want, (ran.stdout, ran.stderr[-600:])
            state["first"] = first
            return {"build_seconds": first, "stdout": ran.stdout}
        state = {}
        case("a two-stage Dockerfile builds: COPY --from by name (a directory and a glob) and by number, mode kept, stage's workdir absent", builds_and_runs)

        def cached():
            built, second = hm("build", "-t", "ms-app-again", str(ctx))
            assert built.returncode == 0, built.stderr[-800:]
            assert second + 4 < state["first"], (state["first"], second)
            return {"first_seconds": state["first"], "second_seconds": second}
        case("building it again is quicker, so the files read out of a stage are the same bytes", cached)

        def stages_are_not_kept():
            gone, _ = hm("run", "--template", "ms-app-stage0")
            assert gone.returncode != 0, gone
            return {"run_of_the_stage_template": gone.stderr.strip()[:200]}
        case("the stage that was only read from is not left behind as a template", stages_are_not_kept)

        def chain():
            built, _ = hm("build", "-t", "ms-chain", str(dockerfile_dir("chain", CHAIN)))
            assert built.returncode == 0, built.stderr[-1000:]
            ran, _ = hm("run", "--template", "ms-chain", "cat", "/c.txt")
            assert ran.returncode == 0 and ran.stdout == "from-a\nand-b\n", (ran.stdout, ran.stderr[-500:])
            return {"stdout": ran.stdout}
        case("three stages, each copying from the one before", chain)

        def failures():
            missing = dockerfile_dir("missing", "FROM alpine:3.20 AS build\nRUN true\nFROM alpine:3.20\nCOPY --from=build /nope /nope\n")
            failed, _ = hm("build", "-t", "ms-missing", str(missing))
            assert failed.returncode != 0 and "nothing at /nope" in failed.stderr, failed
            as_base = dockerfile_dir("asbase", "FROM alpine:3.20 AS build\nRUN true\nFROM build\nRUN true\n")
            refused, _ = hm("build", "-t", "ms-asbase", str(as_base))
            assert refused.returncode != 0 and "earlier stage" in refused.stderr, refused
            return {"missing": failed.stderr.strip()[-200:], "as_base": refused.stderr.strip()[:200]}
        case("COPY --from a path the stage lacks fails the build, and FROM an earlier stage is refused", failures)

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
