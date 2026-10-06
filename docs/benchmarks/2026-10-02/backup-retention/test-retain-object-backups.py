#!/usr/bin/env python3
"""Retention policy, preflight and uncertain-delete regression checks."""
import copy
from datetime import datetime, timezone
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('retention',Path(__file__).with_name('retain-object-backups.py'));retention=importlib.util.module_from_spec(spec);spec.loader.exec_module(retention)
NOW=datetime(2026,10,2,tzinfo=timezone.utc)
BODY=b'HMBACK01'+b'x'*64


def catalog():
    return {'version':1,'bucket':'owned-backups','prefix':'cluster-a/','receipts':[{'created_at':f'2026-0{month}-01T00:00:00Z','receipt':{'operation':'backup','success':True,'object':f'cluster-a/{month}.hmb','version_id':f'version-{month}','encrypted_bytes':len(BODY),'sha256':hashlib.sha256(BODY).hexdigest()}} for month in [6,7,8,9]]}


class Missing(Exception):
    response={'Error':{'Code':'NoSuchVersion'}}


class Store:
    def __init__(self):self.deleted=[];self.read=[];self.wrong=None;self.missing=set();self.fail_delete=None
    def get_object(self,**request):
        version=request['VersionId'];self.read.append(version)
        if version in self.missing:raise Missing()
        body=BODY if self.wrong!=version else b'z'*len(BODY)
        return {'VersionId':version,'ContentLength':len(body),'Body':io.BytesIO(body)}
    def delete_object(self,**request):
        self.deleted.append(request)
        if request['VersionId']==self.fail_delete:raise TimeoutError('sensitive endpoint detail')
        return {'VersionId':request['VersionId']}


class Retention(unittest.TestCase):
    def planned(self,value=None):return retention.plan(value or catalog(),1,30,NOW)
    def test_newest_age_and_pin(self):
        value=catalog();value['receipts'][0]['pinned']=True
        planned=self.planned(value)
        self.assertEqual({r['version_id'] for r in planned['retained']},{'version-6','version-9'})
        self.assertEqual({r['version_id'] for r in planned['expired']},{'version-7','version-8'})
    def test_no_mutation_to_catalog(self):
        value=catalog();before=copy.deepcopy(value);self.planned(value);self.assertEqual(value,before)
    def test_scope_and_receipt_refusals(self):
        cases=[('version_id','null'),('version_id',''),('object','cluster-ab/unrelated'),('success',False),('sha256','x'*64),('encrypted_bytes',True),('upload_confirmed',False)]
        for field,bad in cases:
            value=catalog();value['receipts'][0]['receipt'][field]=bad
            with self.subTest(field=field,bad=bad),self.assertRaises(ValueError):self.planned(value)
    def test_bad_policy_time_duplicates(self):
        for keep,days in [(0,30),(True,30),(1,0)]:
            with self.subTest(keep=keep,days=days),self.assertRaises(ValueError):retention.plan(catalog(),keep,days,NOW)
        for time in ['2027-01-01T00:00:00Z','2026-01-01']:
            value=catalog();value['receipts'][0]['created_at']=time
            with self.assertRaises(ValueError):self.planned(value)
        value=catalog();value['receipts'].append(copy.deepcopy(value['receipts'][0]))
        with self.assertRaises(ValueError):self.planned(value)
    def test_exact_version_deletes_after_all_preflight(self):
        store=Store();planned=self.planned();result=retention.apply(store,planned)
        self.assertTrue(result['success']);self.assertEqual(len(store.read),4)
        self.assertEqual({r['VersionId'] for r in store.deleted},{'version-6','version-7','version-8'})
        self.assertTrue(all(set(r)=={'Bucket','Key','VersionId'} for r in store.deleted))
    def test_corrupted_retained_or_expired_blocks_all_deletion(self):
        for version in ['version-9','version-6']:
            store=Store();store.wrong=version
            with self.subTest(version=version),self.assertRaises(ValueError):retention.apply(store,self.planned())
            self.assertEqual(store.deleted,[])
    def test_missing_kept_blocks_missing_expired_is_idempotent(self):
        store=Store();store.missing={'version-9'}
        with self.assertRaises(Missing):retention.apply(store,self.planned())
        self.assertEqual(store.deleted,[])
        store=Store();store.missing={'version-6'};result=retention.apply(store,self.planned())
        self.assertTrue(result['success']);self.assertEqual(result['already_absent'][0]['version_id'],'version-6')
    def test_uncertain_delete_stops_and_retains_progress(self):
        store=Store();store.fail_delete='version-7';result=retention.apply(store,self.planned())
        self.assertFalse(result['success']);self.assertEqual(result['error'],'TimeoutError')
        self.assertEqual(result['delete_unconfirmed']['version_id'],'version-7')
        self.assertEqual([r['VersionId'] for r in store.deleted],['version-8','version-7'])
        self.assertEqual(result['deleted'][0]['version_id'],'version-8')
    def test_plan_digest_changes_with_pin_and_policy(self):
        value=catalog();first=self.planned(value);value['receipts'][0]['pinned']=True
        self.assertNotEqual(first['plan_sha256'],self.planned(value)['plan_sha256'])
        self.assertNotEqual(first['plan_sha256'],retention.plan(catalog(),2,30,NOW)['plan_sha256'])
    def test_interrupt_retains_uncertain_version(self):
        store=Store()
        def interrupted(**request):raise KeyboardInterrupt()
        store.delete_object=interrupted;result=retention.apply(store,self.planned())
        self.assertFalse(result['success']);self.assertTrue(result['interrupted'])
        self.assertEqual(result['delete_unconfirmed']['version_id'],'version-8')
    def test_cli_plans_without_storage_access(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'catalog.json';path.write_text(json.dumps(catalog()))
            result=subprocess.run([sys.executable,str(Path(__file__).with_name('retain-object-backups.py')),'--catalog',str(path),'--keep-newest','1','--older-than-days','30','--as-of','2026-10-02T00:00:00Z'],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(json.loads(result.stdout)['operation'],'retention_plan')
    def test_cli_changed_review_refuses_before_storage_access(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'catalog.json';path.write_text(json.dumps(catalog()))
            result=subprocess.run([sys.executable,str(Path(__file__).with_name('retain-object-backups.py')),'--catalog',str(path),'--keep-newest','1','--older-than-days','30','--as-of','2026-10-02T00:00:00Z','--apply','--plan-sha256','0'*64],capture_output=True,text=True)
            self.assertEqual(result.returncode,1)
            self.assertEqual(json.loads(result.stderr)['error'],'reviewed plan changed')
    def test_wrong_response_identity_blocks_delete(self):
        for field,value in [('VersionId','wrong'),('ContentLength',999),('DeleteMarker',True)]:
            store=Store();original=store.get_object
            def response(**request):
                result=original(**request);result[field]=value;return result
            store.get_object=response
            with self.subTest(field=field),self.assertRaises(ValueError):retention.apply(store,self.planned())
            self.assertEqual(store.deleted,[])
    def test_wrong_delete_ack_is_uncertain_and_stops(self):
        store=Store()
        def mismatch(**request):
            store.deleted.append(request);return {'VersionId':'wrong'}
        store.delete_object=mismatch;result=retention.apply(store,self.planned())
        self.assertFalse(result['success']);self.assertEqual(len(store.deleted),1)
        self.assertEqual(result['delete_unconfirmed']['version_id'],'version-8')


if __name__=='__main__':unittest.main()
