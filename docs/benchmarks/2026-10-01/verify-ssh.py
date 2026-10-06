#!/usr/bin/env python3
"""Verify frozen SSH functional evidence, optional current tools/staged bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", action="store_true")
    parser.add_argument("--staged", action="store_true")
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[2]
    manifest = json.loads((directory / "ssh-manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        path = directory / name
        assert sha(path) == expected, name
        if args.staged:
            raw = subprocess.check_output(["git", "-c", "safe.directory=" + str(root), "show", ":" + path.relative_to(root).as_posix()], cwd=root)
            assert hashlib.sha256(raw).hexdigest() == expected, "staged " + name
    report = json.loads((directory / "ssh-e2e.json").read_text())
    build = json.loads((directory / "ssh-image-build.json").read_text())
    cli = json.loads((directory / "ssh-cli-build.json").read_text())
    assert report["success"] and build["success"] and cli["success"]
    assert cli["exit_code"] == 0
    assert hashlib.sha256(cli["output_utf8"].encode()).hexdigest() == cli["output_sha256"]
    assert report["artifact_sha256"]["cli"] == cli["cli_sha256"]
    assert report["artifact_sha256"]["initrd"] == build["image_sha256"]
    assert report["ssh_fixture"]["build_sha256"] == sha(directory / "ssh-image-build.json")
    assert report["artifact_sha256"]["coordinator"] == sha(directory / "ssh-measured-e2e.py")
    assert build["builder_sha256"] == sha(directory / "ssh-measured-image-builder.py")
    assert cli["coordinator_sha256"] == sha(directory / "ssh-measured-cli-builder.py")
    for source, expected in cli["source_sha256"].items():
        frozen = directory / ("ssh-cli-source-" + source.replace("/", "--") + ".txt")
        assert sha(frozen) == expected, source
    assert not report["cleanup_errors"] and report["remaining_sandboxes"] == 0
    assert len(report["owned_processes_stopped"]) == 22
    cases = report["cases"]
    assert len(cases) == 19 and len({row["name"] for row in cases}) == 19
    assert all(row["success"] for row in cases)
    initial = json.loads((directory / "ssh-initial-e2e.json").read_text())
    assert initial["success"] and not initial["cleanup_errors"]
    assert initial["remaining_sandboxes"] == 0 and len(initial["owned_processes_stopped"]) == 22
    assert len(initial["cases"]) == 19 and all(row["success"] for row in initial["cases"])
    assert initial["cases"] == cases
    by_name = {row["name"]: row["result"] for row in cases}
    binary = by_name["OpenSSH-API-TLS-node-mTLS-binary-roundtrip"]
    sample = bytes(index % 251 for index in range(1024 * 1024))
    assert binary == {"bytes": len(sample), "sha256": hashlib.sha256(sample).hexdigest(), "strict_host_key_checking": True}
    assert by_name["OpenSSH-preserves-remote-exit-code"]["exit_code"] == 7
    assert by_name["OpenSSH-rejects-unregistered-client-key"]["authentication_rejected"]
    assert by_name["OpenSSH-rejects-mismatched-guest-host-key"]["host_identity_rejected"]
    if args.current:
        for source, frozen in manifest["current_sources"].items():
            assert sha(root / source) == sha(directory / frozen), source
    print(json.dumps({"success": True, "files": len(manifest["files_sha256"]), "functional_cases": len(cases), "performance_claim": False}))


if __name__ == "__main__":
    main()
