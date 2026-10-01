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


class ColdLogs(unittest.TestCase):
    def test_success_and_connection_failure(self):
        log = 'cold guest readiness stages vm=sbx-one blocking_queue_ms=1e-3 connect_ms=4.5 ping_ms=2 succeeded=true phase="ping"\n'
        log += 'cold guest readiness stages vm=sbx-two blocking_queue_ms=3 connect_ms=15 succeeded=false phase="connect"'
        rows = diagnostic.cold_stages(log)
        self.assertEqual(rows['sbx-one'], {'phase':'ping','succeeded':True,
            'blocking_queue_ms':.001,'connect_ms':4.5,'ping_ms':2})
        self.assertFalse(rows['sbx-two']['succeeded'])
        self.assertNotIn('ping_ms', rows['sbx-two'])

    def test_duplicates_and_incomplete_events_fail(self):
        line = 'cold guest readiness stages vm=sbx-one blocking_queue_ms=1 connect_ms=2 ping_ms=3 succeeded=true phase="ping"'
        with self.assertRaisesRegex(RuntimeError, 'duplicate'):
            diagnostic.cold_stages(line + '\n' + line)
        with self.assertRaisesRegex(RuntimeError, 'incomplete'):
            diagnostic.cold_stages(line.replace('ping_ms=3', ''))


class DispatchLogs(unittest.TestCase):
    def test_matched_stages_and_units(self):
        rows = diagnostic.dispatch_stages('VM background dispatch vm=sbx-one dispatch_queue_ms=1e-3\n'
            'vCPU owner thread entry vm=sbx-one vcpu_id=0 wrapper_queue_ms=2 thread_start_ms=3.5')
        self.assertEqual(rows['sbx-one'], {'dispatch_queue_ms':.001,'wrapper_queue_ms':2,'thread_start_ms':3.5})

    def test_duplicate_missing_duration_and_extra_vcpu_fail(self):
        line = 'VM background dispatch vm=sbx-one dispatch_queue_ms=1'
        with self.assertRaisesRegex(RuntimeError,'duplicate'):
            diagnostic.dispatch_stages(line+'\n'+line)
        with self.assertRaisesRegex(RuntimeError,'incomplete'):
            diagnostic.dispatch_stages('VM background dispatch vm=sbx-one')
        with self.assertRaisesRegex(RuntimeError,'one vCPU'):
            diagnostic.dispatch_stages('vCPU owner thread entry vm=sbx-one vcpu_id=1 wrapper_queue_ms=2 thread_start_ms=3')


if __name__ == "__main__": unittest.main()
