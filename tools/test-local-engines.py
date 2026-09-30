#!/usr/bin/env python3
"""Verify matched-workload failures cannot become successful benchmark samples."""
import importlib.util
from pathlib import Path
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("engines", Path(__file__).with_name("bench-local-engines.py"))
engines = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engines)


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
