#!/usr/bin/env python3
"""eBPF hook shields in a VM guest, end to end on real KVM.

Runs an owned hv2-sandboxd with --shield-dir and --shield-guests (loopback, no cluster
token), its keyring written beforehand from a key made by `hm shield keygen`. Every
signature (shield and directives) is made by the `hm` binary; nothing is signed here.
A sandbox is created, and a shield scoped to it with `hooks: [{"hook":"sb_mount","errno":1}]`
is loaded, enforced and removed through `hm shield`, while the check watches
GET /shields (syscallGuests) and GET /shields/hits and runs `mount` in the guest.

--expect applied: the guest kernel has BPF LSM and BTF.

a. The sandbox starts, and after the shield is loaded its guest is listed under
   syscallGuests with hooks status `applied` (polled: the node sweeps every 5 s).
b. Monitor mode: `mount -t tmpfs none <dir>` in the guest SUCCEEDS, and GET /shields/hits
   then shows an entry for the shield (hook sb_mount, enforced false), and its tally
   counts monitored >= 1.
c. Enforce (signed directive): the same mount FAILS with EPERM (exit != 0),
   hits show an entry with enforced true and the tally counts refused >= 1, and exec itself
   still works (the agent is not judged).
d. Remove (signed directive): the mount succeeds again.

--expect unsupported: a stock kernel without BPF LSM.

a. The sandbox starts and exec works.
b. After the shield is loaded, syscallGuests lists the guest with hooks status
   `unsupported` and a non-empty reason.
c. The mount succeeds in monitor mode, and still succeeds after the shield is enforced
   and after it is removed: nothing is refused, and no hit is enforced.

Writes report.json and the daemon's log into --output; exits nonzero on any failure.
The daemon is the one process this script spawns, and the only one it stops.
"""
import argparse, hashlib, json, os, shutil, signal, socket, subprocess, tempfile, time
import urllib.error, urllib.request
from pathlib import Path

