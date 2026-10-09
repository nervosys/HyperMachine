#!/usr/bin/env python3
"""The warm pool on real KVM: creates that take a sandbox restored beforehand.

Runs two owned hv2-sandboxd daemons from one binary, alike except that one has
`--warm-pool`. Through their APIs it checks, on the one with the pool:

1. The pool fills, and its spares are not listed as sandboxes.
2. A create takes a spare, and the sandbox it becomes is a working one of its
   own: it runs commands, has the environment it was created with, a clock
   that agrees with the host's, and random bytes that differ from its
   sibling's.
3. The pool refills after creates.
4. More creates at once than there are spares all succeed; the ones the pool
   could not serve are counted, not failed.
5. A sandbox from the pool pauses to disk, resumes and forks like any other.

Then it times `POST /sandboxes` on both daemons in alternating blocks (without,
with, with, without), one create at a time. These are measurements of this
build on this host, not a comparison with another product.

Writes report.json and both daemons' logs into --output.
"""
import argparse, hashlib, json, os, shutil, socket, statistics, subprocess, tempfile, threading, time
import urllib.error, urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def summary(values):
    ordered = sorted(values)

    def at(q):
        return round(ordered[min(len(ordered) - 1, int(q * len(ordered)))], 3)
    return {"n": len(ordered), "min": round(ordered[0], 3), "p50": round(statistics.median(ordered), 3),
            "p90": at(0.9), "p99": at(0.99), "max": round(ordered[-1], 3)}


