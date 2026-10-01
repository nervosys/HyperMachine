#!/usr/bin/env python3
"""Exercise failure accounting and cleanup in the shared SDK adapter."""

import importlib.util
import io
import json
import os
from pathlib import Path
import shlex
import sys
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("benchmark", Path(__file__).with_name("bench-e2b-sdk.py"))
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class AdapterTests(unittest.TestCase):
    def run_sample(self, *, wrong_marker=False, wrong_resources=False, cleanup_fails=False,
                   operation="create", lost_state=False, partial_fork=False, ignored_pause=False,
                   shared_filesystem=False, barrier=None, failed_fork=False):
        killed = []
        spawned = []
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
                if failed_fork:
                    return [RuntimeError("guest readiness timeout containing fixture-key")]
                spawned.append("child")
                child = FakeSandbox()
                child.sandbox_id = "child"
                child.changed = True
                return [child, RuntimeError("partial fork containing fixture-key")] if partial_fork else [child]
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
            record = benchmark.sample(Factory, args, "nonce", 0, barrier, time.perf_counter())
        self.assertEqual(killed, ["child", "fixture"] if spawned else ["fixture"])
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
        self.assertIn("partial fork", record["fork_errors"][0])
        self.assertNotIn("fixture-key", str(record["fork_errors"]))

    def test_failed_fork_preserves_the_underlying_error_and_deletes_parent(self):
        record = self.run_sample(operation="fork", failed_fork=True)
        self.assertFalse(record["success"])
        self.assertIn("guest readiness timeout", record["fork_errors"][0])
        self.assertNotIn("fixture-key", str(record))
        self.assertEqual(record["failure_timing_origin"], "operation")
        self.assertGreaterEqual(record["failure_elapsed_ms"], 0)

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

    def test_failed_pause_aborts_batch_before_measured_resume(self):
        barrier = threading.Barrier(2)
        record = self.run_sample(operation="resume", ignored_pause=True, barrier=barrier)
        self.assertTrue(barrier.broken)
        self.assertFalse(record["success"])
        self.assertNotIn("operation_start_offset_ms", record)

    def test_broken_batch_still_deletes_the_prepared_guest(self):
        barrier = threading.Barrier(2)
        barrier.abort()
        record = self.run_sample(operation="fork", barrier=barrier)
        self.assertFalse(record["success"])
        self.assertEqual(record["phase"], "operation-barrier")
        self.assertIn("could not synchronize", record["error"])
        self.assertEqual(record["failure_timing_origin"], "create")

    def test_single_member_batch_records_measured_operation_start(self):
        record = self.run_sample(operation="resume", barrier=threading.Barrier(1))
        self.assertTrue(record["success"])
        self.assertGreaterEqual(record["operation_start_offset_ms"], 0)

    def test_preparation_failure_releases_waiting_peer(self):
        barrier = threading.Barrier(2)
        waiting = threading.Event()
        result = []
        def peer():
            waiting.set()
            try:
                benchmark.synchronize_operation(barrier, 5)
            except RuntimeError as error:
                result.append(str(error))
        thread = threading.Thread(target=peer)
        thread.start()
        self.assertTrue(waiting.wait(1))
        self.run_sample(operation="resume", ignored_pause=True, barrier=barrier)
        thread.join(1)
        self.assertFalse(thread.is_alive())
        self.assertEqual(len(result), 1)

    def test_partial_final_batch_uses_its_actual_participant_count(self):
        observed = []
        def fake_sample(factory, args, nonce, index, barrier, started):
            benchmark.synchronize_operation(barrier, 1)
            observed.append((index, barrier.parties))
            return {"index": index, "success": True}
        args = SimpleNamespace(concurrency=3, samples=5, synchronized_operation_batches=True)
        with patch.object(benchmark, "sample", side_effect=fake_sample):
            records = benchmark.run_samples(None, args, "nonce", time.perf_counter())
        self.assertEqual(sorted(observed), [(0, 3), (1, 3), (2, 3), (3, 2), (4, 2)])
        self.assertEqual([record["batch_index"] for record in records], [0, 0, 0, 1, 1])

    def test_create_cannot_be_labeled_as_synchronized_stateful_operations(self):
        arguments = ["benchmark", "--provider", "fixture", "--api-url", "http://fixture",
                     "--template", "base", "--environment", "fixture", "--image-description", "fixture",
                     "--expected-cpus", "1", "--expected-memory-mb", "128", "--synchronized-operation-batches"]
        with patch.object(sys, "argv", arguments), patch.object(sys, "stderr", io.StringIO()), \
                self.assertRaises(SystemExit) as error:
            benchmark.main()
        self.assertEqual(error.exception.code, 2)

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
