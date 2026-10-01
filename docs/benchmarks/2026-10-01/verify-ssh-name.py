#!/usr/bin/env python3
"""Verify real named-SSH evidence and preserved base SSH archive."""
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
    subprocess.run([sys.executable, str(directory / "verify-ssh.py"), *(["--staged"] if args.staged else [])], check=True)
    manifest = json.loads((directory / "ssh-name-manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        path = directory / name
        assert sha(path) == expected, name
        if args.staged:
            raw = subprocess.check_output(["git", "-c", "safe.directory=" + str(root), "show", ":" + path.relative_to(root).as_posix()], cwd=root)
            assert hashlib.sha256(raw).hexdigest() == expected, name
    build = json.loads((directory / "ssh-name-cli-build.json").read_text())
    report = json.loads((directory / "ssh-name-e2e.json").read_text())
    assert build["success"] and build["exit_code"] == 0
    assert hashlib.sha256(build["output_utf8"].encode()).hexdigest() == build["output_sha256"]
    assert build["coordinator_sha256"] == sha(directory / "ssh-name-measured-cli-builder.py")
    for source, expected in build["source_sha256"].items():
        frozen = manifest["source_snapshots"][source]
        assert sha(directory / frozen) == expected, source
    assert report["success"] and report["artifact_sha256"]["cli"] == build["cli_sha256"]
    assert report["artifact_sha256"]["coordinator"] == sha(directory / "ssh-name-measured-e2e.py")
    base = json.loads((directory / "ssh-e2e.json").read_text())
    for name in ["daemon", "control-plane", "kernel", "initrd"]:
        assert report["artifact_sha256"][name] == base["artifact_sha256"][name]
    assert report["ssh_fixture"]["resolution"] == "hm.name metadata"
    assert not report["cleanup_errors"] and report["remaining_sandboxes"] == 0
    assert len(report["owned_processes_stopped"]) == 22
    assert len(report["cases"]) == 19 and all(row["success"] for row in report["cases"])
    cases = {row["name"]: row["result"] for row in report["cases"]}
    assert set(cases) == {row["name"] for row in base["cases"]}
    for row in base["cases"]:
        for field, value in row["result"].items() if isinstance(row["result"], dict) else []:
            assert cases[row["name"]][field] == value
    fork = cases["fork-preserves-guest-listener"]
    assert fork["fork_duplicate_name_rejected"] and fork["name_resolves_after_duplicate_deleted"]
    if args.current:
        for source, frozen in manifest["current_sources"].items():
            assert sha(root / source) == sha(directory / frozen), source
    print(json.dumps({"success": True, "files": len(manifest["files_sha256"]), "functional_cases": 19, "named_ssh": True}))


if __name__ == "__main__":
    main()