class Node:
    def __init__(self, args, label, extra):
        self.api = port()
        self.base = f"http://127.0.0.1:{self.api}"
        env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
        self.log = (args.output / f"{label}.log").open("wb")
        self.process = subprocess.Popen(
            [str(args.daemon), "--port", str(self.api), "--proxy-port", str(port()), "--memory-mb", "512",
             "--cpu-cores", "1", "--capacity", "32"] + extra,
            env=env, stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT)

    def request(self, method, path, body=None, timeout=120):
        req = urllib.request.Request(self.base + path, method=method, headers={"content-type": "application/json"},
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

    def ready(self):
        deadline = time.monotonic() + 300
        while True:
            assert self.process.poll() is None, "a daemon exited"
            try:
                status, templates = self.request("GET", "/templates")
                if status == 200 and any(t.get("snapshot") for t in templates):
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.2)

    def create(self, **body):
        status, created = self.request("POST", "/sandboxes", {"timeout": 600, **body})
        assert status == 201, (status, created)
        return created["sandboxID"]

    def execute(self, sandbox, command):
        status, value = self.request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": command, "timeout_secs": 20})
        assert status == 200 and value["exit_code"] == 0, f"guest command failed: {status} {value}"
        return value["stdout"].strip()

    def delete(self, sandbox):
        self.request("DELETE", f"/sandboxes/{sandbox}")

    def pool(self):
        status, value = self.request("GET", "/pool")
        assert status == 200, (status, value)
        return value

    def wait_full(self, seconds=60):
        deadline = time.monotonic() + seconds
        while True:
            pool = self.pool()
            if pool["ready"] == pool["target"]:
                return pool
            assert time.monotonic() < deadline, f"the pool did not fill: {pool}"
            time.sleep(0.05)

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--spares", type=int, default=4)
    parser.add_argument("--block", type=int, default=50, help="creates per timing block")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "functional verification of the warm pool on KVM, with same-build "
                                           "timings; no comparison with another product",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "host_load_1m_at_start": os.getloadavg()[0], "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-pool-check-", dir="/var/tmp"))
    pooled = Node(args, "with-pool", ["--warm-pool", str(args.spares)])
    plain = Node(args, "without-pool", [])

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    try:
        pooled.ready()
        plain.ready()

        def fills():
            pool = pooled.wait_full()
            status, listed = pooled.request("GET", "/sandboxes")
            assert status == 200 and listed == [], listed
            assert plain.pool() == {"target": 0, "ready": 0, "handedOut": 0, "missed": 0}, plain.pool()
            return pool
        case("the pool fills, and its spares are not listed as sandboxes", fills)

        def a_sandbox_of_its_own():
            before = pooled.pool()
            first = pooled.create(envVars={"HM_POOL_CHECK": "first"})
            second = pooled.create(envVars={"HM_POOL_CHECK": "second"})
            after = pooled.pool()
            assert after["handedOut"] == before["handedOut"] + 2, (before, after)
            assert first != second
            assert pooled.execute(first, "echo $HM_POOL_CHECK") == "first"
            assert pooled.execute(second, "echo $HM_POOL_CHECK") == "second"
            randoms = [pooled.execute(s, "head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \\n'")
                       for s in (first, second)]
            assert len(randoms[0]) == 32 and randoms[0] != randoms[1], randoms
            skew = abs(int(pooled.execute(first, "date +%s")) - time.time())
            assert skew < 5, f"the guest's clock is {skew:.1f}s from the host's"
            status, state = pooled.request("GET", f"/sandboxes/{first}/standby")
            assert status == 200 and state["standby"] is False, state
            # Its own filesystem: what one writes, the other does not see.
            pooled.execute(first, "echo mine > /tmp/only-first")
            assert pooled.execute(second, "ls /tmp/only-first 2>/dev/null || echo absent") == "absent"
            pooled.delete(second)
            return {"first": first, "clock_skew_seconds": round(skew, 2), "random_differs": True,
                    "kept": first}
        case("a create takes a spare, and the sandbox is a working one of its own", a_sandbox_of_its_own)

        def refills():
            pool = pooled.wait_full()
            assert pool["ready"] == args.spares, pool
            return pool
        case("the pool refills after creates", refills)

        def more_than_the_pool():
            before = pooled.wait_full()
            wanted = args.spares + 3
            made, errors = [], []

            def one():
                try:
                    made.append(pooled.create())
                except AssertionError as error:
                    errors.append(str(error))
            threads = [threading.Thread(target=one) for _ in range(wanted)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
            assert not errors and len(set(made)) == wanted, (errors, made)
            for sandbox in made:
                assert pooled.execute(sandbox, "echo up") == "up"
            after = pooled.pool()
            served = after["handedOut"] - before["handedOut"]
            missed = after["missed"] - before["missed"]
            assert served + missed == wanted and served >= args.spares and missed >= 1, (before, after)
            for sandbox in made:
                pooled.delete(sandbox)
            return {"creates_at_once": wanted, "from_the_pool": served, "restored_as_before": missed}
        case("more creates at once than spares all succeed", more_than_the_pool)

        def like_any_other():
            pooled.wait_full()
            sandbox = pooled.create()
            pooled.execute(sandbox, "echo kept > /tmp/kept")
            status, value = pooled.request("POST", f"/sandboxes/{sandbox}/pause", {})
            assert status == 204, (status, value)
            status, value = pooled.request("POST", f"/sandboxes/{sandbox}/resume", {"timeout": 600})
            assert status == 201, (status, value)
            assert pooled.execute(sandbox, "cat /tmp/kept") == "kept"
            status, forked = pooled.request("POST", f"/sandboxes/{sandbox}/fork", {"count": 1, "timeout": 600})
            assert status == 201, (status, forked)
            child = forked[0]["sandbox"]["sandboxID"]
            assert pooled.execute(child, "cat /tmp/kept") == "kept"
            pooled.delete(child)
            pooled.delete(sandbox)
            return {"pause": 204, "resume": 201, "fork": 201}
        case("a sandbox from the pool pauses, resumes and forks like any other", like_any_other)

        def timed():
            def block(node):
                times = []
                for _ in range(args.block):
                    if node is pooled:
                        node.wait_full()
                    started = time.perf_counter()
                    sandbox = node.create()
                    times.append((time.perf_counter() - started) * 1000)
                    # Used once, so a create that handed back something that
                    # does not answer would fail here.
                    assert node.execute(sandbox, "echo up") == "up"
                    node.delete(sandbox)
                return times
            before = pooled.pool()
            results = {"without": [], "with": []}
            order = [("without", plain), ("with", pooled), ("with", pooled), ("without", plain)]
            for label, node in order:
                results[label].extend(block(node))
            after = pooled.pool()
            assert after["missed"] == before["missed"], "a timed create found the pool empty"
            assert after["handedOut"] - before["handedOut"] == 2 * args.block
            return {"order": [label for label, _ in order], "creates_per_block": args.block,
                    "create_ms_without_pool": summary(results["without"]),
                    "create_ms_with_pool": summary(results["with"]),
                    "host_load_1m": os.getloadavg()[0]}
        case("timings: POST /sandboxes without and with the pool, in alternating blocks", timed)
        report["success"] = True
    finally:
        pooled.stop()
        plain.stop()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
