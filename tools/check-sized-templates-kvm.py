#!/usr/bin/env python3
"""Sandboxes of another size restored, not booted, on real KVM.

Runs an owned hv2-sandboxd whose template is one vCPU and 256 MiB, with
`--sized-templates 2 --template-size 2x1024`, and checks through its API and
from inside each guest. What each guest was restored from, if anything, is
the daemon's own log line for it: `VM '<id>' restored from <snapshot>`. A
guest with no such line booted. (A guest's boot_id cannot tell: the kernel
makes it when it is first read, after the restore.)

1. Built at start: a sandbox at 2 vCPUs and 1024 MiB is restored from the
   first one asked for. Two of them are restored from a snapshot that is
   not the base template's, see 2 processors and about 1 GiB, and have
   different random numbers.
2. Built when first asked for: the first sandbox at 2 vCPUs and 512 MiB
   boots, and once that size is built, the next two are restored from it.
3. A resize and a fork to a built size are restored from it too, with the
   variables a reboot keeps.
4. A sandbox restored at another size is still its own size: paused and
   resumed it keeps its memory, and a checkpoint of it is refused.
5. The limit: with two sizes held, a third boots every time.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, re, shutil, signal, socket, subprocess, tempfile, time
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
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification of restoring sandboxes of another size from templates built "
                         "at that size, on KVM; the times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-sized-templates-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon.log"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "12",
         "--sized-templates", "2", "--template-size", "2x1024"],
        env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300):
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

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def sh(sandbox, cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, (status, value)
        return value["stdout"].strip()

    def seen(sandbox):
        cpus, mem = sh(sandbox, "grep -c ^processor /proc/cpuinfo; grep MemTotal /proc/meminfo").splitlines()
        return int(cpus), int(mem.split()[1])

    def restored_from(sandbox):
        """The snapshot this sandbox's guest was last restored from, or None if it booted."""
        text = re.sub(r"\x1b\[[0-9;]*m", "", log_path.read_text(errors="replace"))
        # Its latest start: a resize or a resume provisions it again.
        latest = text.rfind(f"Provisioned VM '{sandbox}'")
        assert latest >= 0, f"{sandbox} was never provisioned"
        found = re.search(rf"VM '{sandbox}' restored from (\S+)", text[latest:])
        return found.group(1) if found else None

    def act(sandbox, action, body=None, want=(200, 201, 204)):
        began = time.monotonic()
        status, value = request("POST", f"/sandboxes/{sandbox}/{action}", body or {})
        assert status in want, (action, status, value)
        return value, round(time.monotonic() - began, 3)

    def create(**body):
        began = time.monotonic()
        status, made = request("POST", "/sandboxes", dict({"templateID": "base", "timeout": 1200}, **body))
        assert status in (200, 201), (status, made)
        return made["sandboxID"], round(time.monotonic() - began, 3)

    def gone(*sandboxes):
        for sandbox in sandboxes:
            request("DELETE", f"/sandboxes/{sandbox}")

    def built(size, within=180):
        line = f"base@{size}: restored from now on"
        deadline = time.monotonic() + within
        while line not in log_path.read_text(errors="replace"):
            assert daemon.poll() is None, "the daemon exited"
            assert time.monotonic() < deadline, f"{size} never built"
            time.sleep(0.2)

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if request("GET", "/sandboxes", timeout=5)[0] == 200:
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)
        state = {}

        def at_start():
            plain, _ = create()
            state["base"] = restored_from(plain)
            assert state["base"], "the base template was not restored from"
            gone(plain)
            built("2x1024")
            (a, took_a), (b, took_b) = create(cpuCount=2, memoryMB=1024), create(cpuCount=2, memoryMB=1024)
            big = restored_from(a)
            assert big and big == restored_from(b) != state["base"], (big, restored_from(b), state["base"])
            for one in (a, b):
                cpus, mem_kb = seen(one)
                assert cpus == 2 and 900 * 1024 < mem_kb <= 1024 * 1024, (cpus, mem_kb)
            random = "head -c 16 /dev/urandom | od -An -tx1"
            assert sh(a, random) != sh(b, random)
            state["big"] = big
            gone(a, b)
            return {"seconds": [took_a, took_b], "memory_kb": mem_kb, "restored_from": big}
        case("a size built at start: its sandboxes are restored from it, each with its own randomness", at_start)

        def when_asked():
            first, took_first = create(cpuCount=2, memoryMB=512)
            assert restored_from(first) is None, restored_from(first)
            built("2x512")
            (a, took_a), (b, took_b) = create(cpuCount=2, memoryMB=512), create(cpuCount=2, memoryMB=512)
            small = restored_from(a)
            assert small and small == restored_from(b), (small, restored_from(b))
            assert small not in (state["base"], state["big"])
            assert seen(a)[0] == 2
            state["small"] = small
            gone(first, a, b)
            return {"booted_seconds": took_first, "restored_seconds": [took_a, took_b], "restored_from": small}
        case("a size built when first asked for: the first boots, the next are restored", when_asked)

        def resize_and_fork():
            sandbox, _ = create(envVars={"KEEP": "kept"})
            value, took_resize = act(sandbox, "resize", {"cpuCount": 2, "memoryMB": 1024})
            assert value["resized"], value
            assert restored_from(sandbox) == state["big"] and seen(sandbox)[0] == 2
            assert sh(sandbox, "echo $KEEP") == "kept"
            value, took_fork = act(sandbox, "fork", {"cpuCount": 2, "memoryMB": 512, "timeout": 600})
            forked = value[0]["sandbox"]["sandboxID"]
            assert restored_from(forked) == state["small"]
            assert sh(forked, "echo $KEEP") == "kept"
            state["resized"] = sandbox
            gone(forked)
            return {"resize_seconds": took_resize, "fork_seconds": took_fork}
        case("a resize and a fork to a built size are restored from it", resize_and_fork)

        def own_size():
            sandbox = state["resized"]
            sh(sandbox, "echo through-a-pause > /tmp/in-memory")
            act(sandbox, "pause")
            act(sandbox, "resume")
            assert sh(sandbox, "cat /tmp/in-memory") == "through-a-pause"
            assert seen(sandbox)[0] == 2
            value, _ = act(sandbox, "checkpoints", {"name": "refused"}, want=(409,))
            gone(sandbox)
            return {"checkpoint_refused": value["message"]}
        case("restored at another size, it is still its own size: pauses whole, no checkpoint", own_size)

        def the_limit():
            starts = []
            for _ in range(2):
                one, _ = create(cpuCount=1, memoryMB=512)
                starts.append(restored_from(one))
                gone(one)
                time.sleep(2)
            assert starts == [None, None], starts
            assert "base@1x512" not in log_path.read_text(errors="replace")
            return {"restored_from": starts}
        case("with as many sizes held as allowed, another size boots every time", the_limit)

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
