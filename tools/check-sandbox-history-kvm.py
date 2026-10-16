#!/usr/bin/env python3
"""A sandbox's commands kept and read again, on real KVM.

Runs two owned hv2-sandboxd nodes sharing a --snapshot-store, and drives
one sandbox on the first through its API and through the E2B Python SDK
(2.51.0), whose commands.run starts each command through envd's
process.Process/Start. Checks:

1. A command run through exec is kept: its command line, exit code and
   both streams.
2. Commands run through the E2B SDK are kept the same way, as `process`,
   with their PIDs; one killed through the SDK is kept as signalled. A
   command line's secrets are masked before it is kept, and a sandbox
   created with commandHistory off keeps none.
3. A command that printed more than is kept: every byte counted, the
   newest 64 KiB kept.
4. Paused and resumed on the same node, its commands are still there.
5. Paused into the store and resumed on the other node, its commands are
   there too, and the next command is numbered after them.
6. What cannot be read is refused, and a deleted sandbox's commands go.

Writes report.json and both daemons' logs into --output.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

KEPT = 64 * 1024


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
    import importlib.metadata
    from e2b import Sandbox
    report = {"success": False,
              "purpose": "functional verification of a sandbox's command history on KVM; no timing claim",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "e2b_sdk": importlib.metadata.version("e2b"),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-sandbox-history-check-", dir="/var/tmp"))
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    nodes = {}
    daemons = []

    def start_daemon(label):
        api, proxy = port(), port()
        log = (args.output / f"daemon-{label}.log").open("wb")
        daemon = subprocess.Popen(
            [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy),
             "--snapshot-store", str(work / "store"), "--node-id", label,
             "--memory-mb", "256", "--cpu-cores", "1", "--capacity", "4"],
            env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        daemons.append((daemon, log))
        nodes[label] = (f"http://127.0.0.1:{api}", f"http://127.0.0.1:{proxy}")
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, f"daemon {label} exited"
            try:
                if request(label, "GET", "/sandboxes", timeout=5)[0] == 200:
                    return
            except OSError:
                pass
            assert time.monotonic() < deadline, f"daemon {label} readiness"
            time.sleep(0.2)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def request(node, method, path, body=None, timeout=120):
        req = urllib.request.Request(nodes[node][0] + path, method=method,
                                     headers={"content-type": "application/json"},
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

    def commands(node, sandbox, want=200):
        status, value = request(node, "GET", f"/sandboxes/{sandbox}/commands")
        assert status == want, (status, value)
        return value

    def command(node, sandbox, n, want=200):
        status, value = request(node, "GET", f"/sandboxes/{sandbox}/commands/{n}")
        assert status == want, (status, value)
        return value

    def until_ended(node, sandbox, n, seconds=30):
        deadline = time.monotonic() + seconds
        while True:
            c = command(node, sandbox, n)
            if c["state"] != "running":
                return c
            assert time.monotonic() < deadline, c
            time.sleep(0.1)

    try:
        start_daemon("a")
        start_daemon("b")
        sdk = Sandbox.create(template="base", timeout=600, api_key="hm-local", api_url=nodes["a"][0],
                             sandbox_url=nodes["a"][1], request_timeout=120)
        sandbox = sdk.sandbox_id
        state = {}

        def exec_kept():
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/exec",
                                    {"cmd": "echo hi; echo err >&2; exit 4"})
            assert status == 200 and value["exit_code"] == 4, (status, value)
            # E2B's commands.run starts /bin/bash; this busybox guest has sh.
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/exec",
                                    {"cmd": "[ -e /bin/bash ] || ln -s /bin/sh /bin/bash"})
            assert status == 200 and value["exit_code"] == 0, (status, value)
            first = command("a", sandbox, 1)
            assert first["source"] == "exec" and first["state"] == "exited", first
            assert first["cmd"] == "echo hi; echo err >&2; exit 4" and first["exitCode"] == 4, first
            assert first["stdout"]["text"] == "hi\n" and first["stderr"]["text"] == "err\n", first
            return {"command": {k: first[k] for k in ["n", "source", "state", "exitCode"]}}
        case("a command run through exec is kept, with its exit code and output", exec_kept)

        def sdk_kept():
            result = sdk.commands.run("echo from-sdk; sleep 0.2; echo done", timeout=30)
            assert result.exit_code == 0 and result.stdout == "from-sdk\ndone\n", result
            listed = commands("a", sandbox)
            ran = [c for c in listed if c["source"] == "process"]
            assert len(ran) == 1 and "echo from-sdk" in ran[0]["cmd"] and ran[0]["pid"], listed
            kept = until_ended("a", sandbox, ran[0]["n"])
            assert kept["state"] == "exited" and kept["exitCode"] == 0, kept
            assert kept["stdout"]["text"] == "from-sdk\ndone\n", kept["stdout"]
            # One killed through the SDK.
            handle = sdk.commands.run("sleep 300", background=True, timeout=0)
            handle.kill()
            killed = [c for c in commands("a", sandbox) if c["source"] == "process" and "sleep 300" in c["cmd"]]
            assert len(killed) == 1, killed
            killed = until_ended("a", sandbox, killed[0]["n"])
            assert killed["state"] == "signalled" and killed["signal"] == 9, killed
            state["count"] = len(commands("a", sandbox))
            return {"sdk_command": {k: kept[k] for k in ["n", "source", "cmd", "pid", "state"]},
                    "killed": {k: killed[k] for k in ["n", "state", "signal"]}}
        case("commands run through the E2B SDK are kept, a killed one as signalled", sdk_kept)

        def masked():
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/exec", {
                "cmd": "TOKEN=abc123 true --password hunter2 https://u:pw@h.example/x"})
            assert status == 200, (status, value)
            kept = commands("a", sandbox)[-1]
            assert kept["cmd"] == "TOKEN=[redacted] true --password [redacted] https://[redacted]@h.example/x", kept
            # The command prints nothing: output is kept as printed, not
            # scanned, so a command that echoes a secret keeps it there.
            raw = json.dumps(command("a", sandbox, kept["n"]))
            for secret in ["abc123", "hunter2", "u:pw"]:
                assert secret not in raw.replace("[redacted]", ""), (secret, raw)
            return {"kept_as": kept["cmd"]}
        case("a command line's secrets are masked before it is kept", masked)

        def off():
            status, made = request("a", "POST", "/sandboxes",
                                   {"templateID": "base", "timeout": 300, "commandHistory": False})
            assert status in (200, 201), (status, made)
            quiet = made["sandboxID"]
            status, value = request("a", "POST", f"/sandboxes/{quiet}/exec", {"cmd": "echo unseen"})
            assert status == 200, (status, value)
            status, value = request("a", "GET", f"/sandboxes/{quiet}/commands")
            assert status == 409, (status, value)
            request("a", "DELETE", f"/sandboxes/{quiet}")
            return {"refused": value["message"]}
        case("a sandbox created with commandHistory off keeps none", off)

        def long_output():
            size = 200 * 1024
            result = sdk.commands.run(f"head -c {size} /dev/zero | tr '\\0' y", timeout=60)
            assert result.exit_code == 0 and len(result.stdout) == size, len(result.stdout)
            last = commands("a", sandbox)[-1]
            kept = until_ended("a", sandbox, last["n"])
            assert kept["stdoutBytes"] == size, kept["stdoutBytes"]
            assert kept["stdout"]["dropped"] == size - KEPT and len(kept["stdout"]["text"]) == KEPT
            return {"stdoutBytes": kept["stdoutBytes"], "dropped": kept["stdout"]["dropped"]}
        case("a command that printed more than is kept: every byte counted, the newest kept", long_output)

        def local_pause():
            before = commands("a", sandbox)
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/pause", {})
            assert status in (200, 204), (status, value)
            # Readable while paused.
            assert commands("a", sandbox) == before
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/resume", {})
            assert status == 201, (status, value)
            assert commands("a", sandbox) == before
            return {"commands": len(before)}
        case("paused and resumed on the same node, its commands are still there", local_pause)

        def other_node():
            before = commands("a", sandbox)
            status, value = request("a", "POST", f"/sandboxes/{sandbox}/pause", {})
            assert status in (200, 204), (status, value)
            meta = json.loads((work / "store" / "paused" / f"{sandbox}.json").read_text())
            assert len(meta.get("history", [])) == len(before), meta.keys()
            status, value = request("b", "POST", f"/sandboxes/{sandbox}/resume", {})
            assert status == 201, (status, value)
            after = commands("b", sandbox)
            assert after == before, (len(after), len(before))
            status, value = request("b", "POST", f"/sandboxes/{sandbox}/exec", {"cmd": "echo on-b"})
            assert status == 200, (status, value)
            newest = commands("b", sandbox)[-1]
            assert newest["n"] == before[-1]["n"] + 1 and newest["cmd"] == "echo on-b", newest
            return {"carried": len(after), "next": newest["n"]}
        case("paused into the store and resumed on the other node, its commands come too", other_node)

        def refusals():
            refused = {}
            refused["no such command"] = command("b", sandbox, 9999, want=404)["message"]
            refused["no such sandbox"] = commands("b", "sbx-absent", want=404)["message"]
            status, value = request("b", "DELETE", f"/sandboxes/{sandbox}")
            assert status in (200, 204), (status, value)
            refused["a deleted sandbox"] = commands("b", sandbox, want=404)["message"]
            return {"refused": refused}
        case("what cannot be read is refused, and a deleted sandbox's commands go", refusals)

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
