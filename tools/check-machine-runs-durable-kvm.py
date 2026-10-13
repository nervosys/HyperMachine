#!/usr/bin/env python3
"""A machine's runs kept across a node restart, on real KVM.

Runs an owned hv2-sandboxd with a --machine-dir and one machine, starts runs
in it, then kills the node outright (SIGKILL) and starts another on the same
--machine-dir. Checks through its API:

1. A run that had ended is there after the restart, as it ended: its exit
   code and both streams, byte for byte.
2. A run still going when the node was killed is there as lost, saying the
   node stopped, with what it had printed by the node's last poll.
3. A run that printed more than is kept (9 MiB, twice the 4 MiB kept and
   more) comes back with the same offsets: every byte counted, the newest
   4 MiB kept, the oldest dropped.
4. A new run after the restart takes the next ID, and works.
5. Deleting the machine deletes its runs, on disk too.

Writes report.json and both daemons' logs into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "keeper-01"
BIG = 9 * 1024 * 1024
KEPT = 4 * 1024 * 1024
CHUNK = 256 * 1024


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
              "purpose": "functional verification of machine runs kept across a node restart on KVM; "
                         "no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-runs-durable-check-", dir="/var/tmp"))
    machines = work / "machines"
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    daemons = []

    def start_daemon(label):
        log = (args.output / f"daemon-{label}.log").open("wb")
        daemon = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--no-template",
             "--machine-dir", str(machines), "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
            env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        daemons.append((daemon, log))
        deadline = time.monotonic() + 60
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/machines")[0] == 200:
                    return daemon
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.1)

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

    def start(cmd):
        status, run = request("POST", f"/machines/{NAME}/runs", {"cmd": cmd})
        assert status == 201, (status, run)
        return run["runID"]

    def read(run_id, stdout=0, stderr=0):
        status, run = request("GET", f"/machines/{NAME}/runs/{run_id}?stdout={stdout}&stderr={stderr}")
        assert status == 200, (status, run)
        return run

    def until(run_id, predicate, seconds=120):
        deadline = time.monotonic() + seconds
        while True:
            run = read(run_id)
            if predicate(run):
                return run
            assert time.monotonic() < deadline, f"run {run_id}: {run}"
            time.sleep(0.2)

    def until_running(seconds=180):
        deadline = time.monotonic() + seconds
        while True:
            status, machine = request("GET", f"/machines/{NAME}")
            if status == 200 and machine.get("state") == "running":
                # And its agent answering.
                status, ran = request("POST", f"/machines/{NAME}/exec", {"cmd": "true"})
                if status == 200:
                    return
            assert time.monotonic() < deadline, (status, machine)
            time.sleep(0.5)

    try:
        first = start_daemon("before")
        status, created = request("POST", "/machines", {
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1}, timeout=180)
        assert status == 201 and created["state"] == "running", (status, created)
        until_running()
        state = {}

        ended_id = start("printf 'out-1\\nout-2\\n'; printf 'err-1\\n' >&2; exit 3")
        ended = until(ended_id, lambda r: r["state"] != "running")
        assert ended["exitCode"] == 3, ended
        # At a pace the node's polls keep up with: the guest agent holds at
        # most 1 MiB of a program's output between them and drops the
        # oldest, uncounted, beyond that (a separate defect).
        big_id = start(f"i=0; while [ $i -lt {BIG // CHUNK} ]; do head -c {CHUNK} /dev/zero | tr '\\0' x; "
                       "sleep 0.2; i=$((i+1)); done; echo; echo big-done")
        big = until(big_id, lambda r: r["state"] != "running", seconds=300)
        assert big["stdoutBytes"] == BIG + len("\nbig-done\n"), big
        going_id = start("i=0; while :; do i=$((i+1)); echo tick-$i; sleep 0.2; done")
        going = until(going_id, lambda r: r["stdoutBytes"] >= len("tick-1\ntick-2\ntick-3\n"))
        before = {"ended": read(ended_id), "big": read(big_id, stdout=BIG), "going": read(going_id)}

        # Killed, not stopped: nothing is written on the way out.
        first.send_signal(signal.SIGKILL)
        first.wait(timeout=30)
        start_daemon("after")
        until_running()

        def ended_kept():
            after = read(ended_id)
            assert after["state"] == "exited" and after["exitCode"] == 3, after
            assert after["stdout"]["text"] == "out-1\nout-2\n", after["stdout"]
            assert after["stderr"]["text"] == "err-1\n", after["stderr"]
            for key in ["cmd", "pid", "startedAt", "endedAt", "stdoutBytes", "stderrBytes"]:
                assert after[key] == before["ended"][key], (key, after[key], before["ended"][key])
            return {"run": {k: after[k] for k in ["runID", "state", "exitCode", "stdoutBytes"]}}
        case("a run that had ended is there after the restart, as it ended", ended_kept)

        def going_lost():
            after = read(going_id)
            assert after["state"] == "lost" and "node stopped" in after["reason"], after
            text = after["stdout"]["text"]
            # Every tick it printed by the last poll, in order, none missing.
            ticks = [line for line in text.splitlines() if line]
            assert ticks == [f"tick-{i}" for i in range(1, len(ticks) + 1)], ticks[-3:]
            assert len(ticks) >= 3 and after["stdoutBytes"] == len(text), after["stdoutBytes"]
            return {"ticks_kept": len(ticks), "reason": after["reason"]}
        case("a run going when the node was killed is lost, saying so, with its output", going_lost)

        def big_kept():
            after = read(big_id, stdout=BIG)
            assert after["stdoutBytes"] == before["big"]["stdoutBytes"], after["stdoutBytes"]
            assert after["stdout"]["text"] == before["big"]["stdout"]["text"] == "\nbig-done\n"
            assert after["stdout"]["dropped"] == after["stdoutBytes"] - KEPT, after["stdout"]["dropped"]
            whole = read(big_id)["stdout"]
            assert whole["offset"] == whole["dropped"] and len(whole["text"]) == KEPT, whole["offset"]
            on_disk = (machines / created["machineID"] / "runs" / f"{big_id}.stdout").stat().st_size
            assert on_disk <= 2 * KEPT, on_disk
            return {"stdoutBytes": after["stdoutBytes"], "dropped": after["stdout"]["dropped"],
                    "file_bytes": on_disk}
        case("a run that printed more than is kept comes back with the same offsets", big_kept)

        def next_id():
            new_id = start("echo after-restart")
            assert new_id == going_id + 1, (new_id, going_id)
            run = until(new_id, lambda r: r["state"] != "running")
            assert run["stdout"]["text"] == "after-restart\n", run
            status, listed = request("GET", f"/machines/{NAME}/runs")
            assert [r["runID"] for r in listed] == [ended_id, big_id, going_id, new_id], listed
            return {"new_run": new_id}
        case("a new run after the restart takes the next ID, and works", next_id)

        def deleted():
            runs = machines / created["machineID"] / "runs"
            assert runs.is_dir() and any(runs.iterdir())
            status, value = request("POST", f"/machines/{NAME}/stop", {}, timeout=120)
            assert status in (200, 204), (status, value)
            status, value = request("DELETE", f"/machines/{NAME}", timeout=120)
            assert status in (200, 204), (status, value)
            assert not runs.exists()
            status, value = request("GET", f"/machines/{NAME}/runs/{ended_id}")
            assert status == 404, (status, value)
        case("deleting the machine deletes its runs, on disk too", deleted)

        report["success"] = True
    finally:
        for daemon, log in daemons:
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
