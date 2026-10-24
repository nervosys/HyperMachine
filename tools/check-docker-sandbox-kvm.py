#!/usr/bin/env python3
"""Docker's engine running inside a sandbox, on real KVM.

Runs an owned hv2-sandboxd whose `base` template is the initramfs that
tools/guest-image/build-tools-image.sh --docker makes (Ubuntu 24.04, systemd,
Docker's static binaries) on a kernel built with build-kernel.sh --docker, and
checks through its API:

1. dockerd is up under systemd and `docker info` names its storage driver and
   cgroup version.
2. A container runs from an image imported from a tarball, with no network.
3. A container's memory limit reaches its cgroup (`--memory 64m`).
4. Two containers on a user-defined bridge reach each other (veth, bridge).
5. With --network: `docker run hello-world` pulls from Docker Hub and runs.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, http.client, json, os, socket, subprocess, time
import urllib.error, urllib.request
from pathlib import Path

DOCKERFILE = """FROM scratch
COPY busybox /busybox
RUN ["/busybox", "sh", "-c", "echo built-by-docker > /msg"]
CMD ["/busybox", "cat", "/msg"]
"""


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
    parser.add_argument("--memory-mb", type=int, default=2048)
    parser.add_argument("--network", action="store_true", help="also pull hello-world (needs the Internet)")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification that Docker runs inside a sandbox on KVM; "
                         "the times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "initrd_bytes": args.initrd.stat().st_size, "memory_mb": args.memory_mb,
              "host_cpus": os.cpu_count(), "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    proxy = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(proxy),
         "--memory-mb", str(args.memory_mb), "--cpu-cores", "2", "--capacity", "4"] + (["--network", "--egress-default", "allow"] if args.network else []),
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

    def sh(sandbox, cmd, within=120, ok=True):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        if ok:
            assert value["exit_code"] == 0, (cmd, value)
        return value["stdout"].strip()

    def create():
        began = time.monotonic()
        status, made = request("POST", "/sandboxes", {"templateID": "base", "timeout": 1200})
        assert status in (200, 201), (status, made)
        return made["sandboxID"], round(time.monotonic() - began, 3)

    def settled(sandbox):
        """systemd's own verdict, once it has one."""
        deadline = time.monotonic() + 90
        while True:
            said = sh(sandbox, "systemctl is-system-running --wait 2>&1; true", 120, ok=False)
            if said and "Failed to connect" not in said:
                return said.splitlines()[-1]
            if time.monotonic() > deadline:
                why = sh(sandbox, "ps -e -o pid,comm | head -30; echo ---; ls -la /run/systemd 2>&1 | head -20; echo ---; "
                                  "journalctl -b --no-pager -o cat 2>&1 | tail -25; echo ---; dmesg 2>&1 | tail -10",
                         30, ok=False)
                raise AssertionError("systemd never answered: " + repr(said) + "\n" + why)
            time.sleep(1)

    try:
        deadline = time.monotonic() + 240
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

        def engine():
            sandbox, took = create()
            state["sandbox"], state["create_seconds"] = sandbox, took
            deadline = time.monotonic() + 120
            while sh(sandbox, "docker info >/dev/null 2>&1 && echo up; true", 30, ok=False) != "up":
                assert time.monotonic() < deadline, sh(sandbox, "journalctl -u docker --no-pager -o cat | tail -30", 30, ok=False)
                time.sleep(1)
            info = sh(sandbox, "docker info --format '{{.Driver}} cgroup{{.CgroupVersion}} {{.ServerVersion}} {{.SecurityOptions}}'")
            return {"create_seconds": took, "docker_info": info,
                    "kernel": sh(sandbox, "uname -r")}
        case("dockerd is up under systemd", engine)

        def offline():
            sandbox = state["sandbox"]
            sh(sandbox, "rm -rf /tmp/r && mkdir -p /tmp/r/bin && cp /bin/busybox /tmp/r/bin/busybox "
                        "&& tar -C /tmp/r -c . | docker import - hvbox")
            out = sh(sandbox, "docker run --rm --network none hvbox /bin/busybox echo from-a-container", 120)
            assert out == "from-a-container", out
            return {"output": out}
        case("a container runs from an imported image, with no network", offline)

        def limited():
            sandbox = state["sandbox"]
            out = sh(sandbox, "docker run --rm --network none --memory 64m hvbox /bin/busybox cat /sys/fs/cgroup/memory.max", 120)
            assert out == "67108864", out
            return {"memory.max": out}
        case("a memory limit reaches the container's cgroup", limited)

        def bridged():
            sandbox = state["sandbox"]
            sh(sandbox, "docker network create hvnet >/dev/null && "
                        "docker run -d --rm --name srv --network hvnet hvbox /bin/busybox sh -c "
                        "'echo hello-over-bridge | /bin/busybox nc -l -p 9000'")
            time.sleep(1)
            out = sh(sandbox, "docker run --rm --network hvnet hvbox /bin/busybox nc srv 9000", 60)
            assert out == "hello-over-bridge", out
            return {"output": out}
        case("two containers on a bridge network reach each other", bridged)

        def built():
            sandbox = state["sandbox"]
            sh(sandbox, "rm -rf /tmp/b && mkdir /tmp/b && cp /bin/busybox /tmp/b/busybox && cat > /tmp/b/Dockerfile <<'EOF'" + chr(10) + DOCKERFILE + "EOF")
            built_out = sh(sandbox, "docker build -q -t hvbuilt /tmp/b 2>&1 | tail -3", 300, ok=False)
            out = sh(sandbox, "docker run --rm --network none hvbuilt", 120)
            assert out == "built-by-docker", (out, built_out)
            return {"output": out}
        case("docker build runs a Dockerfile's RUN step and the image runs", built)

        def published():
            sandbox = state["sandbox"]
            sh(sandbox, "printf 'HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhi-publish' > /tmp/resp")
            sh(sandbox, "docker run -d --rm --name web -p 8080:9000 -v /tmp/resp:/resp:ro hvbox /bin/busybox sh -c "
                        "'while true; do /bin/busybox nc -l -p 9000 < /resp; done'")
            time.sleep(1)
            inside = sh(sandbox, "/bin/busybox nc 127.0.0.1 8080 </dev/null | /bin/busybox tail -n 1", 60)
            assert inside == "hi-publish", inside
            conn = http.client.HTTPConnection("127.0.0.1", proxy, timeout=30)
            try:
                conn.request("GET", "/", headers={"Host": f"8080-{sandbox}.localhost"})
                response = conn.getresponse()
                outside = (response.status, response.read().decode(errors="replace").strip())
            finally:
                conn.close()
            assert outside == (200, "hi-publish"), outside
            rule = sh(sandbox, "iptables -t nat -S DOCKER | grep -c 'dport 8080'", ok=False)
            sh(sandbox, "docker rm -f web >/dev/null; true", 60, ok=False)
            return {"from_the_guest": inside, "through_the_node_proxy": list(outside), "dnat_rules_for_8080": rule}
        case("a published port reaches the container, from the guest and through the node proxy", published)

        def resumed():
            sandbox = state["sandbox"]
            # /tmp/resp is the HTTP response the previous case wrote.
            sh(sandbox, "docker run -d --rm --name keep -p 8081:9000 -v /tmp/resp:/resp:ro hvbox /bin/busybox sh -c "
                        "'while true; do /bin/busybox nc -l -p 9000 < /resp; done'")
            sh(sandbox, "echo before > /root/mark")
            time.sleep(1)

            def serves(target):
                return sh(target, "/bin/busybox nc 127.0.0.1 8081 </dev/null | /bin/busybox tail -n 1", 60, ok=False)
            assert serves(sandbox) == "hi-publish"
            status, made = request("POST", f"/sandboxes/{sandbox}/checkpoints", {"name": "with-container"})
            assert status in (200, 201), (status, made)
            sh(sandbox, "docker rm -f keep >/dev/null; echo after > /root/mark")
            assert sh(sandbox, "docker ps -q | wc -l") == "0"
            status, back = request("POST", f"/sandboxes/{sandbox}/checkpoints/with-container/restore")
            assert status in (200, 201, 204), (status, back)
            restored = {"running": sh(sandbox, "docker ps --format '{{.Names}}'"), "mark": sh(sandbox, "cat /root/mark"),
                        "serves": serves(sandbox)}
            assert restored == {"running": "keep", "mark": "before", "serves": "hi-publish"}, restored
            status, made = request("POST", f"/sandboxes/{sandbox}/fork", {"timeout": 600})
            assert status in (200, 201), (status, made)
            child = made[0]["sandbox"]["sandboxID"]
            try:
                forked = {"running": sh(child, "docker ps --format '{{.Names}}'"), "serves": serves(child),
                          "can_run_another": sh(child, "docker run --rm --network none hvbox /bin/busybox echo ok", 120)}
            finally:
                request("DELETE", f"/sandboxes/{child}")
            assert forked == {"running": "keep", "serves": "hi-publish", "can_run_another": "ok"}, forked
            sh(sandbox, "docker rm -f keep >/dev/null; true", 60, ok=False)
            return {"restored": restored, "forked": forked}
        case("a checkpoint and a fork carry dockerd and a running container", resumed)

        if args.network:
            def pulled():
                sandbox = state["sandbox"]
                out = sh(sandbox, "docker run --rm hello-world 2>&1 | grep -m1 'Hello from Docker'; true", 300)
                assert "Hello from Docker" in out, out
                return {"first_line": out.splitlines()[0]}
            case("an image is pulled from Docker Hub and runs", pulled)

        request("DELETE", f"/sandboxes/{state['sandbox']}")
        report["success"] = True
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=20)
        except subprocess.TimeoutExpired:
            daemon.kill()
        log.close()
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print("success" if report["success"] else "FAILED", flush=True)
    raise SystemExit(0 if report["success"] else 1)


if __name__ == "__main__":
    main()
