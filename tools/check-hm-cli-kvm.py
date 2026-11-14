#!/usr/bin/env python3
"""The `hm sandbox vm` command line, driven end to end against a real hv2-sandboxd on KVM.

Starts an owned hv2-sandboxd (one template, a --disk-dir) and drives it only
through the real `hm` binary, as a subprocess, parsing the JSON the CLI prints.
Cases, one per subcommand area:

 1. disk create / list / inspect
 2. create (--name, --cpus, --memory-mb), list, inspect, and the guest's CPU
    and memory match what was asked
 3. exec: stdout, stderr, the guest's own exit code, argv quoting, --timeout (124)
 4. ps and history
 5. files upload / download: a binary payload's SHA-256, and no overwrite
 6. checkpoint save / list / ls / cat / restore / delete
 7. pause, inspect says paused, resume, state kept
 8. fork --count 1: the child has the parent's state
 9. alias bind / inspect
10. network create / list / connect / inspect / disconnect / delete
11. a sandbox with `create --disk`: data written survives its deletion, a new
    sandbox with the disk reads it, disk delete is refused while held
12. error paths: exec/inspect/pause on a deleted sandbox, a bad argument
13. every sandbox the check made is gone
14. the daemon log says platform: Kvm and never platform: Tcg

A case that fails is recorded and the check goes on; success needs all of them.
Not covered (need a cluster, a control plane, a registry or a browser): see the
README beside the report. Writes report.json and daemon.log into --output.
"""
import argparse, hashlib, json, os, re, shutil, signal, socket, subprocess, tempfile, time, traceback
import urllib.request
from pathlib import Path


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "hm", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "hm sandbox vm, end to end against hv2-sandboxd on KVM; functional",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-cli-check-", dir="/var/tmp"))
    api = port()
    url = f"http://127.0.0.1:{api}"
    log_path = args.output / "daemon.log"
    log = log_path.open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--disk-dir", str(work / "disks"),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
    env = {k: v for k, v in os.environ.items() if k not in ("HV2_API_KEY", "HV2_SANDBOX_URL")} | {"HOME": str(work)}
    owned = set()
    transcript = []

    def get(path):
        with urllib.request.urlopen(url + path, timeout=30) as r:
            return json.loads(r.read())

    def hm(*argv, expect=0, timeout=300):
        """Run `hm sandbox vm ARGV`. Returns (stdout, stderr); asserts the exit code."""
        done = subprocess.run([str(args.hm), "sandbox", "vm", "--endpoint", url, "--request-timeout", "120", *argv],
                              capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL, env=env)
        transcript.append({"argv": list(argv), "exit": done.returncode, "stdout": done.stdout[:400],
                           "stderr": done.stderr[:400]})
        assert done.returncode == expect, \
            f"hm {' '.join(argv)} exited {done.returncode}, expected {expect}: {done.stdout[:300]} | {done.stderr[:300]}"
        return done.stdout, done.stderr

    def hmj(*argv, **kw):
        out, _ = hm(*argv, **kw)
        try:
            return json.loads(out)
        except ValueError:
            raise AssertionError(f"hm {' '.join(argv)} did not print JSON: {out[:300]!r}")

    def sh(sandbox, script, **kw):
        out, _ = hm("exec", sandbox, "--", "/bin/sh", "-c", script, **kw)
        return out.strip()

    def create(*extra):
        made = hmj("create", "--lifetime", "900", *extra)
        owned.add(made["sandboxID"])
        return made

    def delete(sandbox):
        hm("delete", sandbox)
        owned.discard(sandbox)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        began = time.monotonic()
        try:
            detail = action()
            row["success"] = True
            if detail:
                row["detail"] = detail
            print("ok:", name, flush=True)
        except Exception as error:
            row["error"] = f"{type(error).__name__}: {error}"[:1500]
            row["traceback_tail"] = traceback.format_exc().splitlines()[-4:]
            print("FAIL:", name, "--", row["error"][:300], flush=True)
        row["seconds"] = round(time.monotonic() - began, 1)

    state = {}
    try:
        deadline = time.monotonic() + 300
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                if any(t.get("snapshot") for t in get("/templates")):
                    break
            except OSError:
                pass
            assert time.monotonic() < deadline, "daemon readiness"
            time.sleep(0.3)

        def disks():
            made = hmj("disk", "create", "clidata", "--size-mib", "64")
            listed = hmj("disk", "list")
            state["disk"] = made["diskID"]
            inspected = hmj("disk", "inspect", state["disk"])
            assert made.get("name") == "clidata" or made.get("diskID"), made
            names = json.dumps(listed)
            assert "clidata" in names, listed
            assert "clidata" in json.dumps(inspected), inspected
            return {"created": made, "inspected": inspected}
        case("disk create / list / inspect print JSON naming the disk", disks)

        def creating():
            made = create("--name", "cli-a", "--cpus", "1", "--memory-mb", "512")
            state["a"] = a = made["sandboxID"]
            state["a_port"] = made["processPort"]
            listed = hmj("list")
            assert a in [s["sandboxID"] for s in listed], listed
            info = hmj("inspect", a)
            assert info["sandboxID"] == a and info["state"] == "running", info
            cpus = int(sh(a, "grep -c ^processor /proc/cpuinfo"))
            mem_kb = int(sh(a, "awk '/MemTotal/{print $2}' /proc/meminfo"))
            assert cpus == 1, cpus
            assert 300_000 < mem_kb < 560_000, mem_kb
            return {"sandboxID": a, "state": info["state"], "guest_cpus": cpus, "guest_memtotal_kb": mem_kb,
                    "create_keys": sorted(made)}
        case("create --name --cpus --memory-mb; list and inspect show it; the guest has what was asked", creating)
        a = state.get("a")

        def exec_cases():
            out, _ = hm("exec", a, "--", "echo", "hello")
            assert out == "hello\n", out
            out, err = hm("exec", a, "--", "/bin/sh", "-c", "echo out; echo err >&2; exit 7", expect=7)
            assert out == "out\n" and err.endswith("err\n"), (out, err)
            literal = "a'b; $(touch /root/cli-injection); `echo substituted`"
            out, _ = hm("exec", a, "--", "/bin/busybox", "printf", "%s", literal)
            assert out == literal, out
            assert sh(a, "test ! -e /root/cli-injection && echo clean") == "clean"
            began = time.monotonic()
            hm("exec", a, "--timeout", "2", "--", "sleep", "30", expect=124)
            took = round(time.monotonic() - began, 1)
            assert took < 20, took
            return {"timeout_exit": 124, "timeout_seconds": took}
        case("exec: stdout, stderr, the guest's exit code, argv quoting, --timeout gives 124", exec_cases)

        def ps_history():
            procs = hmj("ps", a)
            assert procs, procs
            hist = hmj("history", a)
            text = json.dumps(hist)
            assert "echo hello" in text or "hello" in text, text[:300]
            return {"ps_type": type(procs).__name__, "history_entries": len(hist) if hasattr(hist, "__len__") else None}
        case("ps and history print JSON, and history holds the earlier exec", ps_history)

        def files():
            payload = bytes(range(256)) * 1024
            src, dst = work / "src.bin", work / "dst.bin"
            src.write_bytes(payload)
            route = ["files", a, "--envd-endpoint", f"http://127.0.0.1:{state['a_port']}"]
            up = hmj(*route, "upload", str(src), "/root/cli-file.bin")
            guest_sum = sh(a, "sha256sum /root/cli-file.bin").split()[0]
            assert guest_sum == hashlib.sha256(payload).hexdigest(), guest_sum
            down = hmj(*route, "download", "/root/cli-file.bin", str(dst))
            assert dst.read_bytes() == payload
            hm(*route, "download", "/root/cli-file.bin", str(dst), expect=1)
            assert dst.read_bytes() == payload
            return {"upload": up, "download": down, "sha256": guest_sum}
        case("files upload / download move a 256 KiB binary intact (SHA-256) and refuse to overwrite", files)

        def checkpoints():
            sh(a, "printf before > /root/cli-state")
            saved = hmj("checkpoint", "save", a, "before")
            listed = hmj("checkpoint", "list", a)
            assert "before" in json.dumps(listed), listed
            sh(a, "printf after > /root/cli-state")
            ls = hmj("checkpoint", "ls", a, "before", "/root")
            assert "cli-state" in json.dumps(ls), ls
            cat, _ = hm("checkpoint", "cat", a, "before", "/root/cli-state")
            assert cat == "before", cat
            assert sh(a, "cat /root/cli-state") == "after"
            restored = hmj("checkpoint", "restore", a, "before")
            assert sh(a, "cat /root/cli-state") == "before"
            hm("checkpoint", "delete", a, "before")
            assert "before" not in json.dumps(hmj("checkpoint", "list", a))
            return {"saved": saved, "restored": restored}
        case("checkpoint save / list / ls / cat / restore / delete: the guest goes back to the saved state", checkpoints)

        def pause_resume():
            sh(a, "echo kept > /tmp/ram; (while true; do sleep 1; done) >/dev/null 2>&1 &")
            hm("pause", a)
            assert hmj("inspect", a)["state"] == "paused"
            resumed = hmj("resume", a, "--lifetime", "900")
            assert hmj("inspect", a)["state"] == "running"
            assert sh(a, "cat /tmp/ram") == "kept"
            return {"resume_keys": sorted(resumed) if isinstance(resumed, dict) else resumed}
        case("pause, inspect says paused, resume; the guest's RAM state is kept", pause_resume)

        def forking():
            sh(a, "printf forkme > /root/fork-state")
            kids = hmj("fork", a, "--count", "1", "--lifetime", "900")
            for k in kids:
                if "sandbox" in k:
                    owned.add(k["sandbox"]["sandboxID"])
            assert len(kids) == 1 and "sandbox" in kids[0], kids
            state["child"] = child = kids[0]["sandbox"]["sandboxID"]
            assert sh(child, "cat /root/fork-state") == "forkme"
            sh(child, "printf child-only > /root/fork-state")
            assert sh(a, "cat /root/fork-state") == "forkme"
            return {"child": child}
        case("fork --count 1: the child has the parent's state and diverges from it", forking)

        def aliases():
            named = [s for s in hmj("list") if s.get("metadata", {}).get("hm.name") == "cli-a"]
            assert [s["sandboxID"] for s in named] == [a], named
            # Reserved aliases live in the control plane; a standalone daemon has no such route.
            _, err = hm("alias", "bind", a, "cli-alias", expect=1)
            assert "404" in err, err
            return {"create_name_in_metadata": True, "alias_bind_on_standalone_daemon_stderr": err.strip()}
        case("create --name is in list metadata; alias bind on a standalone daemon (no control plane) exits 1 with the API's 404", aliases)

        def networks():
            child = state["child"]
            hmj("network", "create", "clinet")
            assert "clinet" in json.dumps(hmj("network", "list"))
            hmj("network", "connect", "clinet", "--sandbox-id", a, "--alias", "alpha")
            hmj("network", "connect", "clinet", "--sandbox-id", child, "--alias", "beta")
            info = hmj("network", "inspect", "clinet")
            text = json.dumps(info)
            assert "alpha" in text and "beta" in text, info
            hm("network", "disconnect", "clinet", "--sandbox-id", child)
            assert "beta" not in json.dumps(hmj("network", "inspect", "clinet"))
            hm("network", "disconnect", "clinet", "--sandbox-id", a)
            hm("network", "delete", "clinet")
            assert "clinet" not in json.dumps(hmj("network", "list"))
            return {"members": info}
        case("network create / list / connect / inspect / disconnect / delete", networks)

        def standby_resize():
            status = hmj("standby", a)
            assert status.get("standby") is True, status
            assert sh(a, "cat /tmp/ram") == "kept"      # any request wakes it
            return {"standby_reply": status}
        case("standby stops the vCPUs and prints its status; the next exec wakes it with state intact", standby_resize)

        def deleting():
            delete(state["child"])
            delete(a)
            assert get("/sandboxes") == [], get("/sandboxes")
            return {}
        case("delete removes the fork and the original", deleting)

        def disk_sandbox():
            d = create("--disk", "clidata:/data")["sandboxID"]
            sh(d, "echo on-the-disk > /data/first && sync")
            hm("disk", "delete", state["disk"], expect=1)
            delete(d)
            held = hmj("disk", "inspect", state["disk"])
            assert not held.get("attachedTo"), held
            e = create("--disk", "clidata:/data")["sandboxID"]
            assert sh(e, "cat /data/first") == "on-the-disk"
            delete(e)
            hm("disk", "delete", state["disk"])
            assert "clidata" not in json.dumps(hmj("disk", "list"))
            return {}
        case("create --disk: data survives the sandbox, disk delete is refused while held and works when free",
             disk_sandbox)

        def errors():
            x = create()["sandboxID"]
            delete(x)
            out, err = hm("exec", x, "--", "true", expect=1)
            assert "404" in err or "not found" in err.lower(), err
            _, err2 = hm("inspect", x, expect=1)
            assert "404" in err2 or "not found" in err2.lower(), err2
            _, err3 = hm("pause", x, expect=1)
            assert err3.strip(), err3
            _, err4 = hm("create", "--cpus", "0", expect=2)
            assert "cpus" in err4.lower(), err4
            _, err5 = hm("delete", x, expect=1)
            return {"exec_on_deleted_stderr": err.strip()[:200], "inspect_on_deleted_stderr": err2.strip()[:200],
                    "bad_cpus_stderr": err4.strip()[:200], "delete_twice_stderr": err5.strip()[:200]}
        case("errors: exec/inspect/pause/delete on a deleted sandbox exit 1 with the API's message; a bad flag exits 2", errors)

        def none_left():
            left = get("/sandboxes")
            assert left == [] and not owned, (left, owned)
            return {}
        case("no sandbox is left behind", none_left)
    except Exception as error:
        report["fatal"] = f"{type(error).__name__}: {error}"
    finally:
        for sandbox in list(owned):
            try:
                subprocess.run([str(args.hm), "sandbox", "vm", "--endpoint", url, "delete", sandbox],
                               capture_output=True, timeout=60, env=env, stdin=subprocess.DEVNULL)
            except Exception:
                pass
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        text = log_path.read_text(errors="replace")
        kvm, tcg = len(re.findall(r"platform: Kvm", text)), len(re.findall(r"platform: Tcg", text))
        row = {"name": "ran on KVM: the daemon log has platform: Kvm and no platform: Tcg",
               "success": kvm > 0 and tcg == 0, "detail": {"platform_Kvm": kvm, "platform_Tcg": tcg}}
        report["cases"].append(row)
        print(("ok:" if row["success"] else "FAIL:"), row["name"], row["detail"], flush=True)
        report["success"] = "fatal" not in report and all(c["success"] for c in report["cases"])
        report["hm_invocations"] = len(transcript)
        report["transcript"] = transcript
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("report:", args.output / "report.json", "success:", report["success"])


if __name__ == "__main__":
    main()
