#!/usr/bin/env python3
"""A run that prints faster than it is polled, on real KVM.

The guest agent holds at most 1 MiB of a started program's output between
the node's polls. Before this check's change it dropped the oldest beyond
that without saying so, and a run's byte count and `dropped` were wrong.

Runs an owned hv2-sandboxd with a --machine-dir and one machine, and checks
through its API:

1. A run printing 9 MiB as fast as it can, then a last line: every byte is
   accounted for, as kept or dropped, and the last line is there.
2. A run printing 3 MiB of two-byte characters as fast as it can: the
   text kept has no replacement character in it, so no character was cut
   in two by a poll or by a drop.
3. A run printing slowly loses nothing: dropped is 0.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "flood-01"
BIG = 9 * 1024 * 1024


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
              "purpose": "functional verification of runs that outprint their polls on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-run-flood-check-", dir="/var/tmp"))
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

    def start(cmd):
        status, run = request("POST", f"/machines/{NAME}/runs", {"cmd": cmd})
        assert status == 201, (status, run)
        return run["runID"]

    def ended(run_id, seconds=300):
        deadline = time.monotonic() + seconds
        while True:
            status, run = request("GET", f"/machines/{NAME}/runs/{run_id}")
            assert status == 200, (status, run)
            if run["state"] != "running":
                return run
            assert time.monotonic() < deadline, run
            time.sleep(0.5)

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

        def flood():
            began = time.monotonic()
            run = ended(start(f"head -c {BIG} /dev/zero | tr '\\0' x; echo; echo flood-done"))
            took = round(time.monotonic() - began, 2)
            out = run["stdout"]
            assert run["exitCode"] == 0, run
            assert run["stdoutBytes"] == BIG + len("\nflood-done\n"), run["stdoutBytes"]
            assert out["text"].endswith("\nflood-done\n"), out["text"][-40:]
            # Every byte is kept or dropped, and what is kept is all x.
            assert out["offset"] == out["dropped"], (out["offset"], out["dropped"])
            assert out["dropped"] + len(out["text"].encode()) == run["stdoutBytes"]
            assert set(out["text"][:-len("\nflood-done\n")]) <= {"x"}
            return {"stdoutBytes": run["stdoutBytes"], "dropped": out["dropped"],
                    "kept": len(out["text"].encode()), "seconds": took}
        case("a run printing faster than it is polled has every byte accounted for", flood)

        def characters():
            # Lines of 1,024 two-byte characters and a newline, 1,536 of them
            # (3 MiB), as fast as `yes` writes: a newline every 2,049 bytes
            # puts characters across every boundary a read or a drop makes.
            run = ended(start("s=$(printf '\\303\\251'); for k in 1 2 3 4 5 6 7 8 9 10; do s=\"$s$s\"; done; "
                              "yes \"$s\" | head -n 1536; echo chars-done"))
            out = run["stdout"]
            assert run["exitCode"] == 0, run
            text = out["text"]
            assert text.endswith("\nchars-done\n"), text[-40:]
            assert "�" not in text, text.count("�")
            body = text[:-len("chars-done\n")]
            assert set(body) <= {"é", "\n"}, set(body) - {"é", "\n"}
            return {"stdoutBytes": run["stdoutBytes"], "dropped": out["dropped"],
                    "characters_kept": len(body)}
        case("two-byte characters flooded are never cut in two", characters)

        def slow():
            run = ended(start("i=0; while [ $i -lt 20 ]; do echo line-$i; sleep 0.05; i=$((i+1)); done"))
            out = run["stdout"]
            assert out["dropped"] == 0 and out["offset"] == 0, out
            assert out["text"] == "".join(f"line-{i}\n" for i in range(20)), out["text"]
            return {"stdoutBytes": run["stdoutBytes"]}
        case("a run printing slowly loses nothing", slow)

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
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
