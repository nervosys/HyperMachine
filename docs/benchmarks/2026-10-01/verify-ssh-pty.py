#!/usr/bin/env python3
"""Verify the PTY run, source provenance and preserved named SSH evidence."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", action="store_true")
    parser.add_argument("--staged", action="store_true")
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[2]
    subprocess.run([sys.executable, str(directory / "verify-ssh-name.py"), *(["--staged"] if args.staged else [])], check=True)
    manifest = json.loads((directory / "ssh-pty-manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        path = directory / name
        assert sha(path) == expected, name
        if args.staged:
            raw = subprocess.check_output(["git", "-c", "safe.directory=" + str(root), "show", ":" + path.relative_to(root).as_posix()], cwd=root)
            assert hashlib.sha256(raw).hexdigest() == expected, name
    report = json.loads((directory / "ssh-pty-e2e.json").read_text())
    base = json.loads((directory / "ssh-name-e2e.json").read_text())
    assert report["success"] and not report["cleanup_errors"]
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 22
    for artifact in ["daemon", "control-plane", "cli", "kernel", "initrd"]:
        assert report["artifact_sha256"][artifact] == base["artifact_sha256"][artifact]
    assert report["artifact_sha256"]["coordinator"] == sha(directory / "ssh-pty-measured-e2e.py")
    assert len(report["cases"]) == 20 and all(row["success"] for row in report["cases"])
    cases = {row["name"]: row["result"] for row in report["cases"]}
    assert len(cases) == 20
    for row in base["cases"]:
        assert cases[row["name"]] == row["result"]
    assert cases["OpenSSH-guest-PTY-and-canonical-input"] == {
        "stdin_is_terminal": True, "stdout_is_terminal": True,
        "canonical_input_verified": True, "terminal_newline": "CRLF"}
    if args.current:
        assert sha(root / "tools/e2e-tcp-tunnel.py") == sha(directory / "ssh-pty-measured-e2e.py")
    print(json.dumps({"success": True, "files": len(manifest["files_sha256"]), "functional_cases": 20, "pty_verified": True}))


if __name__ == "__main__":
    main()
