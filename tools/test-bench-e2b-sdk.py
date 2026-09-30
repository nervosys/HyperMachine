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
    def run_sample(self, *, wrong_marker=False, wrong_resources=False, cleanup_fails=False,
                   operation="create", lost_state=False, partial_fork=False, ignored_pause=False):
        killed = []
        class FakeSandbox:
            sandbox_id = "fixture"
            changed = False
            paused = False
            @property
            def commands(self):
                def run(command, timeout):
                    marker = shlex.split(command)[-1] if "printf '%s'" in command else ""
                    failed = self.changed and lost_state and "kill -0" in command
                    return SimpleNamespace(exit_code=1 if failed else 0,
                                           stdout="wrong" if wrong_marker else marker)
                return SimpleNamespace(run=run)
            def pause(self, **kwargs):
                self.changed = True
                self.paused = not ignored_pause
                return True
            def connect(self, **kwargs):
                self.paused = False
                return self
            def fork(self, **kwargs):
                child = FakeSandbox()
                child.sandbox_id = "child"
                child.changed = True
                return [child, RuntimeError("partial fork")] if partial_fork else [child]
            def get_info(self, **kwargs):
                return SimpleNamespace(cpu_count=2 if wrong_resources else 1, memory_mb=128,
                                       state="paused" if self.paused else "running")
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
                               expected_cpus=1, expected_memory_mb=128, operation=operation)
        with patch.dict(os.environ, {"E2B_API_KEY": "fixture-key"}):
            record = benchmark.sample(Factory, args, "nonce", 0)
        self.assertEqual(killed, ["child", "fixture"] if operation == "fork" else ["fixture"])
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
        self.assertNotIn("fixture-key", str(record["cleanup_errors"]))

    def test_success_has_observed_resources_and_empty_samples_have_no_latency(self):
        record = self.run_sample()
        self.assertTrue(record["success"])
        self.assertEqual(record["memory_mb"], 128)
        self.assertIsNone(benchmark.summary([]))

    def test_resume_requires_preserved_live_process_state(self):
        record = self.run_sample(operation="resume", lost_state=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "ready")
        self.assertIn("pause_ms", record)

    def test_noop_pause_is_not_a_fast_resume(self):
        record = self.run_sample(operation="resume", ignored_pause=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "pause")

    def test_fork_cleans_up_child_and_parent_even_when_state_is_lost(self):
        record = self.run_sample(operation="fork", lost_state=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["parent_sandbox_id"], "fixture")

    def test_unexpected_partial_fork_cleans_up_every_returned_sandbox(self):
        record = self.run_sample(operation="fork", partial_fork=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "fork")

    def test_fork_cleanup_continues_after_child_deletion_fails(self):
        record = self.run_sample(operation="fork", cleanup_fails=True)
        self.assertFalse(record["success"])
        self.assertEqual(len(record["cleanup_errors"]), 2)


if __name__ == "__main__":
    unittest.main()
