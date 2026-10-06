#!/usr/bin/env python3
"""Exercise scheduled CLI capture, uncertain upload and recovery on owned S3."""
import argparse
import contextlib
from datetime import datetime, timedelta, timezone
import fcntl
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    if args.output.exists():raise ValueError('output exists; preserve prior evidence')
    import boto3,moto,cryptography
    tools=Path(__file__).parent;schedule=load('scheduled_owned_fixture',tools/'schedule-object-backups.py');require=schedule.require
    files={n:tools/f for n,f in [('runner','schedule-object-backups.py'),('backup','backup-snapshot-store.py'),('retention','retain-object-backups.py'),('coordinator','check-scheduled-backups.py')]}
    report={'success':False,'storage':'owned in-process Moto S3 emulator','native_platform':'Linux','kvm_test':False,'managed_storage_verified':False,'versions':{'boto3':boto3.__version__,'moto':moto.__version__,'cryptography':cryptography.__version__},'artifact_sha256':{n:hashlib.sha256(p.read_bytes()).hexdigest() for n,p in files.items()},'checks':[]}
    os.umask(0o077)
    with moto.mock_aws(),tempfile.TemporaryDirectory(prefix='hm-scheduled-backup-',dir='/var/tmp') as scratch:
        root=Path(scratch);store=root/'store';store.mkdir();(store/'.backup.lock').write_bytes(b'');original=b'owned scheduled backup state\n'*200;(store/'state.bin').write_bytes(original)
        key=root/'key';key.write_text('42'*32);state=root/'runner';configuration=root/'configuration.json';bucket='hm-owned-scheduled-backups'
        clock=[datetime(2026,10,2,tzinfo=timezone.utc)]
        class Clock(datetime):
            @classmethod
            def now(cls,tz=None):return clock[0]
        schedule.datetime=Clock
        configuration.write_text(json.dumps({'start_at':clock[0].isoformat(),'interval_seconds':60,'store':str(store),'key_file':str(key),'bucket':bucket,'prefix':'cluster-a/'}))
        client=boto3.client('s3',region_name='us-east-1',aws_access_key_id='owned-fixture',aws_secret_access_key='owned-fixture');client.create_bucket(Bucket=bucket);client.put_bucket_versioning(Bucket=bucket,VersioningConfiguration={'Status':'Enabled'})
        def invoke(command='run',extra=(),success=True):
            before=sys.argv;stdout=io.StringIO();stderr=io.StringIO()
            try:
                sys.argv=['schedule-object-backups.py',command,'--config',str(configuration),'--state',str(state),*extra]
                with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):code=schedule.main()
            finally:sys.argv=before
            value=json.loads(stdout.getvalue() or stderr.getvalue());require((code==0)==success,'CLI result differs');return value
        first=invoke();require(first['status']=='confirmed','first capture not confirmed');report['checks'].append('scheduled_encrypted_capture_confirmed')
        require(invoke()['status']=='not_due','same slot uploaded twice');report['checks'].append('same_slot_restart_no_duplicate')
        clock[0]+=timedelta(minutes=3);require(invoke()['slot']==3,'missed intervals not coalesced');report['checks'].append('missed_slots_coalesced')
        fd=os.open(store/'.backup.lock',os.O_RDWR);fcntl.flock(fd,fcntl.LOCK_SH);clock[0]+=timedelta(minutes=1)
        try:require(invoke(success=False)['status']=='capture_failed','live-store lock ignored')
        finally:os.close(fd)
        require(len(client.list_object_versions(Bucket=bucket).get('Versions',[]))==2,'busy store uploaded an object');report['checks'].append('live_store_refused_without_upload')
        require(invoke()['status']=='confirmed','known local failure did not retry');report['checks'].append('offline_retry_confirmed')
        original_load=schedule.load;lost_versions=[]
        def injected_load(name,path):
            module=original_load(name,path)
            if name=='scheduled_offline_backup':
                upload=module.upload_ciphertext
                def uncertain(storage,stream,*arguments):
                    class LostReply:
                        def put_object(self,**request):
                            result=storage.put_object(**request);lost_versions.append(result['VersionId']);raise TimeoutError('owned simulated lost reply')
                    return upload(LostReply(),stream,*arguments)
                module.upload_ciphertext=uncertain
            return module
        schedule.load=injected_load;clock[0]+=timedelta(minutes=1)
        try:uncertain=invoke(success=False)
        finally:schedule.load=original_load
        require(uncertain['status']=='reconciliation_required' and uncertain['current']['attempt_receipt'].get('version_id') is None,'uncertain identity not preserved');report['checks'].append('lost_upload_reply_journaled')
        clock[0]+=timedelta(hours=1);require(invoke(success=False)['status']=='reconciliation_required','uncertain upload retried');require(len(client.list_object_versions(Bucket=bucket)['Versions'])==4,'blocked schedule uploaded');report['checks'].append('uncertainty_blocks_later_slots')
        name=uncertain['current']['object'];substitute=client.put_object(Bucket=bucket,Key=name,Body=b'unregistered-substitute')['VersionId']
        invoke('confirm',('--version-id',substitute),success=False);require(invoke('status')['current']['phase']=='uncertain','wrong version confirmation advanced state');report['checks'].append('substituted_version_refused')
        result=invoke('confirm',('--version-id',lost_versions[0]));require(result['status']=='confirmed' and result['slot']==5,'version-pinned reconciliation failed');report['checks'].append('exact_version_checksum_reconciliation')
        registered=schedule.read_json(state/'catalog.json');require(len(registered['receipts'])==4,'confirmed catalog registration differs');schedule.retention.plan(registered,2,30,clock[0]);report['checks'].append('retention_catalog_registered')
        backup=original_load('owned_restore',tools/'backup-snapshot-store.py');destination=root/'recovered';receipt=result['receipt']
        from types import SimpleNamespace
        recovered=backup.restore(SimpleNamespace(destination=destination,key_file=key,endpoint=None,region='us-east-1',bucket=bucket,object=name,version_id=receipt['version_id'],sha256=receipt['sha256'],work_dir=None,max_expanded_bytes=64*1024**3))
        require(recovered['version_pinned'] and (destination/'state.bin').read_bytes()==original,'scheduled ciphertext recovery differs');report['checks'].append('encrypted_version_pinned_restore_verified')
        schedule.load=injected_load;clock[0]+=timedelta(minutes=1)
        try:abandoned=invoke(success=False)
        finally:schedule.load=original_load
        attempt=abandoned['current']['attempt_receipt'];skipped=invoke('skip',('--attempt-sha256',attempt['sha256'],'--reason','backup_unrecoverable'))
        require(skipped['status']=='skipped' and skipped['storage_deleted'] is False,'explicit skip altered storage')
        schedule.retention.verify_object(client,bucket,{**attempt,'version_id':lost_versions[-1]},False);report['checks'].append('explicit_skip_preserves_unconfirmed_object')
        clock[0]+=timedelta(minutes=1);require(invoke()['status']=='confirmed','skip did not permit later capture')
        journal=schedule.read_json(state/'journal.json');require(len(journal['skipped_attempts'])==1 and journal['skipped_attempts'][0]['attempt_receipt']['sha256']==attempt['sha256'],'skipped receipt history lost');report['checks'].append('skipped_receipt_history_survives_next_capture')
        schedule.atomic_json(state,'catalog.json',{'version':1,'bucket':bucket,'prefix':'cluster-a/','receipts':[]})
        require(invoke('status')['confirmed_backups']==5 and len(schedule.read_json(state/'catalog.json')['receipts'])==5,'catalog repair differs');report['checks'].append('catalog_repaired_from_committed_journal')
        identity=first['receipt'];invoke('pin',('--object',identity['object'],'--version-id',identity['version_id']))
        invoke('status');require(schedule.read_json(state/'catalog.json')['receipts'][0]['pinned'] is True,'pin lost during catalog publication');report['checks'].append('exact_version_pin_preserved_during_catalog_repair')
        invoke('unpin',('--object',identity['object'],'--version-id',identity['version_id']));require(schedule.read_json(state/'catalog.json')['receipts'][0]['pinned'] is False,'explicit unpin failed');report['checks'].append('explicit_unpin_verified')
        # Exercise the scheduler-produced catalog through actual version deletion.
        invoke('pin',('--object',identity['object'],'--version-id',identity['version_id']))
        clock[0]+=timedelta(days=2)
        registered=schedule.read_json(state/'catalog.json')
        planned=schedule.retention.plan(registered,1,1,clock[0])
        require(len(planned['retained'])==2 and len(planned['expired'])==3,'scheduled retention policy differs')
        retention_options=('--keep-newest','1','--older-than-days','1','--as-of',clock[0].isoformat())
        original_client=schedule.retention.client
        def no_network(*args):raise ValueError('unexpected storage access during offline plan')
        schedule.retention.client=no_network
        try:
            reviewed=invoke('retain',retention_options)
            require(reviewed['plan_sha256']==planned['plan_sha256'],'locked CLI plan differs')
            invoke('retain',(*retention_options,'--apply','--plan-sha256','0'*64),success=False)
        finally:schedule.retention.client=original_client
        report['checks'].extend(['locked_retention_plan_offline','changed_retention_plan_refused_before_storage'])
        with schedule.locked_state(state,schedule.configuration(schedule.read_json(configuration))):
            invoke('retain',(*retention_options,'--apply'),success=False)
        require(len(client.list_object_versions(Bucket=bucket)['Versions'])==7,'contended retention altered storage')
        report['checks'].append('retention_lock_contention_refused')
        pruned=invoke('retain',(*retention_options,'--apply','--plan-sha256',reviewed['plan_sha256']))
        repeated=invoke('retain',(*retention_options,'--apply','--plan-sha256',reviewed['plan_sha256']))
        require(pruned['success'] and len(pruned['deleted'])==3,'scheduled catalog pruning failed')
        require(repeated['success'] and len(repeated['already_absent'])==3 and not repeated['deleted'],'retention repeat not idempotent')
        report['checks'].extend(['scheduled_catalog_exact_version_pruning','scheduled_retention_repeat_idempotent'])
        pinned_destination=root/'pinned-recovery'
        backup.restore(SimpleNamespace(destination=pinned_destination,key_file=key,endpoint=None,region='us-east-1',bucket=bucket,object=identity['object'],version_id=identity['version_id'],sha256=identity['sha256'],work_dir=None,max_expanded_bytes=64*1024**3))
        require((pinned_destination/'state.bin').read_bytes()==original,'pinned recovery after pruning differs')
        report['checks'].append('pinned_encrypted_restore_after_retention')
        schedule.retention.verify_object(client,bucket,{**attempt,'version_id':lost_versions[-1]},False)
        require(client.get_object(Bucket=bucket,Key=name,VersionId=substitute)['Body'].read()==b'unregistered-substitute','retention deleted unregistered version')
        report['checks'].append('retention_preserves_unregistered_and_uncertain_versions')
        require(invoke()['status']=='confirmed','capture after retention failed')
        invoke('status')
        registered=schedule.read_json(state/'catalog.json')
        require(len(registered['receipts'])==6 and registered['receipts'][0]['pinned'],'journal repair lost history or pin after pruning')
        planned=schedule.retention.plan(registered,1,1,clock[0])
        subsequent=invoke('retain',(*retention_options,'--apply','--plan-sha256',planned['plan_sha256']))
        require(subsequent['success'] and len(subsequent['deleted'])==1 and len(subsequent['already_absent'])==3,'retention after next capture differs')
        report['checks'].append('capture_and_retention_continue_after_pruning')
        remaining=client.list_object_versions(Bucket=bucket).get('Versions',[])
        require(len(remaining)==4,'unexpected retained version inventory')
        report['retention_deleted_versions']=4;report['retention_already_absent_verified']=3
        report['success']=True;report['confirmed_backups']=6;report['retention_receipts']=6;report['known_object_versions']=len(remaining)
    report['artifacts_unchanged']=all(hashlib.sha256(path.read_bytes()).hexdigest()==report['artifact_sha256'][name] for name,path in files.items());require(report['artifacts_unchanged'],'fixture sources changed')
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'success':True,'checks':report['checks']}))


if __name__=='__main__':main()
