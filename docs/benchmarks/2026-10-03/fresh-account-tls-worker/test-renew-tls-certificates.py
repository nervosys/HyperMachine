#!/usr/bin/env python3
"""Renewal scheduling, durable recovery, process identity and timeout regressions."""
import copy
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

spec = importlib.util.spec_from_file_location("renewal_worker", Path(__file__).with_name("renew-tls-certificates.py"))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


def config():
    return worker.configuration({"certbot": "/owned/certbot", "config_dir": "/owned/account",
        "work_dir": "/owned/work", "logs_dir": "/owned/logs", "interval_seconds": 100,
        "retry_seconds": 10, "jobs": [{"id": "app", "cert_name": "app",
        "lineage": "/owned/account/live/app", "manifest": "/owned/bundle.json",
        "generations": "/owned/generations", "control_plane": "/owned/control",
        "domains": ["app.example.test"]}]})


class Scheduling(unittest.TestCase):
    def setUp(self):
        self.config = config()
        self.state = worker.new_state()
        self.saved = []
        self.renewals = []
        self.activations = []
    def save(self, value):
        self.saved.append(copy.deepcopy(value))
    def renew(self, config, job, force):
        self.renewals.append(job["id"])
        return 0
    def activate(self, config, job):
        self.activations.append(job["id"])
        return {"activation_verified": True, "active_leaf_sha256": "a" * 64, "unchanged": False}
    def cycle(self, now=1000, renew=None, activate=None):
        return worker.run_cycle(self.config, self.state, self.save, now=now,
                                renew_fn=renew or self.renew, activate_fn=activate or self.activate)
    def test_success_is_committed_only_after_activation(self):
        result = self.cycle()[0]
        self.assertTrue(result["success"])
        self.assertEqual([s["jobs"]["app"]["pending"] for s in self.saved],
            [{"phase": "renewing", "renewal_exit": None}, {"phase": "deploying", "renewal_exit": 0}, None])
        self.assertEqual(self.state["jobs"]["app"]["next_check_at"], 1100)
    def test_not_due_does_not_reissue_or_rewrite_journal(self):
        self.cycle()
        saved = len(self.saved)
        self.assertEqual(self.cycle(1099), [{"id": "app", "status": "not_due"}])
        self.assertEqual(len(self.saved), saved)
        self.assertEqual(self.renewals, ["app"])
    def test_missed_intervals_coalesce_into_one_check(self):
        self.cycle()
        self.cycle(9999)
        self.assertEqual(self.renewals, ["app", "app"])
        self.assertEqual(self.state["jobs"]["app"]["next_check_at"], 10099)
    def test_failed_renewal_still_reconciles_but_never_reports_success(self):
        result = self.cycle(renew=lambda *_: 1)[0]
        self.assertEqual(result["status"], "renewal_failed")
        self.assertTrue(result["activation_verified"])
        self.assertFalse(result["success"])
        self.assertEqual(self.state["jobs"]["app"]["next_check_at"], 1010)
    def test_activation_failure_waits_for_backoff_then_retries_without_issuance(self):
        def failed(*_):
            raise RuntimeError("fixture activation failure")
        self.assertEqual(self.cycle(activate=failed)[0]["status"], "deployment_failed")
        self.assertEqual(self.cycle(1009), [{"id": "app", "status": "not_due"}])
        self.assertTrue(self.cycle(1010)[0]["success"])
        self.assertEqual(self.renewals, ["app"])
    def test_crash_after_issuance_restarts_with_deployment_only(self):
        def crash(*_):
            raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            self.cycle(activate=crash)
        self.state = copy.deepcopy(self.saved[-1])
        self.assertTrue(self.cycle(1001)[0]["success"])
        self.assertEqual(self.renewals, ["app"])
    def test_crash_during_issuance_rechecks_certbot(self):
        def crash(*_):
            raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            self.cycle(renew=crash)
        self.state = copy.deepcopy(self.saved[-1])
        self.assertTrue(self.cycle(1001)[0]["success"])
        self.assertEqual(self.renewals, ["app"])
    def test_lost_completion_commit_reconciles_without_new_issuance(self):
        original = self.save
        def fail_completion(state):
            if state["jobs"]["app"]["pending"] is None:
                raise KeyboardInterrupt()
            original(state)
        with self.assertRaises(KeyboardInterrupt):
            worker.run_cycle(self.config, self.state, fail_completion, now=1000,
                             renew_fn=self.renew, activate_fn=self.activate)
        self.state = copy.deepcopy(self.saved[-1])
        self.assertTrue(self.cycle(1001)[0]["success"])
        self.assertEqual(self.renewals, ["app"])
    def test_renewal_timeout_does_not_promote_a_healthy_old_leaf_to_success(self):
        def timeout(*_):
            raise TimeoutError()
        self.assertFalse(self.cycle(renew=timeout)[0]["success"])
        self.assertEqual(self.activations, ["app"])
    def test_pending_configuration_change_is_refused(self):
        def crash(*_):
            raise KeyboardInterrupt()
        with self.assertRaises(KeyboardInterrupt):
            self.cycle(activate=crash)
        self.config["jobs"][0]["domains"] = ["other.example.test"]
        with self.assertRaisesRegex(ValueError, "pending job configuration changed"):
            self.cycle(1001)
    def test_completed_configuration_change_checks_immediately(self):
        self.cycle()
        self.config["interval_seconds"] = 200
        self.assertTrue(self.cycle(1001)[0]["success"])
        self.assertEqual(self.state["jobs"]["app"]["next_check_at"], 1201)
    def test_backward_clock_does_not_defer_for_old_future_deadline(self):
        self.cycle()
        self.assertTrue(self.cycle(500)[0]["success"])
        self.assertEqual(self.state["jobs"]["app"]["next_check_at"], 600)
    def test_one_failed_job_does_not_skip_another_due_job(self):
        second = copy.deepcopy(self.config["jobs"][0])
        second.update(id="second", manifest="/owned/second.json")
        self.config["jobs"].append(second)
        def activation(config, job):
            if job["id"] == "app":
                raise RuntimeError()
            return self.activate(config, job)
        result = self.cycle(activate=activation)
        self.assertFalse(result[0]["success"])
        self.assertTrue(result[1]["success"])
    def test_stop_does_not_start_another_job(self):
        event = threading.Event()
        event.set()
        self.assertEqual(worker.run_cycle(self.config, self.state, self.save, now=1000,
                         renew_fn=self.renew, activate_fn=self.activate, stop=event), [])
        self.assertEqual(self.saved, [])


