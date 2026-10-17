#!/usr/bin/env python3
"""A sandbox from an image with the development tools already in it, on real KVM.

Runs an owned hv2-sandboxd whose `base` template is the initramfs that
tools/guest-image/build-tools-image.sh makes (Ubuntu 24.04 with git, Python,
Node.js, npm, gcc, make, curl, jq), and checks through its API:

1. The guest is Ubuntu 24.04 and every tool is there and answers.
2. Work with them, with no network and nothing installed: a Python program, a
   C program compiled and run, a Node.js test run by `node --test`, and a git
   repository with a commit.
3. A sandbox from it checkpoints and a fork carries the files and the tools.
4. The cost: how long a create takes, and what the guest has free.

Writes report.json and the daemon's log into --output.
"""
import argparse, hashlib, json, os, socket, subprocess, tempfile, time
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
    parser.add_argument("--memory-mb", type=int, default=2048)
    parser.add_argument("--network", action="store_true",
                        help="start the daemon with --network --egress-default allow and also check "
                             "git clone, npm install, curl and apt from inside (needs the Internet)")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"success": False,
              "purpose": "functional verification that a template holding the development tools runs them "
                         "in a sandbox on KVM; the times are the check's own stopwatch and not a benchmark",
              "input_sha256": {k: digest(getattr(args, k)) for k in ["daemon", "kernel", "initrd"]},
              "initrd_bytes": args.initrd.stat().st_size,
              "memory_mb": args.memory_mb,
              "network": args.network,
              "host_cpus": os.cpu_count(),
              "cases": []}
    report["input_sha256"]["driver"] = digest(__file__)
    env = dict(os.environ, HV2_KERNEL=str(args.kernel), HV2_INITRD=str(args.initrd), RUST_LOG="info")
    api = port()
    base = f"http://127.0.0.1:{api}"
    log = (args.output / "daemon.log").open("wb")
    daemon = subprocess.Popen(
        [str(args.daemon), "--port", str(api), "--proxy-port", str(port()),
         "--memory-mb", str(args.memory_mb), "--cpu-cores", "2", "--capacity", "4"]
        + (["--network", "--egress-default", "allow"] if args.network else []),
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

    def sh(sandbox, cmd, within=120):
        status, value = request("POST", f"/sandboxes/{sandbox}/exec", {"cmd": cmd, "timeout_secs": within})
        assert status == 200, (status, value)
        return value["stdout"].strip()

    def create(**body):
        began = time.monotonic()
        status, made = request("POST", "/sandboxes", dict({"templateID": "base", "timeout": 1200}, **body))
        assert status in (200, 201), (status, made)
        return made["sandboxID"], round(time.monotonic() - began, 3)

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

        def tools_there():
            sandbox, took = create()
            state["sandbox"] = sandbox
            release = sh(sandbox, "grep -E '^(NAME|VERSION_ID)=' /etc/os-release")
            assert 'NAME="Ubuntu"' in release and 'VERSION_ID="24.04"' in release, release
            versions = {}
            for tool, cmd in [("git", "git --version"), ("python3", "python3 --version"),
                              ("node", "node --version"), ("npm", "npm --version"),
                              ("gcc", "gcc --version | head -1"), ("make", "make --version | head -1"),
                              ("curl", "curl --version | head -1"), ("jq", "jq --version")]:
                out = sh(sandbox, cmd + " 2>&1")
                assert out and "not found" not in out, (tool, out)
                versions[tool] = out
            return {"create_seconds": took, "os": release.replace("\n", " "), "versions": versions}
        case("an Ubuntu 24.04 guest with git, Python, Node.js, npm, gcc, make, curl and jq", tools_there)

        def work():
            sandbox = state["sandbox"]
            py = sh(sandbox, "python3 -c 'import json,sys; print(sum(range(101)), json.dumps({\"a\": 1}))'")
            assert py == '5050 {"a": 1}', py
            c = sh(sandbox, "cd /tmp && cat > t.c <<'EOF'\n#include <stdio.h>\nint main(void) { printf(\"%d\\n\", 6 * 7); return 0; }\nEOF\n"
                            "gcc -O2 -o t t.c && ./t")
            assert c == "42", c
            node = sh(sandbox,
                      "mkdir -p /tmp/n && cd /tmp/n && printf \"const t=require('node:test');const a=require('node:assert');"
                      "t('adds',()=>a.strictEqual(1+2,3));t('joins',()=>a.strictEqual(['a','b'].join('-'),'a-b'));\\n\" > t.test.js"
                      " && node --test 2>&1 | grep -E '^# (tests|pass|fail)'")
            assert "# tests 2" in node and "# pass 2" in node and "# fail 0" in node, node
            git = sh(sandbox, "mkdir -p /tmp/r && cd /tmp/r && git init -q . && echo hi > f"
                              " && git add f && git -c user.name=t -c user.email=t@t commit -q -m first"
                              " && git log --oneline | wc -l")
            assert git == "1", git
            return {"python": py, "c": c, "node_test": node.replace("\n", " "), "git_commits": git}
        case("work with the tools, offline: Python, a C build, node --test, a git commit", work)

        def internet():
            sandbox = state["sandbox"]
            seen = {}
            head = sh(sandbox, "curl -sS -m 30 -o /dev/null -w '%{http_code}' https://archive.ubuntu.com/ubuntu/dists/noble/Release")
            assert head == "200", head
            seen["curl_https_status"] = head
            clone = sh(sandbox, "cd /tmp && rm -rf hw && git clone -q --depth 1 https://github.com/octocat/Hello-World.git hw"
                                " && git -C hw log --oneline | wc -l && ls hw", 180)
            assert clone.splitlines()[0] == "1" and "README" in clone, clone
            seen["git_clone"] = clone.replace("\n", " ")
            npm = sh(sandbox, "mkdir -p /tmp/np && cd /tmp/np && npm init -y >/dev/null 2>&1"
                              " && npm install --no-audit --no-fund is-odd@3.0.1 2>&1 | tail -2"
                              " && node -e \"console.log(require('is-odd')(3))\"", 240)
            assert npm.splitlines()[-1] == "true", npm
            seen["npm_install"] = npm.replace("\n", " ")
            apt = sh(sandbox, "apt-get update -qq 2>&1 | tail -2; DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tree 2>&1 | tail -2;"
                              " tree --version", 300)
            assert apt.splitlines()[-1].startswith("tree v"), apt
            seen["apt_install"] = apt.splitlines()[-1]
            # Writes outside /tmp: the root is a tmpfs, and was once full on arrival.
            root = sh(sandbox, "echo kept > /root/note && cat /root/note; mkdir -p /home/u && echo ok > /home/u/f && cat /home/u/f; df -k / | tail -1")
            assert root.splitlines()[:2] == ["kept", "ok"], root
            seen["root_filesystem"] = root.splitlines()[-1]
            return seen
        if args.network:
            case("from inside, with the network on: curl over HTTPS, git clone, npm install, apt install", internet)

        def forked():
            sandbox = state["sandbox"]
            sh(sandbox, "echo kept > /tmp/marker")
            status, made = request("POST", f"/sandboxes/{sandbox}/fork", {"timeout": 600})
            assert status in (200, 201), (status, made)
            child = made[0]["sandbox"]["sandboxID"]
            out = sh(child, "cat /tmp/marker; cd /tmp/r && git log --oneline | wc -l; node --version")
            assert out.splitlines()[0] == "kept" and out.splitlines()[1] == "1", out
            request("DELETE", f"/sandboxes/{child}")
            return {"fork_output": out.replace("\n", " ")}
        case("a fork carries the files and the tools", forked)

        def cost():
            sandbox = state["sandbox"]
            free = sh(sandbox, "grep -E '^(MemTotal|MemAvailable):' /proc/meminfo")
            second, took = create()
            request("DELETE", f"/sandboxes/{second}")
            return {"meminfo_kb": free.replace("\n", " "), "second_create_seconds": took,
                    "initramfs_bytes": args.initrd.stat().st_size}
        case("what it costs: create time and free memory", cost)

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
