#!/usr/bin/env python3
"""Scheduled backup state transitions and upload uncertainty regressions."""
import copy
from datetime import datetime, timedelta, timezone
import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('scheduled',Path(__file__).with_name('schedule-object-backups.py'));scheduled=importlib.util.module_from_spec(spec);spec.loader.exec_module(scheduled)
NOW=datetime(2026,10,2,tzinfo=timezone.utc)


def config():return scheduled.configuration({'start_at':NOW.isoformat(),'interval_seconds':3600,'store':'/owned/store','key_file':'/owned/key','bucket':'owned-backups','prefix':'cluster-a/'})


def upload(args,sink):
    receipt={'operation':'backup','object':args.object,'encrypted_bytes':72,'sha256':hashlib.sha256(b'x'*72).hexdigest()}
    sink(copy.deepcopy(receipt));return {**receipt,'version_id':'owned-version'}


class Schedule(unittest.TestCase):
    def setUp(self):self.config=config();self.state=scheduled.new_state(self.config);self.writes=[]
    def persist(self,value):self.writes.append(copy.deepcopy(value))
    def capture(self,now=NOW,function=upload):return scheduled.run_once(self.config,self.state,now,self.persist,function)
    def test_not_due_before_anchor(self):
        self.assertEqual(self.capture(NOW-timedelta(seconds=1))['status'],'not_due');self.assertEqual(self.writes,[])
    def test_due_commit_and_same_slot_no_repeat(self):
        self.assertTrue(self.capture()['success']);self.assertEqual(self.state['last_slot'],0);self.assertEqual(len(self.state['receipts']),1)
        self.assertEqual([r['current']['phase'] for r in self.writes],['capturing','upload_pending','confirmed'])
        self.assertEqual(self.capture()['status'],'not_due')
    def test_missed_slots_coalesce_and_restart(self):
        self.capture(NOW+timedelta(hours=12));self.assertEqual(self.state['last_slot'],12)
        restored=copy.deepcopy(self.writes[-1]);self.assertIsNone(scheduled.due(self.config,restored,NOW+timedelta(hours=12,minutes=10)))
        self.assertEqual(len(restored['receipts']),1)
    def test_known_pre_upload_failure_can_retry_with_new_key(self):
        objects=[]
        def fail(args,sink):objects.append(args.object);raise ValueError('offline store busy')
        self.assertEqual(self.capture(function=fail)['status'],'capture_failed')
        self.assertTrue(self.capture()['success']);self.assertNotEqual(objects[0],self.state['current']['object'])
    def test_post_intent_failure_blocks_next_slot(self):
        def fail(args,sink):upload(args,sink);raise TimeoutError('remote account detail')
        self.assertEqual(self.capture(function=fail)['status'],'reconciliation_required')
        self.assertEqual(self.state['current']['error'],'TimeoutError')
        self.assertEqual(self.capture(NOW+timedelta(days=1))['status'],'reconciliation_required');self.assertEqual(self.state['receipts'],[])
    def test_crash_pending_survives_reload(self):
        self.capture();pending=self.writes[1];restored=copy.deepcopy(pending)
        result=scheduled.run_once(self.config,restored,NOW+timedelta(hours=1),self.persist,lambda *args:self.fail('unexpected upload'))
        self.assertEqual(result['status'],'reconciliation_required')
    def test_confirmation_uses_original_attempt_timestamp(self):
        self.capture();self.state=copy.deepcopy(self.writes[1]);receipt={**self.state['current']['attempt_receipt'],'version_id':'reconciled-version'}
        result=scheduled.confirmed(self.config,self.state,receipt,self.persist)
        self.assertTrue(result['success']);self.assertEqual(self.state['receipts'][0]['created_at'],NOW.isoformat())
    def test_missing_intent_cannot_confirm(self):
        def invalid(args,sink):return {'operation':'backup','object':args.object,'version_id':'v'}
        self.assertEqual(self.capture(function=invalid)['status'],'capture_failed');self.assertEqual(self.state['receipts'],[])
    def test_unversioned_ack_blocks_and_preserves_ack(self):
        def invalid(args,sink):return {**upload(args,sink),'version_id':'null'}
        self.assertEqual(self.capture(function=invalid)['status'],'reconciliation_required')
        self.assertEqual(self.state['current']['acknowledged_receipt']['version_id'],'null')
    def test_changed_configuration_refused(self):
        changed={**self.config,'prefix':'other/'}
        with self.assertRaises(ValueError):scheduled.due(changed,self.state,NOW)
    def test_configuration_typo_and_ranges_refused(self):
        raw={key:self.config[key] for key in scheduled.REQUIRED}
        for changed in [{**raw,'interval_seconds':True},{**raw,'interval_seconds':0},{**raw,'intervl_seconds':3600},{**raw,'prefix':'../'}]:
            with self.subTest(changed=changed),self.assertRaises(ValueError):scheduled.configuration(changed)
    def test_failed_pending_persist_never_uploads(self):
        calls=[]
        def persist(value):
            if value['current']['phase']=='upload_pending':raise OSError('disk full')
            self.persist(value)
        def capture(args,sink):
            receipt={'operation':'backup','object':args.object};sink(receipt);calls.append('uploaded');return receipt
        result=scheduled.run_once(self.config,self.state,NOW,persist,capture)
        self.assertFalse(result['success']);self.assertEqual(calls,[])
    def test_failed_confirm_persist_preserves_acknowledged_receipt(self):
        def persist(value):
            if value['current']['phase']=='confirmed':raise OSError('disk full')
            self.persist(value)
        result=scheduled.run_once(self.config,self.state,NOW,persist,upload)
        self.assertEqual(result['status'],'reconciliation_required')
        self.assertEqual(result['current']['acknowledged_receipt']['version_id'],'owned-version')
        self.assertIsNone(self.state['last_slot'])
    def test_failed_error_persist_reports_uncertainty(self):
        def persist(value):
            if value['current']['phase']=='uncertain':raise OSError('disk full')
            self.persist(value)
        def capture(args,sink):upload(args,sink);raise TimeoutError()
        result=scheduled.run_once(self.config,self.state,NOW,persist,capture)
        self.assertEqual(result['status'],'journal_write_failed')
        self.assertEqual(result['current']['phase'],'uncertain')
        self.assertIn('sha256',result['current']['attempt_receipt'])
    def test_corrupted_journal_phase_or_pending_identity_refused(self):
        self.capture();pending=copy.deepcopy(self.writes[1])
        for value in [{**pending,'current':{**pending['current'],'phase':'unknown'}},{**pending,'current':{**pending['current'],'attempt_receipt':{**pending['current']['attempt_receipt'],'object':'other/wrong'}}}]:
            with self.assertRaises(ValueError):scheduled.due(self.config,value,NOW)
    def test_explicit_skip_preserves_unknown_backup_and_allows_later_slot(self):
        self.capture();self.state=copy.deepcopy(self.writes[1]);receipt=self.state['current']['attempt_receipt']
        with self.assertRaises(ValueError):scheduled.skip_attempt(self.state,'0'*64,'object_absence_verified',self.persist)
        result=scheduled.skip_attempt(self.state,receipt['sha256'],'backup_unrecoverable',self.persist)
        self.assertFalse(result['storage_deleted']);self.assertEqual(self.state['receipts'],[])
        self.assertEqual(self.state['skipped_attempts'][0]['attempt_receipt'],receipt)
        self.assertTrue(self.capture(NOW+timedelta(hours=1))['success'])
        self.assertEqual(len(self.state['skipped_attempts']),1)
    def test_pin_survives_new_capture_and_unpin_is_explicit(self):
        receipt=self.capture()['receipt'];scheduled.set_pin(self.state,receipt['object'],receipt['version_id'],True,self.persist)
        self.capture(NOW+timedelta(hours=1));self.assertTrue(self.state['receipts'][0]['pinned'])
        scheduled.set_pin(self.state,receipt['object'],receipt['version_id'],False,self.persist)
        self.assertFalse(self.state['receipts'][0]['pinned'])
        with self.assertRaises(ValueError):scheduled.set_pin(self.state,receipt['object'],'wrong-version',True,self.persist)
    @unittest.skipUnless(sys.platform=='linux','Linux file locking')
    def test_process_lock_and_private_atomic_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'state'
            with scheduled.locked_state(root,self.config):
                with self.assertRaises(BlockingIOError):
                    with scheduled.locked_state(root,self.config):pass
                scheduled.atomic_json(root,'journal.json',self.state)
                self.assertEqual(scheduled.read_json(root/'journal.json'),self.state)
                self.assertEqual((root/'journal.json').stat().st_mode&0o777,0o600)
    @unittest.skipUnless(sys.platform=='linux','Linux state filesystem')
    def test_state_inside_store_refused_before_creation(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Path(directory)/'store';store.mkdir();root=store/'state';value={**self.config,'store':str(store)}
            with self.assertRaises(ValueError):
                with scheduled.locked_state(root,value):pass
            self.assertFalse(root.exists())


if __name__=='__main__':unittest.main()
