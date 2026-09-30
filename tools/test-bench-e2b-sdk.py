#!/usr/bin/env python3
"""Exercise failure accounting and cleanup in the shared SDK adapter."""

import importlib.util
import os
from pathlib import Path
import shlex
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("benchmark", Path(__file__).with_name("bench-e2b-sdk.py"))
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class AdapterTests(unittest.TestCase):
    def run_sample(self, *, wrong_marker=False, wrong_resources=False, cleanup_fails=False):
        killed = []
        class FakeSandbox:
            sandbox_id = "fixture"
            @property
            def commands(self):
                return SimpleNamespace(run=lambda command, timeout: SimpleNamespace(
                    exit_code=0, stdout="wrong" if wrong_marker else shlex.split(command)[-1]))
            def get_info(self, **kwargs):
                return SimpleNamespace(cpu_count=2 if wrong_resources else 1, memory_mb=128)
            def kill(self, **kwargs):
                killed.append(self.sandbox_id)
                if cleanup_fails:
                    raise RuntimeError("fixture cleanup failure containing fixture-key")
        class Factory:
            @staticmethod
            def create(**kwargs):
                self.assertFalse(kwargs["debug"])
                self.assertEqual(kwargs["retries"], 0)
                return FakeSandbox()
        args = SimpleNamespace(template="base", api_url="http://fixture", sandbox_url=None,
                               request_timeout=120, command_timeout=30, workload="posix",
                               expected_cpus=1, expected_memory_mb=128)
        with patch.dict(os.environ, {"E2B_API_KEY": "fixture-key"}):
            record = benchmark.sample(Factory, args, "nonce", 0)
        self.assertEqual(killed, ["fixture"])
        return record

    def test_wrong_guest_output_cannot_be_a_fast_success(self):
        record = self.run_sample(wrong_marker=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "ready")

    def test_mismatched_resources_fail_even_after_a_ready_command(self):
        record = self.run_sample(wrong_resources=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "resources")

    def test_cleanup_failure_invalidates_success_and_redacts_the_key(self):
        record = self.run_sample(cleanup_fails=True)
        self.assertFalse(record["success"])
        self.assertNotIn("fixture-key", record["cleanup_error"])

    def test_success_has_observed_resources_and_empty_samples_have_no_latency(self):
        record = self.run_sample()
        self.assertTrue(record["success"])
        self.assertEqual(record["memory_mb"], 128)
        self.assertIsNone(benchmark.summary([]))


if __name__ == "__main__":
    unittest.main()
