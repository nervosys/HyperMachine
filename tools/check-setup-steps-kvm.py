#!/usr/bin/env python3
"""Setup steps of a template (its RUN steps, in E2B's Template.build) on real KVM.

Runs an owned hv2-sandboxd (`--guest-kit` set to an empty directory) whose base template is
the image under test (it needs /bin/sh: use the busybox image), and drives the template-build
API (POST /v3/templates, the COPY upload, POST /v2/templates/{id}/builds/{buildID}, GET
.../status). Each build is `fromTemplate: base`. Cases:

a. A template built with setup steps runs them at build: a marker written by a RUN step is in
   a sandbox created later, and its timestamp (the guest's `date +%s`, written at build) lies
   inside the host's window of the build and well before the sandbox was created.
b. A second sandbox from the same template does not run them again: the file the step
   appended a line to still holds one line in both sandboxes, and the marker is unchanged.
c. Steps run in order, after the files, variables and working directory set before them: a RUN
   after COPY reads the copied file, after ENV sees the variable, after WORKDIR runs there,
   and two RUN steps leave their lines in step order.
d. A failing setup step fails the build, says which step and its exit status, a later step
   is not run, and no usable template is left: creating a sandbox from it is refused and the
   template is not listed.

Writes report.json and the daemon's log into --output, with the number of `Kvm` and `Tcg`
platform lines (Tcg must be 0).
"""
import argparse, hashlib, io, json, os, signal, socket, subprocess, tarfile, tempfile, time
import urllib.error, urllib.request
from pathlib import Path


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of a template's setup (RUN) steps on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-setup-steps-check-", dir="/var/tmp"))
    kit = work / "kit"
    kit.mkdir()
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(kit),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "6"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd),
                 RUST_LOG="info,hv2_sandboxd::builds=debug"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300, url=None, data=None):
        payload = data if data is not None else (None if body is None else json.dumps(body).encode())
        req = urllib.request.Request(url or base + path, method=method,
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

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    builds = {}

    def build(name, steps, uploads=None):
        """Run a build to its end: (status, reason, host start, host end)."""
        status, made = request("POST", "/v3/templates", {"name": name})
        assert status == 202, (status, made)
        hashes = {}
        for label, payload in (uploads or {}).items():
            h = hashlib.sha256(payload).hexdigest()
            hashes[label] = h
            status, link = request("GET", f"/templates/{name}/files/{h}")
            assert status == 201, (status, link)
            if not link["present"]:
                with urllib.request.urlopen(urllib.request.Request(link["url"], method="PUT", data=payload),
                                            timeout=60) as r:
                    assert r.status == 200, r.status
        wired = []
        for step in steps:
            step = dict(step)
            if "files" in step:
                step["filesHash"] = hashes[step.pop("files")]
            wired.append(step)
        started = time.time()
        status, value = request("POST", f"/v2/templates/{name}/builds/{made['buildID']}",
                                {"fromTemplate": "base", "steps": wired})
        assert status == 202, (status, value)
        deadline = time.monotonic() + 600
        while True:
            status, value = request("GET", f"/templates/{name}/builds/{made['buildID']}/status")
            assert status == 200, (status, value)
            if value["status"] in ("ready", "error"):
                builds[name] = value
                return value["status"], value.get("reason") or {}, started, time.time()
            assert time.monotonic() < deadline, ("build", name, value["status"])
            time.sleep(0.5)

    def sandbox_of(template):
        status, made = request("POST", "/sandboxes", {"templateID": template, "timeout": 900})
        assert status in (200, 201), (status, made)
        return made["sandboxID"], time.time()

    def sh(sandbox, command):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 30})
        assert status == 200 and value["exit_code"] == 0, (status, value)
        return value["stdout"]

    state = {}
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

        seed = tar_of("seed.txt", b"seeded\n")
        script_one = ("date +%s > /built-at; echo run >> /runs; "
                      "cat /proc/sys/kernel/random/boot_id > /build-boot; "
                      "cat /opt/seed.txt > /order; echo \"env=$GREET\" >> /order; "
                      "echo \"cwd=$(pwd)\" >> /order; echo step-one >> /order")
        steps = [
            {"type": "COPY", "args": ["seed.txt", "/opt/seed.txt"], "files": "seed"},
            {"type": "ENV", "args": ["GREET", "hello-env"]},
            {"type": "WORKDIR", "args": ["/srv/setup"]},
            {"type": "RUN", "args": [script_one]},
            {"type": "RUN", "args": ["echo step-two >> /order"]},
        ]
        status, reason, t0, t1 = build("ss-ok", steps, {"seed": seed})
        assert status == "ready", (status, reason)

        def runs_at_build():
            time.sleep(4)
            sandbox, created = sandbox_of("ss-ok")
            state["a"] = sandbox
            marker = int(sh(sandbox, "cat /built-at").strip())
            now = int(sh(sandbox, "date +%s").strip())
            assert t0 - 5 <= marker <= t1 + 5, ("marker outside the build window", marker, t0, t1)
            assert marker <= created - 3, ("marker not before creation", marker, created)
            assert now >= marker + 3, ("sandbox clock", now, marker)
            state["marker"] = marker
            same_boot = sh(sandbox, "cat /build-boot").strip() == sh(
                sandbox, "cat /proc/sys/kernel/random/boot_id").strip()
            return {"marker": marker, "build_window": [round(t0), round(t1)], "created_at": round(created),
                    "guest_now": now, "build_boot_id_equals_sandbox_boot_id": same_boot}
        case("a. a RUN step ran at build: its marker holds the build time, not the sandbox's", runs_at_build)

        def not_again():
            time.sleep(2)
            second, _ = sandbox_of("ss-ok")
            assert sh(state["a"], "cat /runs") == "run\n", sh(state["a"], "cat /runs")
            assert sh(second, "cat /runs") == "run\n", sh(second, "cat /runs")
            assert int(sh(second, "cat /built-at").strip()) == state["marker"]
            request("DELETE", f"/sandboxes/{second}")
            return {"runs_file_first": "run", "runs_file_second": "run", "marker_same": True}
        case("b. a second sandbox from the template does not run the steps again", not_again)

        def in_order():
            order = sh(state["a"], "cat /order").splitlines()
            assert order == ["seeded", "env=hello-env", "cwd=/srv/setup", "step-one", "step-two"], order
            return {"order": order}
        case("c. steps run in order, after the COPY, ENV and WORKDIR before them", in_order)

        def failing():
            steps = [{"type": "RUN", "args": ["echo a > /a"]},
                     {"type": "RUN", "args": ["echo before-fail > /b; exit 7"]},
                     {"type": "RUN", "args": ["echo after-fail > /c"]}]
            status, reason, _, _ = build("ss-fail", steps)
            message = reason.get("message", "")
            assert status == "error", (status, reason)
            assert reason.get("step") == "2" and "7" in message, reason
            status, made = request("POST", "/sandboxes", {"templateID": "ss-fail", "timeout": 60})
            assert status >= 400, (status, made)
            _, listed = request("GET", "/templates")
            assert "ss-fail" not in json.dumps(listed), listed
            return {"step": reason["step"], "message": message, "create_status": status,
                    "create_response": made, "listed_in_templates": False}
        case("d. a failing setup step fails the build, clearly, and leaves no usable template", failing)

        log.flush()
        lines = log_path.read_text(errors="replace").splitlines()
        counts = {"kvm_lines": sum("platform: Kvm" in l for l in lines),
                  "tcg_lines": sum("platform: Tcg" in l for l in lines)}
        report["log_counts"] = counts
        assert counts["tcg_lines"] == 0 and counts["kvm_lines"] >= 1, counts
        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        report["builds"] = {n: {"status": v["status"], "reason": v.get("reason"), "logs": [
            e["message"] for e in v.get("logEntries", [])]} for n, v in builds.items()}
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
