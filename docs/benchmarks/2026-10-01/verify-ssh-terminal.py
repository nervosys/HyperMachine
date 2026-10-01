#!/usr/bin/env python3
"""Verify terminal control events and preserved SSH/TCP provenance."""
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
    subprocess.run([sys.executable, str(directory / "verify-ssh-pty.py"), *(["--staged"] if args.staged else [])], check=True)
    manifest = json.loads((directory / "ssh-terminal-manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        path = directory / name
        assert sha(path) == expected, name
        if args.staged:
            raw = subprocess.check_output(["git", "-c", "safe.directory=" + str(root), "show", ":" + path.relative_to(root).as_posix()], cwd=root)
            assert hashlib.sha256(raw).hexdigest() == expected, name
    report = json.loads((directory / "ssh-terminal-e2e.json").read_text())
    base = json.loads((directory / "ssh-pty-e2e.json").read_text())
    assert report["success"] and not report["cleanup_errors"]
    assert report["remaining_sandboxes"] == 0 and len(report["owned_processes_stopped"]) == 23
    assert {row["name"]: row["exit_code"] for row in report["owned_processes_stopped"]}["ssh-terminal"] == 0
    for name in ["daemon", "control-plane", "cli", "kernel", "initrd"]:
        assert report["artifact_sha256"][name] == base["artifact_sha256"][name]
    assert report["artifact_sha256"]["coordinator"] == sha(directory / "ssh-terminal-measured-e2e.py")
    assert len(report["cases"]) == 21 and all(row["success"] for row in report["cases"])
    cases = {row["name"]: row["result"] for row in report["cases"]}
    assert len(cases) == 21
    for row in base["cases"]:
        assert cases[row["name"]] == row["result"]
    terminal = cases["OpenSSH-terminal-size-SIGWINCH-and-Ctrl-C"]
    for field, expected in {"initial_rows": 24, "initial_columns": 80, "resized_rows": 43,
                            "resized_columns": 132, "guest_SIGWINCH": True,
                            "guest_SIGINT": True, "exit_code": 0}.items():
        assert terminal[field] == expected, field
    transcript = terminal["output_utf8"]
    markers = ["terminal-initial:24 80\r\n", "terminal-resized:43 132\r\n", "terminal-interrupt-ok\r\n"]
    positions = [transcript.index(marker) for marker in markers]
    assert positions == sorted(positions)
    assert len(transcript.encode()) <= 128 * 1024
    if args.current:
        assert sha(root / "tools/e2e-tcp-tunnel.py") == sha(directory / "ssh-terminal-measured-e2e.py")
    print(json.dumps({"success": True, "files": len(manifest["files_sha256"]), "functional_cases": 21, "terminal_events_verified": True}))


if __name__ == "__main__":
    main()
