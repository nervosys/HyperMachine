#!/usr/bin/env python3
"""Exercise burst synchronization, measurement and failure accounting without KVM."""
import importlib.util
from pathlib import Path
import threading
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, Mock, patch

spec = importlib.util.spec_from_file_location("burst", Path(__file__).with_name("bench-local-engines-concurrent.py"))
burst = importlib.util.module_from_spec(spec)
spec.loader.exec_module(burst)


class AllocatorEnvironment(unittest.TestCase):
    def test_default_child_ignores_inherited_allocator_and_guest_overrides(self):
        args = SimpleNamespace(kernel=Path("kernel"), initrd=Path("initrd"))
        with patch.dict(burst.os.environ, {"MALLOC_ARENA_MAX":"1", "HV2_KERNEL":"wrong", "LD_PRELOAD":"wrong"}):
            child = burst.daemon_environment(args)
        self.assertEqual(child["HV2_KERNEL"], "kernel")
        self.assertEqual(child["HV2_INITRD"], "initrd")
        self.assertNotIn("MALLOC_ARENA_MAX", child)
        self.assertNotIn("LD_PRELOAD", child)

    def test_explicit_setting_is_scoped_to_child_environment(self):
        args = SimpleNamespace(kernel=Path("kernel"), initrd=Path("initrd"), daemon_allocator_arena_max=2)
        before = dict(burst.os.environ)
        self.assertEqual(burst.daemon_environment(args)["MALLOC_ARENA_MAX"], "2")
        self.assertEqual(dict(burst.os.environ), before)


class Bursts(unittest.TestCase):
    def run_batch(self, failure=None, memory_failure=False):
        alive, measured = set(), []
        lock = threading.Lock()
        def attempt(args, row, started, ready):
            row["start_offset_ms"] = (time.perf_counter()-started)*1000
            with lock: alive.add(row["index"])
            row["success"] = row["index"] != failure
            if row["success"]: row["ready_ms"] = 1.0
            else: row["error"] = "retained failure"
            ready(row, row["index"]+100)
            with lock: alive.remove(row["index"])
            row["cleanup_success"] = True
        def memory(pid):
            with lock:
                self.assertEqual(len(alive), 8, "a guest was cleaned up before measurement")
                measured.append(pid)
            if memory_failure: raise OSError("smaps unavailable")
            return {"Pss_kib":100, "Rss_kib":200}
        result = burst.batch(SimpleNamespace(concurrency=8), "firecracker", 3, attempt, memory)
        self.assertEqual(alive, set(), "owned guest cleanup did not complete")
        self.assertTrue(all(row["cleanup_success"] for row in result["samples"]))
        self.assertEqual(len(result["samples"]), 8)
        return result, measured

    def test_every_guest_is_held_through_measurement_and_then_cleaned(self):
        result, measured = self.run_batch()
        self.assertTrue(result["success"], result)
        self.assertTrue(result["all_guests_validated_while_held"])
        self.assertEqual(len(measured), 8)
        self.assertEqual(result["held_process_count"], 8)
        self.assertEqual(result["held_process_memory_kib"]["Pss_kib"], 800)
        self.assertGreater(result["passing_attempts_per_second"], 0)
        self.assertGreaterEqual(result["total_wall_ms_including_cleanup"], result["readiness_wall_ms"])

    def test_failed_readiness_is_retained_in_denominator(self):
        result, _ = self.run_batch(failure=3)
        self.assertFalse(result["success"])
        self.assertFalse(result["all_guests_validated_while_held"])
        self.assertEqual(result["passing_attempts"], 7)
        self.assertEqual(result["samples"][3]["error"], "retained failure")

    def test_measurement_failure_still_releases_all_cleanup(self):
        result, _ = self.run_batch(memory_failure=True)
        self.assertFalse(result["success"])
        self.assertIn("smaps unavailable", result["error"])

    def test_unexpected_worker_error_cannot_strand_other_guests(self):
        def fail(args, row, started, ready):
            raise RuntimeError("unexpected worker failure")
        result = burst.batch(SimpleNamespace(concurrency=8), "firecracker", 0, fail, lambda _: {})
        self.assertFalse(result["success"])
        self.assertEqual(len(result["samples"]), 8)
        self.assertTrue(all(row["error"] == "unexpected worker failure" for row in result["samples"]))


class IdleMemory(unittest.TestCase):
    def run_idle(self, engine="hypermachine", baseline=None, fail_idle_read=False):
        alive, calls, waits = set(), [], []
        lock = threading.Lock()
        def attempt(args, row, started, ready):
            row.update(start_offset_ms=0, ready_ms=1, success=True)
            with lock: alive.add(row["index"])
            ready(row, row["index"]+100)
            with lock: alive.remove(row["index"])
            row["cleanup_success"] = True
        def memory(pid):
            with lock:
                self.assertEqual(len(alive), 2)
            calls.append(pid)
            if fail_idle_read and len(calls) > 2:
                raise OSError("idle smaps unavailable")
            return {"Pss_kib":100, "Rss_kib":200}
        def idle(seconds):
            with lock: self.assertEqual(len(alive), 2)
            waits.append(seconds)
        result = burst.batch(SimpleNamespace(concurrency=2, memory_idle_seconds=5), engine, 0,
            attempt, memory, memory_baseline=baseline, idle_wait=idle)
        self.assertEqual(waits, [5])
        self.assertEqual(alive, set())
        self.assertTrue(all(row["cleanup_success"] for row in result["samples"]))
        return result, calls

    def test_same_batch_baseline_is_subtracted_without_clamping(self):
        result, calls = self.run_idle(baseline={"Pss_kib":250,"Rss_kib":300})
        self.assertTrue(result["success"], result)
        self.assertEqual(len(calls), 4)
        self.assertEqual(result["incremental_idle_process_memory_kib"], {"Pss_kib":-50,"Rss_kib":100})
        self.assertEqual(result["memory_idle_requested_seconds"], 5)
        self.assertIn("guest_idle_at_measurement_start_ms", result)

    def test_fresh_firecracker_processes_use_zero_process_baseline(self):
        result, _ = self.run_idle(engine="firecracker")
        self.assertTrue(result["success"], result)
        self.assertEqual(result["empty_process_memory_baseline_kib"], {"Pss_kib":0,"Rss_kib":0})
        self.assertEqual(result["incremental_idle_process_memory_kib"]["Pss_kib"], 200)

    def test_missing_baseline_or_failed_idle_read_retains_failure_and_cleans_up(self):
        for options in [{}, {"baseline":{"Pss_kib":1}}, {"fail_idle_read":True}]:
            with self.subTest(options=options):
                result, _ = self.run_idle(**options)
                self.assertFalse(result["success"])
                self.assertIsNotNone(result["error"])


