#!/usr/bin/env python3
"""Verify protoc installer refuses altered artifacts and unsupported hosts."""
import hashlib
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("installer", Path(__file__).with_name("ci-install-protoc.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def test_altered_archive_is_refused(self):
        original = b"fixture compiler archive"
        with patch.dict(installer.HASHES, {"fixture": hashlib.sha256(original).hexdigest()}):
            installer.verify(original, "fixture")
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                installer.verify(original + b"altered", "fixture")

    def test_host_architecture_selects_the_matching_locked_artifact(self):
        for system, machine, expected in [("Windows", "AMD64", "win64"),
                                          ("Linux", "x86_64", "linux-x86_64"),
                                          ("Linux", "aarch64", "linux-aarch_64"),
                                          ("Darwin", "x86_64", "osx-x86_64"),
                                          ("Darwin", "arm64", "osx-aarch_64")]:
            with patch.object(installer.platform, "system", return_value=system), \
                    patch.object(installer.platform, "machine", return_value=machine):
                self.assertEqual(installer.target(), expected)

    def test_unsupported_architecture_does_not_silently_use_another_binary(self):
        with patch.object(installer.platform, "system", return_value="Windows"), \
                patch.object(installer.platform, "machine", return_value="arm64"):
            with self.assertRaisesRegex(RuntimeError, "unsupported protoc platform"):
                installer.target()


if __name__ == "__main__":
    unittest.main()
