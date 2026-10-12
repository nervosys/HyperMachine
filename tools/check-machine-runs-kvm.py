#!/usr/bin/env python3
"""Runs on real KVM: a command left running in a machine, its output kept.

Runs an owned hv2-sandboxd with a --machine-dir and one machine from the base
template, then checks, through its API:

1. A run answers at once with an ID, while its command is still going; its
   output arrives a piece at a time and is read from where the last read
   left off; and when it ends, its exit code and both streams are all there.
2. A run is told something on its standard input while it runs, and answers.
3. A run is signalled, and ends by that signal and not by an exit.
4. Two runs go at once, and each has its own output.
5. A finished run's output is still there to read, from the start, and the
   machine's runs are listed in the order they were started.
6. A run's environment reaches its command and is in nothing the API
   returns about it.
7. When the machine stops, a run still going ends as lost, saying so, with
   what it had printed; and no run can be started in a stopped machine.
8. What cannot be done is refused: an empty command, a run that is not
   there, input or a signal to one that has ended, a signal that is not one.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "runner-01"


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
    report = {"success": False,
              "purpose": "functional verification of machine runs on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-runs-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
         "--machine-dir", str(work / "machines"), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(method, path, body=None, timeout=60):
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

    def start(cmd, **more):
        status, run = request("POST", f"/machines/{NAME}/runs", dict(cmd=cmd, **more))
        assert status == 201, (status, run)
        assert run["state"] == "running" and run["exitCode"] is None, run
        return run["runID"]

    def read(run_id, stdout=0, stderr=0):
        status, run = request("GET", f"/machines/{NAME}/runs/{run_id}?stdout={stdout}&stderr={stderr}")
        assert status == 200, (status, run)
        return run

    def until_ended(run_id, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            run = read(run_id)
            if run["state"] != "running":
                return run
            assert time.monotonic() < deadline, f"run {run_id} still running: {run}"
            time.sleep(0.2)

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
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
        assert status == 201 and created["state"] == "running", (status, created)

        def follows():
            began = time.monotonic()
            run_id = start("for i in 1 2 3 4; do echo line-$i; sleep 1; done; echo to-stderr >&2; exit 3")
            answered_in = time.monotonic() - began
            # Answered while the command, which takes four seconds, was going.
            assert answered_in < 2.5, f"start took {answered_in:.1f}s"
            pieces, offset, reads = [], 0, 0
            deadline = time.monotonic() + 30
            while True:
                run = read(run_id, stdout=offset)
                reads += 1
                assert run["stdout"]["offset"] == offset, run["stdout"]
                if run["stdout"]["text"]:
                    pieces.append(run["stdout"]["text"])
                offset = run["stdout"]["next"]
                if run["state"] != "running":
                    break
                assert time.monotonic() < deadline, run
                time.sleep(0.3)
            assert "".join(pieces) == "line-1\nline-2\nline-3\nline-4\n", pieces
            # In more than one piece: it was read as it was written.
            assert len(pieces) >= 2, pieces
            assert (run["state"], run["exitCode"], run["signal"]) == ("exited", 3, None), run
            assert run["stderr"]["text"] == "to-stderr\n", run["stderr"]
            assert run["stdoutBytes"] == 28 and run["stderrBytes"] == 10, run
            assert run["endedAt"] >= run["startedAt"], run
            state["first"] = run_id
            return {"runID": run_id, "start_answered_in_seconds": round(answered_in, 2),
                    "pieces": len(pieces), "reads": reads, "exitCode": run["exitCode"]}
        state = {}
        case("a run answers at once, is read a piece at a time, and ends with its exit code", follows)

        def is_told():
            run_id = start("read a; echo first-$a; read b; echo second-$b")
            status, value = request("POST", f"/machines/{NAME}/runs/{run_id}/stdin", {"data": "one\n"})
            assert status == 204, (status, value)
            deadline = time.monotonic() + 20
            while "first-one" not in read(run_id)["stdout"]["text"]:
                assert time.monotonic() < deadline, read(run_id)
                time.sleep(0.2)
            assert read(run_id)["state"] == "running"
            status, value = request("POST", f"/machines/{NAME}/runs/{run_id}/stdin", {"data": "two\n", "close": True})
            assert status == 204, (status, value)
            run = until_ended(run_id)
            assert run["stdout"]["text"] == "first-one\nsecond-two\n" and run["exitCode"] == 0, run
            # And given whole at the start, with standard input closed after it.
            whole = until_ended(start("cat; echo end", stdin="fed at the start\n"))
            assert whole["stdout"]["text"] == "fed at the start\nend\n", whole
            return {"runID": run_id}
        case("a run is told something on its standard input while it runs, and answers", is_told)

        def is_signalled():
            run_id = start("echo waiting; exec sleep 300")
            deadline = time.monotonic() + 20
            while "waiting" not in read(run_id)["stdout"]["text"]:
                assert time.monotonic() < deadline
                time.sleep(0.2)
            status, value = request("POST", f"/machines/{NAME}/runs/{run_id}/signal", {"signal": "TERM"})
            assert status == 204, (status, value)
            run = until_ended(run_id, 20)
            assert (run["state"], run["exitCode"], run["signal"]) == ("exited", None, 15), run
            return {"runID": run_id, "signal": run["signal"]}
        case("a run is signalled, and ends by that signal", is_signalled)

        def two_at_once():
            a = start("for i in 1 2 3; do echo a-$i; sleep 0.5; done")
            b = start("for i in 1 2 3; do echo b-$i; sleep 0.5; done")
            ra, rb = until_ended(a), until_ended(b)
            assert ra["stdout"]["text"] == "a-1\na-2\na-3\n", ra
            assert rb["stdout"]["text"] == "b-1\nb-2\nb-3\n", rb
            # They overlapped: the second started before the first ended.
            assert rb["startedAt"] < ra["endedAt"], (ra, rb)
            return {"runs": [a, b]}
        case("two runs go at once, each with its own output", two_at_once)

        def kept():
            run = read(state["first"])
            assert run["stdout"]["text"] == "line-1\nline-2\nline-3\nline-4\n", run
            assert run["stdout"]["offset"] == 0 and run["stdout"]["dropped"] == 0, run
            # From part way: only what is after it.
            assert read(state["first"], stdout=14)["stdout"]["text"] == "line-3\nline-4\n"
            status, listed = request("GET", f"/machines/{NAME}/runs")
            assert status == 200, (status, listed)
            ids = [r["runID"] for r in listed]
            assert ids == sorted(ids) and ids[0] == state["first"], ids
            assert all("stdout" not in r for r in listed), "a listing carries no output"
            return {"listed": len(listed)}
        case("a finished run's output is still there, and the runs are listed in order", kept)

        def environment():
            run_id = start("echo got-$HM_CHECK_SECRET; echo in-$PWD", env={"HM_CHECK_SECRET": "s3cr3t-value"}, cwd="/tmp")
            run = until_ended(run_id)
            assert run["stdout"]["text"] == "got-s3cr3t-value\nin-/tmp\n", run
            # The value is in the command's own output, which is the point,
            # and nowhere else: not in the listing, and not in the run's record
            # apart from that output. The variable's name is in the command
            # line, which the caller wrote and the record does keep.
            status, listed = request("GET", f"/machines/{NAME}/runs")
            record = json.dumps([r for r in listed if r["runID"] == run_id])
            assert "s3cr3t-value" not in record, record
            described = dict(run, stdout=None, stderr=None)
            assert "s3cr3t-value" not in json.dumps(described), described
            assert "env" not in run, run
            return {"runID": run_id, "record_keys": sorted(described)}
        case("a run's environment reaches its command and is in nothing returned about it", environment)

        def lost_with_the_machine():
            run_id = start("echo before-the-stop; exec sleep 300")
            deadline = time.monotonic() + 20
            while "before-the-stop" not in read(run_id)["stdout"]["text"]:
                assert time.monotonic() < deadline
                time.sleep(0.2)
            status, stopped = request("POST", f"/machines/{NAME}/stop", timeout=120)
            assert status == 200 and stopped["state"] == "stopped", (status, stopped)
            run = until_ended(run_id, 30)
            assert run["state"] == "lost" and "stopped" in run["reason"], run
            assert run["stdout"]["text"] == "before-the-stop\n", run
            # History outlives the stop; nothing new starts in a stopped machine.
            assert read(state["first"])["exitCode"] == 3
            status, value = request("POST", f"/machines/{NAME}/runs", {"cmd": "true"})
            assert status == 409, (status, value)
            status, started = request("POST", f"/machines/{NAME}/start", timeout=180)
            assert status == 200 and started["state"] == "running", (status, started)
            again = until_ended(start("echo after-the-start"))
            assert again["stdout"]["text"] == "after-the-start\n", again
            return {"lost": run_id, "reason": run["reason"]}
        case("a run still going when the machine stops ends as lost, with what it printed", lost_with_the_machine)

        def refusals():
            ended = state["first"]
            results = {}
            for label, method, path, body, want in [
                ("empty command", "POST", "/runs", {"cmd": "  "}, 400),
                ("no such run", "GET", "/runs/99999", None, 404),
                ("input to an ended run", "POST", f"/runs/{ended}/stdin", {"data": "x"}, 409),
                ("signal to an ended run", "POST", f"/runs/{ended}/signal", {"signal": "TERM"}, 409),
                ("not a signal", "POST", f"/runs/{ended}/signal", {"signal": "NOPE"}, 400),
                ("no such machine", "POST", None, {"cmd": "true"}, 404),
            ]:
                target = f"/machines/absent-99/runs" if path is None else f"/machines/{NAME}{path}"
                status, value = request(method, target, body)
                assert status == want, (label, status, value)
                results[label] = value["message"]
            return results
        case("what cannot be done is refused, each with why", refusals)

        report["success"] = True
    finally:
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
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