class NativeAttempts(unittest.TestCase):
    def run_hm(self, result=None, resources=None):
        events = []
        def request(url, method, path, body=None):
            events.append(method)
            if path == "/v2/sandboxes": return {"sandboxID":"fixture"}
            if path.endswith("/exec"):
                return result or {"exit_code":0,"stdout":"hm-concurrent-fixture","timed_out":False}
            if method == "GET": return resources or {"cpuCount":1,"memoryMB":1024}
            return None
        row = {"success":False,"cleanup_success":False}
        with patch.object(burst.engines, "request", side_effect=request), \
                patch.object(burst.uuid, "uuid4", return_value=SimpleNamespace(hex="fixture")):
            burst.hm_attempt(SimpleNamespace(url="http://fixture", node_pid=42), row,
                time.perf_counter(), lambda row, pid: events.append("HELD"))
        self.assertEqual(events[-2:], ["HELD", "DELETE"])
        self.assertTrue(row["cleanup_success"])
        return row

    def test_valid_guest_and_exact_resources_pass(self):
        self.assertTrue(self.run_hm()["success"])

    def test_guest_output_exit_timeout_and_truncation_fail(self):
        for result in [
            {"exit_code":0,"stdout":"wrong"},
            {"exit_code":7,"stdout":"hm-concurrent-fixture"},
            {"exit_code":0,"stdout":"hm-concurrent-fixture","timed_out":True},
            {"exit_code":0,"stdout":"hm-concurrent-fixture","truncated":True},
        ]:
            with self.subTest(result=result): self.assertFalse(self.run_hm(result=result)["success"])

    def test_resource_mismatch_fails_and_still_deletes(self):
        self.assertFalse(self.run_hm(resources={"cpuCount":2,"memoryMB":1024})["success"])

    def test_failed_create_is_retained_without_deleting_an_unknown_id(self):
        row = {"success":False,"cleanup_success":False}
        held = []
        with patch.object(burst.engines, "request", return_value={}) as request:
            burst.hm_attempt(SimpleNamespace(url="http://fixture", node_pid=42), row,
                time.perf_counter(), lambda row, pid: held.append(row))
        self.assertFalse(row["success"])
        self.assertEqual(len(held), 1)
        self.assertEqual(request.call_count, 1)
        self.assertIn("no known sandbox ID", row["error"])

    def run_fc(self, result, config=None, total_timeout=30):
        row = {"success":False,"cleanup_success":False}
        events = []
        process = Mock(pid=42)
        guest = MagicMock()
        with tempfile.TemporaryDirectory() as directory:
            scratch = SimpleNamespace(name=directory, cleanup=Mock())
            with patch.object(burst.tempfile, "TemporaryDirectory", return_value=scratch), \
                    patch.object(burst.subprocess, "Popen", return_value=process), \
                    patch.object(burst.fc, "wait_api"), \
                    patch.object(burst.fc, "api", return_value=config or {"vcpu_count":1,"mem_size_mib":1024}), \
                    patch.object(burst.fc, "guest", return_value=guest) as connect, \
                    patch.object(burst.fc, "rpc", return_value=result), \
                    patch.object(burst.time, "perf_counter", return_value=10), \
                    patch.object(burst.engines, "stop", side_effect=lambda p: events.append("STOP") or True):
                burst.fc_attempt(SimpleNamespace(firecracker=Path("fc"), kernel=Path("kernel"),
                    initrd=Path("initrd"), timeout=total_timeout), row, 0,
                    lambda row, pid: events.append("HELD"))
                self.assertEqual(connect.call_args.args[1], min(10+total_timeout, 25))
            scratch.cleanup.assert_called_once()
        self.assertEqual(events, ["HELD","STOP"])
        self.assertTrue(row["cleanup_success"])
        return row

    def test_firecracker_matches_guest_budget_and_caps_total_deadline(self):
        result = {"kind":"exited","exit_code":0,"stdout":"hm-concurrent-fixture"}
        with patch.object(burst.uuid, "uuid4", return_value=SimpleNamespace(hex="fixture")):
            for timeout in [8, 30]: self.assertTrue(self.run_fc(result, total_timeout=timeout)["success"])

    def test_firecracker_guest_or_resource_mismatch_still_stops_owned_process(self):
        result = {"kind":"exited","exit_code":0,"stdout":"hm-concurrent-fixture"}
        with patch.object(burst.uuid, "uuid4", return_value=SimpleNamespace(hex="fixture")):
            self.assertFalse(self.run_fc({**result,"stdout":"wrong"})["success"])
            self.assertFalse(self.run_fc(result, config={"vcpu_count":2,"mem_size_mib":1024})["success"])


if __name__ == "__main__": unittest.main()
