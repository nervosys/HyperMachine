#!/usr/bin/env python3
"""Exercise failure accounting and cleanup in the shared SDK adapter."""

import importlib.util
import io
import json
import os
from pathlib import Path
import shlex
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("benchmark", Path(__file__).with_name("bench-e2b-sdk.py"))
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class AdapterTests(unittest.TestCase):
    def run_sample(self, *, wrong_marker=False, wrong_resources=False, cleanup_fails=False,
                   operation="create", lost_state=False, partial_fork=False, ignored_pause=False,
                   shared_filesystem=False):
        killed = []
        isolation = {"mutated": False}
        class FakeSandbox:
            sandbox_id = "fixture"
            changed = False
            paused = False
            @property
            def commands(self):
                def run(command, timeout):
                    marker = shlex.split(command)[-1] if "printf '%s'" in command else ""
                    failed = self.changed and lost_state and "kill -0" in command
                    if self.sandbox_id == "child" and command.startswith("printf '%s' "):
                        isolation["mutated"] = True
                    if self.sandbox_id == "fixture" and shared_filesystem and isolation["mutated"]:
                        failed = True
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

    def test_shared_fork_filesystem_is_rejected_and_both_sandboxes_cleaned_up(self):
        record = self.run_sample(operation="fork", shared_filesystem=True)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "parent-state")

    def test_fork_success_requires_a_child_write_that_preserves_parent_state(self):
        record = self.run_sample(operation="fork")
        self.assertTrue(record["success"])
        self.assertTrue(record["fork_filesystem_isolation_verified"])

    def test_changed_harness_invalidates_an_otherwise_successful_report(self):
        arguments = ["benchmark", "--provider", "fixture", "--api-url", "http://fixture",
                     "--template", "base", "--environment", "fixture", "--image-description", "fixture",
                     "--expected-cpus", "1", "--expected-memory-mb", "128", "--samples", "1"]
        row = {"success": True, "create_ms": 1, "exec_ms": 1, "ready_ms": 2}
        for final_digest, changed in [("a" * 64, False), ("b" * 64, True), (OSError("removed"), True)]:
            output = io.StringIO()
            with patch.object(sys, "argv", arguments), patch.object(sys, "stdout", output), \
                    patch.dict(os.environ, {"E2B_API_KEY": "fixture-key", "E2B_API_URL": "",
                                            "E2B_SANDBOX_URL": "", "E2B_ENVD_POOL_SHARDS": ""}), \
                    patch.dict(sys.modules, {"e2b": SimpleNamespace(Sandbox=object())}), \
                    patch.object(benchmark.importlib.metadata, "version", return_value="2.51.0"), \
                    patch.object(benchmark, "sample", return_value=row), \
                    patch.object(benchmark, "harness_digest", side_effect=["a" * 64, final_digest]):
                result = benchmark.main()
            report = json.loads(output.getvalue())
            self.assertEqual(result, 1 if changed else 0)
            self.assertEqual(report["successful_samples"], 1)
            self.assertEqual(report["harness_sha256"], "a" * 64)
            self.assertEqual(report["harness_unchanged_during_run"], not changed)


if __name__ == "__main__":
    unittest.main()
