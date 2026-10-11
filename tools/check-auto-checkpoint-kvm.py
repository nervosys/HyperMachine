#!/usr/bin/env python3
"""Automatic checkpoints on real KVM: a sandbox saved on an interval, unasked.

Runs an owned hv2-sandboxd with a template and checks, through its API:

1. A sandbox created with `autoCheckpoint` has no checkpoint at first, and one
   named `auto` an interval after it starts, without a request for it.
2. Rolling back to `auto` brings back the file as it was and the process that
   was running: its counter carries on from where the checkpoint caught it,
   not from nothing. Memory is in the checkpoint, not only files.
3. After the sandbox is used again, `auto` is replaced by a newer one, and
   there is still exactly one.
4. Left alone, with nothing running in it, the sandbox is not checkpointed
   again: there is nothing new to save.
5. A sandbox created without `autoCheckpoint` is never checkpointed.
6. What cannot be done is refused when asked: an interval shorter than the
   least, and a caller saving under the name `auto`.

The interval is the least the node accepts, so this takes several minutes.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

EVERY = 60


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of automatic checkpoints on KVM; "
                                           "no timings and no comparison with another product",
              "interval_seconds": EVERY,
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-auto-checkpoint-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info",
               TMPDIR=str(work))
    api, proxy = port(), port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy), "--memory-mb", "512",
         "--cpu-cores", "1", "--capacity", "4"],
        env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=120):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
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

    def execute(sandbox, command):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 20})
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

    def checkpoints(sandbox):
        status, value = request("GET", f"/sandboxes/{sandbox}/checkpoints")
        assert status == 200, (status, value)
        return {entry["name"]: entry["createdAt"] for entry in value}

    def wait_for_auto(sandbox, newer_than, within):
        """The time of an `auto` newer than `newer_than`, waited for up to `within` seconds."""
        deadline = time.monotonic() + within
        while time.monotonic() < deadline:
            assert daemon.poll() is None, "the daemon exited"
            created = checkpoints(sandbox).get("auto")
            if created is not None and created > newer_than:
                return created
            time.sleep(1)
        raise AssertionError(f"no automatic checkpoint newer than {newer_than} within {within}s: "
                             f"{checkpoints(sandbox)}")

    try:
        deadline = time.monotonic() + 300
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                status, templates = request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

        status, plain = request("POST", "/sandboxes", {"timeout": 1800})
        assert status == 201, (status, plain)
        plain = plain["sandboxID"]
        status, created = request("POST", "/sandboxes", {"timeout": 1800, "autoCheckpoint": EVERY})
        assert status == 201, (status, created)
        sandbox = created["sandboxID"]
        started = time.time()
        state = {}

        def first():
            assert checkpoints(sandbox) == {}, checkpoints(sandbox)
            execute(sandbox, "echo one > /tmp/state")
            execute(sandbox, "(i=0; while true; do i=$((i+1)); echo $i > /tmp/count; sleep 0.1; done) "
                             ">/dev/null 2>&1 &")
            state["first"] = wait_for_auto(sandbox, 0, EVERY + 30)
            waited = time.time() - started
            # An interval after it started, not at once.
            assert waited >= EVERY - 2, f"checkpointed after only {waited:.1f}s"
            assert list(checkpoints(sandbox)) == ["auto"], checkpoints(sandbox)
            return {"seconds_after_start": round(waited, 1)}
        case("a sandbox with an interval is checkpointed an interval after it starts, unasked", first)

        def rolls_back():
            time.sleep(3)
            later = int(execute(sandbox, "cat /tmp/count"))
            execute(sandbox, "echo two > /tmp/state")
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints/auto/restore")
            assert status == 200, (status, value)
            assert execute(sandbox, "cat /tmp/state") == "one"
            caught = int(execute(sandbox, "cat /tmp/count"))
            # The counter is the one that was running: it is past zero, and
            # behind where it had got to before the roll back.
            assert 0 < caught < later, (caught, later)
            time.sleep(2)
            moving = int(execute(sandbox, "cat /tmp/count"))
            assert moving > caught, (caught, moving)
            return {"count_before_restore": later, "count_after_restore": caught, "count_two_seconds_on": moving}
        case("rolling back to it restores the file and the running process with its memory", rolls_back)

        def replaced():
            state["second"] = wait_for_auto(sandbox, state["first"], EVERY + 30)
            assert list(checkpoints(sandbox)) == ["auto"], checkpoints(sandbox)
            return {"first_ms": state["first"], "second_ms": state["second"]}
        case("after use it is replaced by a newer one, and there is still one", replaced)

        def left_alone():
            # Stop the only thing running in it. That is itself use, so one
            # more checkpoint follows; after that one nothing happens.
            execute(sandbox, "pkill -f 'echo \\$i' || kill %1 || true; killall sleep 2>/dev/null || true")
            execute(sandbox, "for p in $(pgrep sh); do [ $p -gt 10 ] && kill $p 2>/dev/null; done; true")
            settled = wait_for_auto(sandbox, state["second"], EVERY + 30)
            time.sleep(2 * EVERY + 10)
            now = checkpoints(sandbox).get("auto")
            assert now == settled, f"checkpointed again with nothing new: {settled} -> {now}"
            return {"seconds_left_alone": 2 * EVERY + 10, "checkpoint_ms": settled}
        case("left alone with nothing running, it is not checkpointed again", left_alone)

        def never():
            assert checkpoints(plain) == {}, checkpoints(plain)
            return {"seconds_running": round(time.time() - started, 1)}
        case("a sandbox without an interval is never checkpointed", never)

        def refusals():
            status, value = request("POST", "/sandboxes", {"timeout": 60, "autoCheckpoint": 5})
            assert status == 400 and "autoCheckpoint" in json.dumps(value), (status, value)
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "auto"})
            assert status == 400 and "by itself" in json.dumps(value), (status, value)
            # A named one is still the caller's to take, beside the automatic one.
            status, value = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "mine"})
            assert status == 201, (status, value)
            assert sorted(checkpoints(sandbox)) == ["auto", "mine"], checkpoints(sandbox)
        case("too short an interval and a caller's `auto` are refused; a named one still saves", refusals)

        report["success"] = True
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
