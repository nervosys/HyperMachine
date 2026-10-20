#!/usr/bin/env python3
"""`hm sandbox vm compose`, a compose file run as detached VMs, on real KVM.

Runs an owned hv2-sandboxd with a guest kit (no Docker anywhere; the node
pulls `alpine:3.20` itself) and drives the real `hm` binary:

1. A file with two services, `db` and `app` (`depends_on: [db]`), `up` exits
   0 and prints both, db first, each with a sandbox ID.
2. Both commands are still running after `up` returned: each wrote its
   /tmp/state, a `sleep 600` is in the process list, and `app` sees its
   environment.
3. `down` deletes both, `list` shows neither, and a second `down` is a
   success with nothing deleted.
4. A dependency cycle, an unknown dependency, `ports:` and `volumes:` each
   fail with a non-zero exit and create no sandbox.
5. `up` twice without `down` refuses the second time and leaves the first
   project alone.
6. A `build:` service builds from a Dockerfile, runs its CMD, and goes away
   with `down`.

Every case takes its project down afterwards, pass or fail.
Writes report.json and the daemon log into --output.
"""
import argparse, hashlib, json, os, re, signal, socket, subprocess, time
import urllib.request
from pathlib import Path

SLEEP = "sleep 600"

VOL_WRITER = """\
services:
  w:
    image: alpine:3.20
    command: ["sh", "-c", "echo persisted > /data/note; sleep 600"]
    volumes: ["data:/data"]
volumes:
  data:
"""

VOL_READER = """\
services:
  r:
    image: alpine:3.20
    command: ["sh", "-c", "sleep 600"]
    volumes: ["data:/data"]
volumes:
  data:
"""

