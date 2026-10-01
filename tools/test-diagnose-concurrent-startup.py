#!/usr/bin/env python3
"""Validate diagnostic duration decoding and duplicate/missing log handling."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("diagnostic", Path(__file__).with_name("diagnose-concurrent-startup.py"))
diagnostic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diagnostic)


class StageLogs(unittest.TestCase):
    def line(self, id="sbx-example"):
        return f"{id} up in 1.5s: build 2ms, launch 3µs, agent answering 1.4s,          network and envd 4ns"

    def test_units_and_ansi_log_prefixes_decode_exactly(self):
        rows = diagnostic.stages("\x1b[34mDEBUG\x1b[0m " + self.line())
        self.assertEqual(rows["sbx-example"], {"total_ms":1500,"build_ms":2,
            "launch_ms":.003,"agent_ms":1400,"network_envd_ms":.000004})

    def test_multiple_ids_are_kept_and_incomplete_lines_do_not_match(self):
        rows = diagnostic.stages(self.line("sbx-first") + "\n" + self.line("sbx-second")
            + "\nsbx-incomplete up in 3ms: build 2ms")
        self.assertEqual(set(rows), {"sbx-first","sbx-second"})

    def test_duplicate_ids_are_refused_instead_of_overwritten(self):
        with self.assertRaisesRegex(RuntimeError, "duplicate"):
            diagnostic.stages(self.line() + "\n" + self.line())


if __name__ == "__main__": unittest.main()