HOOK = "sb_mount"
ERRNO = 1
SPEC_NAME = "check-hook-shield-sb-mount"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for name in ["daemon", "hm", "kernel", "initrd", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--expect", choices=["applied", "unsupported"], required=True)
    parser.add_argument("--label", default="")
    args = parser.parse_args()
    expect = args.expect
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "label": args.label, "expect": expect,
              "purpose": "functional verification of eBPF hook shields in a VM guest, on KVM; no timing claim",
              "input_sha256": {k: digest_of(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest_of(__file__)
    work = Path(tempfile.mkdtemp(prefix="hm-hook-shield-check-", dir="/var/tmp"))
    shield_dir = work / "shields"
    shield_dir.mkdir()
    api = port()
    base = f"http://127.0.0.1:{api}"
    hm_env = dict(os.environ, HOME=str(work), HV2_SANDBOX_URL=base)
    hm_env.pop("HV2_API_KEY", None)

    def hm(*argv):
        done = subprocess.run([str(args.hm), "shield", "--endpoint", base, *argv], env=hm_env,
                              capture_output=True, text=True, timeout=120)
        assert done.returncode == 0, ("hm shield", argv, done.returncode, done.stdout, done.stderr)
        return json.loads(done.stdout)

    # The operator's key, and the keyring the node reads on start.
    key_file = work / "operator.key"
    made = hm("keygen", "--out", str(key_file))
    assert made.get("publicKey"), made
    (shield_dir / "keyring.json").write_text(
        json.dumps({"keys": [{"publicKey": made["publicKey"], "label": "hook-shield-check"}]}))

    log_path = args.output / "daemon-log.txt"
    log = log_path.open("wb")
    rust_log = "info,hv2_sandboxd::guest_shields=debug,hv2_sandboxd::shields=debug"
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--shield-dir", str(shield_dir), "--shield-guests",
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "4"],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG=rust_log),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def request(method, path, body=None, timeout=300):
        req = urllib.request.Request(base + path, method=method, headers={"content-type": "application/json"},
                                     data=None if body is None else json.dumps(body).encode())
        try:
            response = urllib.request.urlopen(req, timeout=timeout)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            data = response.read()
            try:
                return response.status, json.loads(data) if data else None
            except ValueError:
                return response.status, data.decode(errors="replace")

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        detail = action()
        row["success"] = True
        if detail:
            row["detail"] = detail
        print("ok:", name, flush=True)

    def poll(what, probe, seconds=60, step=1.0):
        """Call probe() until it returns something truthy; fail loudly with the last value."""
        deadline = time.monotonic() + seconds
        last = None
        while True:
            assert daemon.poll() is None, "the daemon exited while waiting for " + what
            last = probe()
            if last:
                return last
            assert time.monotonic() < deadline, f"timed out after {seconds}s waiting for {what}"
            time.sleep(step)

    def run(sandbox, cmd):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": 30})
        assert status == 200, ("exec", cmd, status, value)
        return value

    mounts = {"n": 0}

    def try_mount(sandbox):
        """Mount a tmpfs on a fresh directory; returns (succeeded, exit_code, output). Unmounts on success."""
        mounts["n"] += 1
        target = f"/mnt/hook-check-{mounts['n']}"
        made_dir = run(sandbox, f"mkdir -p {target}")
        assert made_dir["exit_code"] == 0, ("mkdir", made_dir)
        done = run(sandbox, f"mount -t tmpfs none {target}")
        output = (done["stdout"] + done["stderr"]).strip()
        ok = done["exit_code"] == 0
        if ok:
            run(sandbox, f"umount {target}")
        return ok, done["exit_code"], output

    def guests():
        status, value = request("GET", "/shields")
        assert status == 200, ("GET /shields", status, value)
        return value["syscallGuests"]

    def hits():
        status, value = request("GET", "/shields/hits")
        assert status == 200, ("GET /shields/hits", status, value)
        return value

    def hook_entries(snapshot, digest, enforced):
        return [h for h in snapshot["hits"]
                if h["digest"] == digest and h["what"] == f"hook {HOOK}" and h["enforced"] is enforced]

    state = {}
    try:
        def ready():
            try:
                return request("GET", "/sandboxes", timeout=5)[0] == 200
            except OSError:
                return False
        poll("the daemon", ready, seconds=240, step=0.3)

        status, made_box = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1800})
        assert status in (200, 201), ("create sandbox", status, made_box)
        sandbox = made_box["sandboxID"]
        state["sandbox"] = sandbox

        # The shield: scoped to this sandbox, one hook rule, expiring in an hour.
        spec_text = json.dumps({"name": SPEC_NAME,
                                "description": "end-to-end check of eBPF hook shields in a guest",
                                "scope": {"kind": "sandboxes", "ids": [sandbox]},
                                "hooks": [{"hook": HOOK, "errno": ERRNO}],
                                "expiresAt": int(time.time()) + 3600})
        spec_file = work / "shield.json"
        spec_file.write_text(spec_text)
        checked = hm("check", str(spec_file))
        digest = checked["digest"]
        assert len(digest) == 64, checked
        state["digest"] = digest

        def hooks_status():
            entry = guests().get(sandbox)
            return entry.get("hooks") if isinstance(entry, dict) else None

        if expect == "applied":
            def started_and_applied():
                before = run(sandbox, "echo alive")
                assert before["exit_code"] == 0 and "alive" in before["stdout"], before
                loaded = hm("load", "--key", str(key_file), str(spec_file))
                assert loaded.get("event") is not None, loaded
                got = poll("guest hooks status applied (the node sweeps every 5 s)",
                           lambda: (lambda h: h if h and h.get("status") in ("applied", "unsupported", "unreachable")
                                    and h.get("status") != "unreachable" else None)(hooks_status()), seconds=90)
                assert got["status"] == "applied", (
                    "the guest did not apply hook rules: is the kernel built with BPF LSM + BTF "
                    "(tools/guest-image/build-kernel.sh --shields) and the agent new enough?", got)
                return {"hooks": got, "digest": digest}
            case("a. the sandbox starts; its guest is listed with hooks status applied", started_and_applied)

            def monitor():
                ok, code, output = poll("a mount in monitor mode that succeeds", lambda: (lambda r: r if r[0] else None)(
                    try_mount(sandbox)), seconds=30)
                snapshot = poll("a monitored hit for the shield", lambda: (lambda s: s if hook_entries(s, digest, False)
                                                                           else (try_mount(sandbox), None)[1])(hits()),
                                seconds=60, step=2)
                entries = hook_entries(snapshot, digest, False)
                tally = snapshot["tally"].get(digest, {})
                assert tally.get("monitored", 0) >= 1 and tally.get("refused", 0) == 0, snapshot["tally"]
                assert entries[0]["sandbox"] == sandbox and entries[0]["shield"] == SPEC_NAME, entries[0]
                return {"mount_exit": code, "entry": entries[0], "tally": tally}
            case("b. monitor mode: the mount succeeds, and a hit is recorded as not enforced", monitor)

            def enforce():
                hm("enforce", "--key", str(key_file), digest)

                def refused():
                    ok, code, output = try_mount(sandbox)
                    return None if ok else (code, output)
                code, output = poll("the mount to be refused after enforcing", refused, seconds=60, step=2)
                # EPERM: libc words it "Operation not permitted"; busybox mount
                # says "permission denied (are you root?)".
                assert code != 0 and ("not permitted" in output.lower()
                                      or "permission denied" in output.lower()), (code, output)
                snapshot = poll("an enforced hit for the shield", lambda: (lambda s: s if hook_entries(s, digest, True)
                                                                           else (try_mount(sandbox), None)[1])(hits()),
                                seconds=60, step=2)
                tally = snapshot["tally"].get(digest, {})
                assert tally.get("refused", 0) >= 1, snapshot["tally"]
                # The agent is not judged: exec still works, and so does a command that mounts nothing.
                alive = run(sandbox, "echo still-alive")
                assert alive["exit_code"] == 0 and "still-alive" in alive["stdout"], alive
                return {"mount_exit": code, "mount_output": output,
                        "entry": hook_entries(snapshot, digest, True)[0], "tally": tally}
            case("c. enforce: the mount is refused, a hit is recorded as enforced, exec still works", enforce)

            def remove():
                hm("remove", "--key", str(key_file), digest)
                ok, code, output = poll("the mount to succeed again after removal",
                                        lambda: (lambda r: r if r[0] else None)(try_mount(sandbox)),
                                        seconds=60, step=2)
                return {"mount_exit": code}
            case("d. remove: the mount succeeds again", remove)
        else:
            def starts():
                alive = run(sandbox, "echo alive")
                assert alive["exit_code"] == 0 and "alive" in alive["stdout"], alive
                return {"exec": alive["stdout"].strip()}
            case("a. the sandbox starts and exec works", starts)

            def unsupported():
                loaded = hm("load", "--key", str(key_file), str(spec_file))
                assert loaded.get("event") is not None, loaded
                got = poll("guest hooks status (the node sweeps every 5 s)",
                           lambda: (lambda h: h if h and h.get("status") != "unreachable" else None)(hooks_status()),
                           seconds=90)
                assert got["status"] == "unsupported", (
                    "expected unsupported on a kernel without BPF LSM; the guest answered", got)
                assert isinstance(got.get("reason"), str) and got["reason"].strip(), got
                return {"hooks": got}
            case("b. hooks status is unsupported, with a reason, after the shield is loaded", unsupported)

            def never_refused():
                detail = {}
                for stage in ["monitor", "enforce", "remove"]:
                    if stage == "enforce":
                        hm("enforce", "--key", str(key_file), digest)
                    elif stage == "remove":
                        hm("remove", "--key", str(key_file), digest)
                    # Let the node push once more (it sweeps every 5 s), then try several times.
                    time.sleep(6)
                    for _ in range(3):
                        ok, code, output = try_mount(sandbox)
                        assert ok, (f"the mount was refused in stage {stage}", code, output)
                    detail[stage] = "mount succeeded x3"
                snapshot = hits()
                assert not [h for h in snapshot["hits"] if h["enforced"]], snapshot["hits"]
                return detail
            case("c. the mount is never refused, in monitor, enforce or after removal", never_refused)
    except AssertionError as error:
        report["error"] = repr(error)
        print("FAIL:", error, flush=True)
    except Exception as error:  # noqa: BLE001 - report anything, then fail
        report["error"] = repr(error)
        print("ERROR:", repr(error), flush=True)
    finally:
        if state.get("sandbox") and daemon.poll() is None:
            try:
                request("DELETE", f"/sandboxes/{state['sandbox']}", timeout=60)
            except Exception:  # noqa: BLE001
                pass
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        text = log_path.read_text(errors="replace")
        report["platform_lines"] = {"Kvm": text.count("Detected hypervisor platform: Kvm"),
                                    "Tcg": text.count("Detected hypervisor platform: Tcg")}
        want = 4 if expect == "applied" else 3
        if all(row["success"] for row in report["cases"]) and len(report["cases"]) == want:
            if report["platform_lines"]["Tcg"] != 0:
                report["error"] = f"the guest ran on TCG, not KVM: {report['platform_lines']}"
                print("FAIL:", report["error"], flush=True)
            else:
                report["success"] = True
        report.update(state)
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        shutil.rmtree(work, ignore_errors=True)
    print("success:", report["success"])
    print("report:", args.output / "report.json")
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
