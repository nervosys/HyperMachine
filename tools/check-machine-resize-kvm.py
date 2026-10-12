#!/usr/bin/env python3
"""Resizing on real KVM: a stopped machine given more processors, more
memory and a larger disk, and its guest finding them.

Runs an owned hv2-sandboxd with a --machine-dir and one machine from the base
template, then checks, through its API and from inside the guest:

1. A running machine is not resized, and is left as it was.
2. Stopped, it takes other processors, other memory and a larger disk, and
   its record says so before it is started.
3. Started, the guest has them: the processors it counts, the memory it
   reports, and a root filesystem as large as the disk, with what was on it.
4. One size changed alone leaves the others.
5. What cannot be is refused, and changes nothing.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

NAME = "sized-01"


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
              "purpose": "functional verification of machine resizing on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-machine-resize-check-", dir="/var/tmp"))
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

    def request(method, path, body=None, timeout=180):
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

    def guest(cmd):
        status, value = request("POST", f"/machines/{NAME}/exec", {"cmd": cmd, "timeout_secs": 60})
        assert status == 200 and value["exit_code"] == 0, (cmd, status, value)
        return value["stdout"]

    def seen_by_the_guest():
        return {"processors": int(guest("grep -c ^processor /proc/cpuinfo")),
                "memory_kb": int(guest("grep MemTotal /proc/meminfo").split()[1]),
                "root_kb": int(guest("df -k / | tail -1").split()[1])}

    def sizes(machine):
        return {k: machine[k] for k in ["cpuCount", "memoryMB", "diskGiB"]}

    def record():
        status, machine = request("GET", f"/machines/{NAME}")
        assert status == 200, (status, machine)
        return machine

    def power(action, state):
        status, machine = request("POST", f"/machines/{NAME}/{action}")
        assert status == 200 and machine["state"] == state, (action, status, machine)

    def disk_bytes():
        found = list((work / "machines").glob("*/root.img"))
        assert len(found) == 1, found
        return found[0].stat().st_size

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
            "name": NAME, "diskGiB": 1, "memoryMB": 512, "cpuCount": 1})
        assert status == 201 and created["state"] == "running", (status, created)
        assert sizes(created) == {"cpuCount": 1, "memoryMB": 512, "diskGiB": 1}, created
        state = {"before": seen_by_the_guest()}
        report["as_created"] = state["before"]
        assert state["before"]["processors"] == 1 and state["before"]["memory_kb"] < 512 * 1024, state
        assert state["before"]["root_kb"] < 1024 * 1024, state
        assert disk_bytes() == 1 << 30
        # Something on the disk to find again, that nothing could remake.
        state["marker"] = guest("dd if=/dev/urandom of=/root/marker bs=1k count=700 2>/dev/null; sync; "
                                "sha256sum /root/marker").split()[0]

        def running_is_refused():
            status, value = request("PATCH", f"/machines/{NAME}", {"cpuCount": 2, "diskGiB": 3})
            assert status == 409 and "stop it first" in value["message"], (status, value)
            assert sizes(record()) == {"cpuCount": 1, "memoryMB": 512, "diskGiB": 1}
            assert disk_bytes() == 1 << 30, "a running machine's disk was grown"
            return {"refused": value["message"]}
        case("a running machine is not resized, and is left as it was", running_is_refused)

        def resized_while_stopped():
            power("stop", "stopped")
            status, machine = request("PATCH", f"/machines/{NAME}", {"cpuCount": 2, "memoryMB": 1024, "diskGiB": 3})
            assert status == 200, (status, machine)
            want = {"cpuCount": 2, "memoryMB": 1024, "diskGiB": 3}
            assert sizes(machine) == want and machine["state"] == "stopped", machine
            assert sizes(record()) == want
            assert disk_bytes() == 3 << 30
            return {"record": want, "disk_bytes": disk_bytes()}
        case("stopped, it takes other processors, memory and a larger disk, and its record says so",
             resized_while_stopped)

        def the_guest_has_them():
            power("start", "running")
            after = seen_by_the_guest()
            assert after["processors"] == 2, after
            # Less than was given, by what the kernel keeps; more than before
            # by about what was added.
            assert 900 * 1024 < after["memory_kb"] <= 1024 * 1024, after
            # The filesystem is the disk's size, less its own bookkeeping.
            assert 2.8 * 1024 * 1024 < after["root_kb"] <= 3 * 1024 * 1024, after
            assert guest("sha256sum /root/marker").split()[0] == state["marker"], "what was on the disk changed"
            # And the new space can be written to: more than the old disk held.
            guest("dd if=/dev/zero of=/root/fill bs=1M count=1500 2>/dev/null && sync && rm /root/fill")
            return {"before": state["before"], "after": after}
        case("started, the guest counts the processors, reports the memory, and has a root as large as the disk",
             the_guest_has_them)

        def one_alone():
            power("stop", "stopped")
            status, machine = request("PATCH", f"/machines/{NAME}", {"memoryMB": 768})
            assert status == 200 and sizes(machine) == {"cpuCount": 2, "memoryMB": 768, "diskGiB": 3}, (status, machine)
            # The size it has is not a change, and is not refused.
            status, machine = request("PATCH", f"/machines/{NAME}", {"diskGiB": 3})
            assert status == 200 and machine["diskGiB"] == 3 and disk_bytes() == 3 << 30, (status, machine)
            power("start", "running")
            after = seen_by_the_guest()
            assert after["processors"] == 2, after
            assert 650 * 1024 < after["memory_kb"] <= 768 * 1024, after
            assert after["root_kb"] > 2.8 * 1024 * 1024, after
            assert guest("sha256sum /root/marker").split()[0] == state["marker"]
            return {"after": after}
        case("one size changed alone leaves the others", one_alone)

        def refusals():
            power("stop", "stopped")
            before = sizes(record())
            results = {}
            for label, body, want in [
                ("a smaller disk", {"diskGiB": 2}, 400),
                ("a smaller disk, with sizes that could be", {"cpuCount": 1, "diskGiB": 1}, 400),
                ("no processors", {"cpuCount": 0}, 400),
                ("too little memory", {"memoryMB": 64}, 400),
                ("a disk too large", {"diskGiB": 4096}, 400),
                ("nothing", {}, 400),
                ("something that is not a size", {"name": "other", "cpuCount": 1}, 422),
            ]:
                status, value = request("PATCH", f"/machines/{NAME}", body)
                assert status == want, (label, status, value)
                results[label] = value["message"] if isinstance(value, dict) else value[:120]
            status, value = request("PATCH", "/machines/absent-99", {"cpuCount": 1})
            assert status == 404, (status, value)
            assert sizes(record()) == before and disk_bytes() == 3 << 30, "a refusal changed something"
            power("start", "running")
            assert guest("sha256sum /root/marker").split()[0] == state["marker"]
            return results
        case("what cannot be is refused, and changes nothing", refusals)

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
