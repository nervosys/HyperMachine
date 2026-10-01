#!/usr/bin/env python3
"""Verify matched-workload failures cannot become successful benchmark samples."""
import importlib.util
import errno
from pathlib import Path
import types
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("engines", Path(__file__).with_name("bench-local-engines.py"))
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)


class FirecrackerReadiness(unittest.TestCase):
    def test_socket_created_before_listen_retries_only_read_only_probe(self):
        process = Mock()
        process.poll.return_value = None
        with patch.object(engines.fc, "api", side_effect=[
            FileNotFoundError(errno.ENOENT, "not bound"),
            ConnectionRefusedError(errno.ECONNREFUSED, "not listening"), {}]) as api, \
                patch.object(engines.fc.time, "perf_counter", return_value=0), \
                patch.object(engines.fc.time, "sleep"):
            engines.fc.wait_api(Path("api.sock"), 1, process)
        self.assertEqual(api.call_count, 3)
        for call in api.call_args_list:
            self.assertEqual(call.args[1:], ("GET", "/machine-config"))

    def test_probe_stops_on_deadline_or_process_exit(self):
        process = Mock()
        process.poll.return_value = None
        with patch.object(engines.fc.time, "perf_counter", return_value=1), \
                patch.object(engines.fc, "api") as api:
            with self.assertRaises(TimeoutError):
                engines.fc.wait_api(Path("api.sock"), 1, process)
            api.assert_not_called()
        process.poll.return_value = 1
        with patch.object(engines.fc.time, "perf_counter", return_value=0), \
                patch.object(engines.fc, "api") as api:
            with self.assertRaises(RuntimeError):
                engines.fc.wait_api(Path("api.sock"), 1, process)
            api.assert_not_called()

    def test_probe_does_not_hide_other_transport_or_api_errors(self):
        process = Mock()
        process.poll.return_value = None
        for error in [PermissionError(errno.EACCES, "denied"), RuntimeError("API 500")]:
            with patch.object(engines.fc.time, "perf_counter", return_value=0), \
                    patch.object(engines.fc, "api", side_effect=error) as api:
                with self.assertRaises(type(error)):
                    engines.fc.wait_api(Path("api.sock"), 1, process)
                self.assertEqual(api.call_count, 1)


class Samples(unittest.TestCase):
    def run_sample(self, replies, memory=None):
        with patch.object(engines.uuid,"uuid4",return_value=types.SimpleNamespace(hex="fixture")), \
                patch.object(engines,"request",side_effect=replies) as request, \
                patch.object(engines,"memory",side_effect=memory or [{"Pss_kib":100},{"Pss_kib":50}]):
            row = engines.hm_sample("http://127.0.0.1",0,42)
        return row,request

    def valid(self):
        return [{"sandboxID":"known-vm"},{"exit_code":0,"stdout":"hm-engine-fixture","timed_out":False},
                {"cpuCount":1,"memoryMB":1024},None]

    def test_valid_workload_resources_and_deletion_pass(self):
        row,request = self.run_sample(self.valid())
        self.assertTrue(row["success"] and row["cleanup_success"])
        self.assertEqual(request.call_args.args[1:3],("DELETE","/sandboxes/known-vm"))

    def test_wrong_guest_output_fails_but_deletes_known_vm(self):
        row,request = self.run_sample([{"sandboxID":"known-vm"},{"exit_code":0,"stdout":"wrong"},None])
        self.assertFalse(row["success"])
        self.assertTrue(row["cleanup_success"])
        self.assertEqual(request.call_count,3)

    def test_wrong_resources_fail_but_delete_known_vm(self):
        replies = self.valid(); replies[2]["memoryMB"] = 512
        row,_ = self.run_sample(replies)
        self.assertFalse(row["success"])
        self.assertTrue(row["cleanup_success"])

    def test_failed_deletion_is_not_a_passing_sample(self):
        replies = self.valid(); replies[-1] = OSError("delete failed")
        row,_ = self.run_sample(replies)
        self.assertFalse(row["cleanup_success"])
        self.assertIn("cleanup_error",row)

    def test_failed_memory_measurement_invalidates_run(self):
        row,_ = self.run_sample(self.valid()[:2] + [self.valid()[2],None], [OSError("measurement failed"),{}])
        self.assertFalse(row["success"])
        self.assertTrue(row["cleanup_success"])

    def test_post_delete_measurement_failure_is_not_a_resource_leak(self):
        row,_ = self.run_sample(self.valid(), [{},OSError("measurement failed")])
        self.assertFalse(row["success"])
        self.assertTrue(row["cleanup_success"])
        self.assertIn("diagnostic_error",row)

    def test_create_without_known_id_cannot_claim_cleanup(self):
        row,_ = self.run_sample([{}])
        self.assertFalse(row["success"] or row["cleanup_success"])


if __name__ == "__main__": unittest.main()