TWO = """\
services:
  app:
    image: alpine:3.20
    depends_on: [db]
    command: ["sh", "-c", "echo app-up > /tmp/state; echo log-app; sleep 600"]
    environment:
      GREETING: hello
  db:
    image: alpine:3.20
    command: ["sh", "-c", "echo db-up > /tmp/state; echo log-db; sleep 600"]
"""


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["daemon", "hm", "kernel", "initrd", "kit", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False, "purpose": "hm sandbox vm compose up/down on KVM",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "hm", "kernel", "initrd"]},
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)

    api = port()
    url = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()), "--guest-kit", str(args.kit),
         "--memory-mb", "512", "--cpu-cores", "1", "--capacity", "8",
         "--volume-dir", str(args.output / "volumes")],
        env=dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info"),
        stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)

    def get(path):
        with urllib.request.urlopen(url + path, timeout=30) as r:
            return r.read().decode()

    def hm(*argv, timeout=1500):
        full = [str(args.hm), "sandbox", "vm", "--endpoint", url, "--request-timeout", "300", *argv]
        env = {k: v for k, v in os.environ.items() if k != "HV2_API_KEY"} | {"HOME": str(args.output)}
        return subprocess.run(full, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL, env=env)

    def case(name, action):
        row = {"name": name, "success": False}
        report["cases"].append(row)
        row["detail"] = action()
        row["success"] = True
        print("ok:", name, flush=True)

    def sandbox_ids():
        """Every sandbox ID `hm sandbox vm list` shows."""
        done = hm("list")
        assert done.returncode == 0, done.stderr[-400:]
        try:
            rows = json.loads(done.stdout)
            return sorted(r["sandboxID"] for r in rows)
        except (ValueError, TypeError, KeyError):
            return sorted(set(re.findall(r'"sandboxID"\s*:\s*"([^"]+)"', done.stdout)))

    def write_compose(name, text):
        d = args.output / name
        d.mkdir()
        (d / "compose.yaml").write_text(text)
        return d / "compose.yaml"

    def up(file, project):
        return hm("compose", "-f", str(file), "-p", project, "up")

    def down(file, project):
        return hm("compose", "-f", str(file), "-p", project, "down")

    def services_of(done):
        out = json.loads(done.stdout)
        return out["services"]

    def exec_in(sid, *command):
        return hm("exec", sid, *command)

    def wait_for(check, what, seconds=10):
        deadline = time.monotonic() + seconds
        while True:
            last = check()
            if last is True:
                return
            if time.monotonic() >= deadline:
                raise AssertionError(f"{what}: {last}")
            time.sleep(0.5)

    try:
        deadline = time.monotonic() + 180
        while True:
            assert daemon.poll() is None, "the daemon exited"
            try:
                get("/sandboxes")
                break
            except OSError:
                assert time.monotonic() < deadline, "daemon readiness"
                time.sleep(0.3)

        two = write_compose("two", TWO)
        started = {}

        def two_services_up():
            done = up(two, "stack")
            assert done.returncode == 0, (done.returncode, done.stdout, done.stderr[-800:])
            services = services_of(done)
            names = [s["service"] for s in services]
            assert names == ["db", "app"], names
            assert all(s.get("sandboxID") for s in services), services
            started.update({s["service"]: s["sandboxID"] for s in services})
            return {"services": services}
        case("up starts db before app, each with a sandbox ID", two_services_up)

        def still_running():
            assert len(started) == 2, "case 1 did not start the services"
            for service, want in [("db", "db-up"), ("app", "app-up")]:
                sid = started[service]

                def state():
                    done = exec_in(sid, "cat", "/tmp/state")
                    return True if done.returncode == 0 and done.stdout.strip() == want else (done.returncode, done.stdout, done.stderr[-200:])
                wait_for(state, f"{service} /tmp/state is {want}")

                def sleeping():
                    done = exec_in(sid, "ps")
                    return True if done.returncode == 0 and SLEEP in done.stdout else (done.returncode, done.stdout[-400:])
                wait_for(sleeping, f"{service} has a {SLEEP!r} process")
            greeting = exec_in(started["app"], "printenv", "GREETING")
            assert greeting.returncode == 0 and greeting.stdout.strip() == "hello", greeting
            return {"app_greeting": greeting.stdout.strip()}
        case("the commands are still running after up returned, and app has its environment", still_running)

        def volume_survives_down():
            writer = write_compose("vol-writer", VOL_WRITER)
            reader = write_compose("vol-reader", VOL_READER)
            done = up(writer, "vol")
            assert done.returncode == 0, (done.returncode, done.stdout, done.stderr[-800:])
            sid = services_of(done)[0]["sandboxID"]

            def note():
                out = exec_in(sid, "cat", "/data/note")
                return True if out.returncode == 0 and out.stdout.strip() == "persisted" else (out.returncode, out.stdout, out.stderr[-200:])
            wait_for(note, "the writer's file is in its volume")
            mounts = exec_in(sid, "sh", "-c", "grep ' /data ' /proc/mounts")
            assert mounts.returncode == 0 and "9p" in mounts.stdout, mounts
            assert down(writer, "vol").returncode == 0
            volumes = json.loads(hm("volume", "list").stdout)
            ours = [v for v in volumes if v["name"] == "vol-data"]
            assert len(ours) == 1, volumes
            try:
                done = up(reader, "vol")
                assert done.returncode == 0, (done.returncode, done.stdout, done.stderr[-800:])
                sid = services_of(done)[0]["sandboxID"]

                def again():
                    out = exec_in(sid, "cat", "/data/note")
                    return True if out.returncode == 0 and out.stdout.strip() == "persisted" else (out.returncode, out.stdout, out.stderr[-200:])
                wait_for(again, "a later project run sees the file")
                assert down(reader, "vol").returncode == 0
                return {"mount": mounts.stdout.strip(), "volume": ours[0]["name"]}
            finally:
                down(reader, "vol")
                hm("volume", "delete", ours[0]["volumeID"])
        case("a named volume is mounted over 9p, outlives down, and the next up sees its data", volume_survives_down)

        def logs_show_each_service():
            assert len(started) == 2, "case 1 did not start the services"

            def logs(*extra):
                done = hm("compose", "-f", str(two), "-p", "stack", "logs", *extra)
                assert done.returncode == 0, (done.returncode, done.stdout, done.stderr[-400:])
                return {row["service"]: row["log"] for row in json.loads(done.stdout)["logs"]}
            seen = {}

            def both():
                seen.update(logs())
                ok = "log-app" in seen.get("app", "") and "log-db" in seen.get("db", "")
                return True if ok else seen
            wait_for(both, "compose logs shows each service's output")
            assert "log-db" not in seen["app"] and "log-app" not in seen["db"], seen
            only = logs("db")
            assert list(only) == ["db"] and "log-db" in only["db"], only
            missing = hm("compose", "-f", str(two), "-p", "stack", "logs", "ghost")
            assert missing.returncode != 0 and "ghost" in missing.stderr, missing.stderr[-300:]
            return {"app": seen["app"].strip(), "db": seen["db"].strip()}
        case("logs shows each service's own output, one service on request, and refuses an unknown one", logs_show_each_service)

        def down_removes():
            before = sandbox_ids()
            assert all(sid in before for sid in started.values()), (before, started)
            done = down(two, "stack")
            assert done.returncode == 0, (done.returncode, done.stderr[-800:])
            deleted = sorted(d["sandboxID"] for d in json.loads(done.stdout)["deleted"])
            assert deleted == sorted(started.values()), (deleted, started)
            after = sandbox_ids()
            assert not any(sid in after for sid in started.values()), after
            again = down(two, "stack")
            assert again.returncode == 0, (again.returncode, again.stderr[-400:])
            assert json.loads(again.stdout)["deleted"] == [], again.stdout
            return {"deleted": deleted}
        case("down deletes both, list shows neither, and a second down deletes nothing", down_removes)

        def refused_files():
            base = "services:\n"
            files = {
                "cycle": base + "  a:\n    image: alpine:3.20\n    depends_on: [b]\n  b:\n    image: alpine:3.20\n    depends_on: [a]\n",
                "unknown": base + "  a:\n    image: alpine:3.20\n    depends_on: [ghost]\n",
                "ports": base + "  a:\n    image: alpine:3.20\n    ports: [\"8000-8010:80\"]\n",
                "volumes": base + "  a:\n    image: alpine:3.20\n    volumes: [\"./d:/d\"]\n",
            }
            before = sandbox_ids()
            rows = {}
            for label, text in files.items():
                path = write_compose("bad-" + label, text)
                project = "bad" + label
                try:
                    done = up(path, project)
                    assert done.returncode != 0, (label, done.stdout)
                    rows[label] = done.stderr.strip().splitlines()[-1][:200] if done.stderr.strip() else ""
                finally:
                    down(path, project)
                assert sandbox_ids() == before, (label, sandbox_ids(), before)
            return rows
        case("a cycle, an unknown dependency, a port range and volumes fail and create no sandbox", refused_files)

        def twice():
            file = write_compose("twice", TWO)
            try:
                first = up(file, "twice")
                assert first.returncode == 0, (first.returncode, first.stderr[-800:])
                ids = sorted(s["sandboxID"] for s in services_of(first))
                second = up(file, "twice")
                assert second.returncode != 0, second.stdout
                assert "already has" in second.stderr, second.stderr[-400:]
                now = sandbox_ids()
                assert all(sid in now for sid in ids), (ids, now)
                state = exec_in(ids[0], "ps")
                assert state.returncode == 0 and SLEEP in state.stdout, state
                return {"ids": ids, "refusal": second.stderr.strip().splitlines()[-1][:200]}
            finally:
                down(file, "twice")
        case("a second up without down is refused and leaves the first project alone", twice)

        def built():
            d = args.output / "built"
            (d / "app").mkdir(parents=True)
            (d / "app" / "Dockerfile").write_text(
                'FROM alpine:3.20\nCMD ["sh","-c","echo built > /tmp/state; sleep 600"]\n')
            file = d / "compose.yaml"
            file.write_text("services:\n  web:\n    build: ./app\n")
            try:
                done = up(file, "built")
                assert done.returncode == 0, (done.returncode, done.stderr[-800:])
                [service] = services_of(done)
                sid = service["sandboxID"]

                def state():
                    r = exec_in(sid, "cat", "/tmp/state")
                    return True if r.returncode == 0 and r.stdout.strip() == "built" else (r.returncode, r.stdout, r.stderr[-200:])
                wait_for(state, "built service /tmp/state is built")
                removed = down(file, "built")
                assert removed.returncode == 0, removed.stderr[-400:]
                assert sid not in sandbox_ids()
                return {"service": service}
            finally:
                down(file, "built")
        case("a build: service builds, runs its CMD, and goes away with down", built)

        report["success"] = True
    finally:
        for name, project in [("two", "stack"), ("twice", "twice"), ("built", "built")]:
            try:
                path = args.output / name / "compose.yaml"
                if path.exists() and daemon.poll() is None:
                    down(path, project)
            except Exception:
                pass
        if daemon.poll() is None:
            daemon.send_signal(signal.SIGTERM)
            try:
                daemon.wait(timeout=60)
            except subprocess.TimeoutExpired:
                daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("report:", args.output / "report.json")


if __name__ == "__main__":
    main()
