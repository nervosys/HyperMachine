#!/usr/bin/env python3
"""Verify Firecracker benchmark setup refuses untrusted or ambiguous artifacts."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("installer", Path(__file__).with_name("install-firecracker-benchmark.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)
NAME = f"firecracker-v{installer.VERSION}-x86_64"


def archive_bytes(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, value in entries:
            entry = tarfile.TarInfo(name)
            if value is None:
                entry.type = tarfile.SYMTYPE
                entry.linkname = "../../outside"
                archive.addfile(entry)
            else:
                entry.size = len(value)
                archive.addfile(entry, io.BytesIO(value))
    return output.getvalue()


class InstallerTests(unittest.TestCase):
    def invoke(self, directory, data, expected=None, version=None):
        source = directory / "source.tgz"
        source.write_bytes(data)
        output = io.StringIO()
        execute = Mock(return_value=version or f"Firecracker v{installer.VERSION}\n\nclean-exit log\n")
        self.last_execute = execute
        with patch.object(sys, "argv", ["installer", "--archive", str(source), "--output-dir", str(directory / "bin")]), \
                patch.object(installer.platform, "system", return_value="Linux"), \
                patch.object(installer.platform, "machine", return_value="x86_64"), \
                patch.object(installer, "ARCHIVE_SHA256", expected or hashlib.sha256(data).hexdigest()), \
                patch.object(installer.subprocess, "check_output", new=execute), \
                contextlib.redirect_stdout(output):
            installer.main()
        return json.loads(output.getvalue()), execute

    def test_checksum_failure_precedes_extraction_or_execution(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            data = archive_bytes([(NAME, b"fixture")])
            with self.assertRaisesRegex(RuntimeError, "checksum/size mismatch"):
                self.invoke(directory, data + b"altered", expected=hashlib.sha256(data).hexdigest())
            self.last_execute.assert_not_called()
            self.assertFalse((directory / "bin").exists())

    def test_linked_and_duplicate_binary_members_are_refused(self):
        for entries in [[(NAME, None)], [(NAME, b"one"), ("nested/" + NAME, b"two")]]:
            with self.subTest(entries=entries), tempfile.TemporaryDirectory() as root:
                directory = Path(root)
                with self.assertRaisesRegex(RuntimeError, "unexpected Firecracker archive member"):
                    self.invoke(directory, archive_bytes(entries))
                self.last_execute.assert_not_called()
                self.assertFalse((directory / "bin").exists())

    def test_existing_different_binary_is_not_overwritten_or_executed(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            destination = directory / "bin" / NAME
            destination.parent.mkdir()
            destination.write_bytes(b"original")
            with self.assertRaisesRegex(RuntimeError, "refusing to overwrite"):
                self.invoke(directory, archive_bytes([(NAME, b"replacement")]))
            self.last_execute.assert_not_called()
            self.assertEqual(destination.read_bytes(), b"original")

    def test_only_named_binary_is_written_and_exit_log_is_accepted(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            result, execute = self.invoke(directory, archive_bytes([
                ("nested/" + NAME, b"verified fixture"), ("../escaped", b"unrelated")]))
            self.assertEqual((directory / "bin" / NAME).read_bytes(), b"verified fixture")
            self.assertFalse((directory / "escaped").exists())
            self.assertEqual(result["binary_sha256"], hashlib.sha256(b"verified fixture").hexdigest())
            self.assertEqual(execute.call_args.args[0], [str((directory / "bin" / NAME).resolve()), "--version"])
            self.assertEqual(execute.call_args.kwargs["timeout"], 10)

    def test_wrong_executable_version_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(RuntimeError, "installed version mismatch"):
                self.invoke(Path(root), archive_bytes([(NAME, b"fixture")]), version="Firecracker v0.0.0\n")


if __name__ == "__main__":
    unittest.main()
