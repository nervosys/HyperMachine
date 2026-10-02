#!/usr/bin/env python3
"""Verify registration ACL regression evidence."""
import hashlib
import json
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    manifest = json.loads((directory / "manifest.json").read_text())
    for name, expected in manifest["files_sha256"].items():
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, name
    before = json.loads((directory / "before.json").read_text())
    after = json.loads((directory / "after.json").read_text())
    assert not before["success"] and before["test_exit_code"] == 101
    assert after["success"] and after["test_exit_code"] == 0
    for report in (before, after):
        assert report["owned_redis_stopped"] and report["redis_exit_code"] == 0
        assert report["temporary_acl_users_removed"]
    assert "registration partially wrote after denying sadd" in (directory / "before.tests.log").read_text()
    tests = (directory / "after.tests.log").read_text()
    assert "38 passed; 0 failed" in tests
    for name in ("redis_store_keeps_the_contract", "redis_named_registration_checks_index_types_before_writing",
                 "redis_named_registration_checks_write_permissions_before_writing"):
        assert f"test store::tests::{name} ... ok" in tests
    assert "skipped:" not in tests
    lint = (directory / "clippy.log").read_text()
    assert "Finished" in lint and "error:" not in lint
    print(json.dumps({"success": True, "tests": 38, "ACL_regression_verified": True,
                      "files": len(manifest["files_sha256"])}))


if __name__ == "__main__":
    main()
