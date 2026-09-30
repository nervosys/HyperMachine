#!/usr/bin/env python3
"""Check sandboxd's strict startup policy using a deliberately missing initrd.

HV2_KERNEL and HV2_INITRD must be set. Optional --check-ready also boots the
valid image and proves strict startup can serve a snapshot-backed template.
"""

import argparse
import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    parser.add_argument("--port", type=int, default=13997)
    parser.add_argument("--check-ready", action="store_true")
    args = parser.parse_args()
    for name in ("HV2_KERNEL", "HV2_INITRD"):
        if not os.environ.get(name):
            parser.error(f"{name} must be set")
    for port in (args.port, args.port + 1):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", port))
    command = [args.binary, "--port", str(args.port), "--proxy-port", str(args.port + 1)]
    conflict = subprocess.run(command + ["--require-template", "--no-template"],
                              capture_output=True, text=True, timeout=15)
    assert conflict.returncode != 0 and "conflicts" in conflict.stderr, conflict.stderr
    with tempfile.TemporaryDirectory(prefix="hm-template-policy-") as directory:
        missing = os.environ.copy()
        missing["HV2_INITRD"] = os.path.join(directory, "missing-initrd.cpio.gz")
        strict = subprocess.run(command + ["--require-template"], env=missing,
                                capture_output=True, text=True, timeout=30)
        assert strict.returncode != 0 and "required template base failed" in strict.stderr, strict.stderr

        def serve(environment, strict_mode):
            with tempfile.TemporaryFile(mode="w+") as log:
                daemon = subprocess.Popen(command + (["--require-template"] if strict_mode else []),
                                          env=environment, stdout=log, stderr=log)
                try:
                    deadline = time.monotonic() + (150 if strict_mode else 30)
                    while time.monotonic() < deadline and daemon.poll() is None:
                        try:
                            with urllib.request.urlopen(f"http://127.0.0.1:{args.port}/templates", timeout=1) as response:
                                templates = json.load(response)
                            assert templates[0]["snapshot"] is strict_mode, templates
                            return
                        except OSError:
                            time.sleep(0.1)
                    log.seek(0)
                    raise AssertionError("daemon did not serve expected template mode: " + log.read())
                finally:
                    if daemon.poll() is None:
                        daemon.terminate()
                        try:
                            daemon.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            daemon.kill()
                            daemon.wait(timeout=10)

        serve(missing, False)
        if args.check_ready:
            serve(os.environ.copy(), True)
    print("PASS: conflicting options rejected; strict failure exits; default cold-boot fallback preserved"
          + ("; strict snapshot-backed startup verified" if args.check_ready else ""))


if __name__ == "__main__":
    main()
