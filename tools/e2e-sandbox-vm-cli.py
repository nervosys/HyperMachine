#!/usr/bin/env python3
"""Verify the shipped VM CLI against a running sandboxd with real KVM guests."""

import argparse
import json
import os
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--endpoint", required=True)
    args = parser.parse_args()
    owned = set()

    def run(*command, expected=0):
        result = subprocess.run(
            [args.binary, "sandbox", "vm", "--endpoint", args.endpoint, *command],
            capture_output=True, text=True, timeout=150, env=os.environ.copy(),
        )
        if result.returncode != expected:
            raise AssertionError(
                f"{command[0]} exited {result.returncode}, expected {expected}: "
                f"{result.stdout}\n{result.stderr}"
            )
        return result.stdout

    def execute(sandbox, *command, expected=0):
        return run("exec", sandbox, "--", *command, expected=expected)

    try:
        created = json.loads(run("create", "--lifetime", "600"))
        sandbox = created["sandboxID"]
        owned.add(sandbox)
        literal = "a'b; $(touch /root/cli-injection); `echo substituted`"
        assert execute(sandbox, "/bin/busybox", "printf", "%s", literal) == literal
        assert execute(sandbox, "/bin/sh", "-c", "test ! -e /root/cli-injection") == ""
        assert execute(sandbox, "/bin/sh", "-c", "printf guest-failure; exit 7", expected=7) == "guest-failure"
        execute(sandbox, "/bin/sh", "-c", "printf before > /root/cli-state")
        run("checkpoint", "save", sandbox, "before")
        assert any(c["name"] == "before" for c in json.loads(run("checkpoint", "list", sandbox)))
        execute(sandbox, "/bin/sh", "-c", "printf after > /root/cli-state")
        run("checkpoint", "restore", sandbox, "before")
        assert execute(sandbox, "/bin/busybox", "cat", "/root/cli-state") == "before"
        run("checkpoint", "delete", sandbox, "before")
        run("pause", sandbox)
        assert json.loads(run("inspect", sandbox))["state"] == "paused"
        run("resume", sandbox, "--lifetime", "600")
        assert execute(sandbox, "/bin/busybox", "cat", "/root/cli-state") == "before"
        children = json.loads(run("fork", sandbox, "--count", "2", "--lifetime", "600"))
        # Track every returned child before asserting its state, so failures clean up.
        for result in children:
            if "sandbox" in result:
                owned.add(result["sandbox"]["sandboxID"])
        assert len(children) == 2
        for result in children:
            assert "sandbox" in result, result
            child = result["sandbox"]
            assert execute(child["sandboxID"], "/bin/busybox", "cat", "/root/cli-state") == "before"
        listed = json.loads(run("list"))
        assert owned.issubset({s["sandboxID"] for s in listed})
        print("PASS: guest argument quoting, exit status, checkpoints, pause/resume, fork and list")
    finally:
        failures = []
        for sandbox in owned:
            try:
                run("delete", sandbox)
            except Exception as error:
                failures.append(str(error))
        if failures:
            raise RuntimeError("cleanup failed: " + "; ".join(failures))


if __name__ == "__main__":
    main()