class Inputs(unittest.TestCase):
    def test_unknown_field_and_plaintext_ca_are_refused(self):
        for changes in ({"unexpected": True}, {"server": "http://127.0.0.1/dir"},
                        {"server": "https://user:secret@example.test/dir"}, {"retry_seconds": True}):
            with self.assertRaises(ValueError):
                worker.configuration({**config(), **changes})
    def test_duplicate_and_mismatched_lineages_are_refused(self):
        raw = config()
        raw["jobs"].append(copy.deepcopy(raw["jobs"][0]))
        with self.assertRaises(ValueError):
            worker.configuration(raw)
        raw = config()
        raw["jobs"][0]["lineage"] = "/other/live/app"
        with self.assertRaises(ValueError):
            worker.configuration(raw)
    def test_future_timestamp_bool_and_corrupt_pending_are_refused(self):
        state = worker.new_state()
        value = {"configuration_sha256": "a" * 64, "next_check_at": 0,
                 "last_started_at": 0, "pending": None, "last_result": None}
        for changes in ({"next_check_at": True}, {"next_check_at": -1},
                        {"pending": {"phase": "unexpected", "renewal_exit": 0}}):
            state["jobs"]["app"] = {**value, **changes}
            with self.assertRaises(ValueError):
                worker.validate_state(state)
    def test_impossible_renewal_and_retry_states_are_refused(self):
        for pending in ({"phase": "renewing", "renewal_exit": 0},
                        {"phase": "retry_deployment", "renewal_exit": 1}):
            state = worker.new_state()
            state["jobs"]["app"] = {"configuration_sha256": "a" * 64, "next_check_at": 0,
                "last_started_at": 0, "pending": pending, "last_result": None}
            with self.assertRaises(ValueError):
                worker.validate_state(state)
    def test_owned_input_rejects_symlink_fifo_and_oversize(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            regular = root / "regular"
            regular.write_bytes(b"abcd")
            (root / "link").symlink_to(regular)
            os.mkfifo(root / "fifo")
            for path in (root / "link", root / "fifo", regular):
                with self.assertRaises((ValueError, OSError)):
                    worker.owned_file(path, 3)


class Processes(unittest.TestCase):
    def test_unique_process_discovery_and_ambiguous_refusal(self):
        processes = []
        with tempfile.TemporaryDirectory() as temporary:
            manifest = str(Path(temporary) / "owned-manifest.json")
            job = {"control_plane": str(Path(sys.executable).resolve()), "manifest": manifest}
            argv = [sys.executable, "-c", "import time; time.sleep(30)", "--tls-bundle-file", manifest]
            try:
                with self.assertRaises(ValueError):
                    worker.selected_pid(job)
                processes.append(subprocess.Popen(argv))
                self.assertEqual(worker.selected_pid(job), processes[0].pid)
                processes.append(subprocess.Popen(argv))
                with self.assertRaises(ValueError):
                    worker.selected_pid(job)
            finally:
                for process in processes:
                    process.terminate()
                    process.wait(timeout=5)
    def test_timed_out_command_reaps_owned_parent_and_stops_descendant(self):
        with tempfile.TemporaryDirectory() as temporary:
            marker = Path(temporary) / "pids"
            code = ("import os,subprocess,sys,time,pathlib; "
                    "c=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); "
                    "pathlib.Path(sys.argv[1]).write_text(str(os.getpid())+' '+str(c.pid)); time.sleep(30)")
            with self.assertRaisesRegex(ValueError, "command timed out"):
                worker.command([sys.executable, "-c", code, str(marker)], dict(os.environ), 0.5, stop=threading.Event())
            parent, child = map(int, marker.read_text().split())
            self.assertFalse(Path(f"/proc/{parent}").exists())
            child_stat = Path(f"/proc/{child}/stat")
            if child_stat.exists():
                self.assertEqual(child_stat.read_text().split(") ", 1)[1][0], "Z")


if __name__ == "__main__":
    unittest.main()
