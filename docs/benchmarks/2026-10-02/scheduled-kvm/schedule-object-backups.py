#!/usr/bin/env python3
"""Durable one-shot scheduling for offline, versioned S3 snapshot backups."""
import argparse
import contextlib
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import tempfile
from types import SimpleNamespace
import uuid


def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module


retention=load('scheduled_retention',Path(__file__).with_name('retain-object-backups.py'))
require=retention.require
DEFAULTS={'region':'us-east-1','endpoint':None,'work_dir':None,'compression_level':1,'max_expanded_bytes':64*1024**3,'multipart_threshold_mib':64,'multipart_part_mib':64}
REQUIRED={'start_at','interval_seconds','store','bucket','prefix','key_file'}


def configuration(raw):
    require(isinstance(raw,dict) and REQUIRED<=raw.keys() and raw.keys()<=REQUIRED|DEFAULTS.keys(),'invalid schedule configuration fields')
    result={**DEFAULTS,**raw};result['start_at']=retention.instant(result['start_at']).isoformat()
    for name,low,high in [('interval_seconds',1,365*86400),('compression_level',1,9),('max_expanded_bytes',1,64*1024**3),('multipart_threshold_mib',1,4096),('multipart_part_mib',8,128)]:
        require(type(result[name]) is int and low<=result[name]<=high,'invalid schedule '+name)
    for name in ['store','key_file']:require(isinstance(result[name],str) and PurePosixPath(result[name]).is_absolute(),'schedule paths must be absolute')
    require(isinstance(result['bucket'],str) and retention.re.fullmatch(r'[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]',result['bucket']) is not None,'invalid schedule bucket')
    require(isinstance(result['prefix'],str) and result['prefix'].endswith('/'),'schedule prefix must end in slash');retention.key(result['prefix'][:-1])
    if result['work_dir'] is not None:require(isinstance(result['work_dir'],str) and PurePosixPath(result['work_dir']).is_absolute(),'work directory must be absolute')
    require(isinstance(result['region'],str) and result['region'],'invalid schedule region')
    if result['endpoint'] is not None:
        from urllib.parse import urlsplit
        endpoint=urlsplit(result['endpoint']);require(endpoint.scheme=='https' or (endpoint.scheme=='http' and endpoint.hostname in ['127.0.0.1','localhost','::1']),'endpoint requires HTTPS except loopback')
        require(endpoint.username is None and endpoint.password is None and not endpoint.query and not endpoint.fragment,'invalid schedule endpoint')
    return result


def new_state(config):return {'version':1,'configuration_sha256':hashlib.sha256(retention.canonical(config)).hexdigest(),'last_slot':None,'current':None,'receipts':[],'skipped_attempts':[]}


def due(config,state,now):
    expected=new_state(config)['configuration_sha256']
    require(isinstance(state,dict) and set(state)=={'version','configuration_sha256','last_slot','current','receipts','skipped_attempts'} and type(state['version']) is int and state['version']==1 and state['configuration_sha256']==expected,'schedule configuration or journal schema changed')
    require(isinstance(state['receipts'],list) and len(state['receipts'])<=10000,'receipt history exceeds limit')
    require(isinstance(state['skipped_attempts'],list) and len(state['receipts'])+len(state['skipped_attempts'])<=10000,'schedule history exceeds limit')
    last=state['last_slot'];require(last is None or (type(last) is int and last>=0),'invalid schedule cursor')
    current=state['current']
    if current is not None:
        require(isinstance(current,dict) and current['phase'] in ['capturing','upload_pending','uncertain','failed','confirmed','skipped'] and type(current['slot']) is int and current['slot']>=0,'invalid current capture')
        require(retention.key(current['object']).startswith(config['prefix']),'capture object outside scope');retention.instant(current['created_at'])
        if current['phase'] in ['upload_pending','uncertain']:
            receipt=current['attempt_receipt'];require(receipt['operation']=='backup' and receipt['object']==current['object'] and type(receipt['encrypted_bytes']) is int and 36<=receipt['encrypted_bytes']<=retention.MAX_OBJECT and retention.re.fullmatch('[0-9a-f]{64}',receipt['sha256']) is not None,'invalid pending ciphertext identity')
        if current['phase'] in ['confirmed','skipped']:require(last==current['slot'],'completed capture cursor differs')
    if state['receipts']:retention.plan(catalog(config,state),1,1,now)
    elapsed=(now-retention.instant(config['start_at'])).total_seconds()
    if elapsed<0:return None
    slot=int(elapsed//config['interval_seconds'])
    return slot if last is None or slot>last else None


def catalog(config,state):return {'version':1,'bucket':config['bucket'],'prefix':config['prefix'],'receipts':state['receipts']}


def confirmed(config,state,receipt,persist):
    current=state['current'];require(current is not None and receipt['object']==current['object'],'confirmation object differs')
    require(receipt.get('version_id') not in [None,'null'],'confirmed scheduled backup requires non-null version')
    item={'created_at':current['created_at'],'pinned':False,'receipt':{**receipt,'success':True,'upload_confirmed':True}}
    candidate={**state,'last_slot':current['slot'],'current':{**current,'phase':'confirmed','receipt':item['receipt']},'receipts':state['receipts']+[item]}
    # Apply the retention validator to every registered identity before commit.
    retention.plan(catalog(config,candidate),1,1,retention.instant(current['created_at']))
    persist(candidate);state.clear();state.update(candidate)
    return {'success':True,'status':'confirmed','slot':current['slot'],'receipt':item['receipt']}


def run_once(config,state,now,persist,capture):
    slot=due(config,state,now)
    if state['current'] and state['current']['phase'] in ['upload_pending','uncertain']:
        return {'success':False,'status':'reconciliation_required','current':state['current']}
    if slot is None:return {'success':True,'status':'not_due','last_slot':state['last_slot']}
    require(len(state['receipts'])+len(state['skipped_attempts'])<10000,'archive schedule history before more captures')
    current={'slot':slot,'object':config['prefix']+str(slot)+'-'+uuid.uuid4().hex+'.hmb','created_at':now.isoformat(),'phase':'capturing'}
    candidate={**state,'current':current};persist(candidate);state.clear();state.update(candidate)
    def sink(receipt):
        require(receipt['object']==current['object'],'attempt object differs')
        pending={**state,'current':{**current,'phase':'upload_pending','attempt_receipt':receipt}}
        persist(pending);state.clear();state.update(pending)
    args=SimpleNamespace(**{key:value for key,value in config.items() if key in DEFAULTS or key in ['store','bucket','key_file']},object=current['object'])
    for name in ['store','key_file','work_dir']:
        if getattr(args,name) is not None:setattr(args,name,Path(getattr(args,name)))
    receipt=None
    try:
        receipt=capture(args,sink)
        require(state['current']['phase']=='upload_pending','capture did not record upload intent')
        return confirmed(config,state,receipt,persist)
    except (Exception,KeyboardInterrupt) as error:
        uncertain=state['current']['phase']=='upload_pending'
        failed={**state,'current':{**state['current'],'phase':'uncertain' if uncertain else 'failed','error':str(error) if type(error) is ValueError else type(error).__name__}}
        if hasattr(error,'receipt'):failed['current']['attempt_receipt']=error.receipt
        if receipt is not None:failed['current']['acknowledged_receipt']=receipt
        if hasattr(error,'cleanup'):failed['current']['multipart_cleanup']=error.cleanup
        try:persist(failed)
        except Exception as write_error:
            return {'success':False,'status':'journal_write_failed','current':failed['current'],'journal_error':type(write_error).__name__,'interrupted':isinstance(error,KeyboardInterrupt)}
        state.clear();state.update(failed)
        return {'success':False,'status':'reconciliation_required' if uncertain else 'capture_failed','current':state['current'],'interrupted':isinstance(error,KeyboardInterrupt)}


def skip_attempt(state,sha,reason,persist):
    current=state['current'];require(current is not None and current['phase'] in ['upload_pending','uncertain'],'skip requires uncertain upload')
    require(sha==current['attempt_receipt']['sha256'] and reason in ['object_absence_verified','backup_unrecoverable'],'skip requires exact reviewed attempt and operator decision')
    skipped={**current,'phase':'skipped','operator_reason':reason}
    candidate={**state,'last_slot':current['slot'],'current':skipped,'skipped_attempts':state['skipped_attempts']+[skipped]}
    persist(candidate);state.clear();state.update(candidate)
    return {'success':True,'status':'skipped','current':skipped,'storage_deleted':False,'absence_automatically_verified':False}


def set_pin(state,object_key,version,pinned,persist):
    matches=[item for item in state['receipts'] if item['receipt']['object']==object_key and item['receipt']['version_id']==version]
    require(len(matches)==1,'pin requires one exact registered object version')
    candidate={**state,'receipts':[{**item,'pinned':pinned} if item is matches[0] else item for item in state['receipts']]}
    persist(candidate);state.clear();state.update(candidate)
    return {'success':True,'status':'pinned' if pinned else 'unpinned','object':object_key,'version_id':version}


def atomic_json(root,name,value):
    path=root/name;require(not path.is_symlink(),'state file is a symlink')
    fd,temp=tempfile.mkstemp(prefix='.state-',dir=root)
    try:
        with os.fdopen(fd,'wb') as stream:stream.write(retention.canonical(value));stream.flush();os.fsync(stream.fileno())
        os.replace(temp,path)
        directory=os.open(root,os.O_RDONLY|os.O_DIRECTORY)
        try:os.fsync(directory)
        finally:os.close(directory)
    finally:
        if os.path.exists(temp):os.unlink(temp)


@contextlib.contextmanager
def locked_state(path,config):
    import fcntl
    require(not path.is_symlink(),'state directory is a symlink')
    require(not path.resolve().is_relative_to(Path(config['store']).resolve()),'schedule state must be outside store')
    parent=path.parent.resolve(strict=True)
    path.mkdir(mode=0o700,exist_ok=True);root=path.resolve(strict=True)
    info=root.stat();require(info.st_uid==os.getuid() and stat.S_IMODE(info.st_mode)&0o077==0,'state directory must be private and operator-owned')
    require(not root.is_relative_to(Path(config['store']).resolve()),'schedule state must be outside store')
    parent_fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY)
    try:os.fsync(parent_fd)
    finally:os.close(parent_fd)
    fd=os.open(root/'.schedule.lock',os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    try:
        require(stat.S_ISREG(os.fstat(fd).st_mode),'schedule lock is not regular');fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB);yield root
    finally:os.close(fd)


def read_json(path):
    require(not path.is_symlink(),'JSON path is a symlink')
    with path.open('rb') as stream:data=stream.read(32*1024*1024+1)
    require(len(data)<=32*1024*1024,'schedule JSON oversized');return json.loads(data,object_pairs_hook=retention.unique)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('command',choices=['run','status','confirm','skip','pin','unpin','retain']);parser.add_argument('--config',type=Path,required=True);parser.add_argument('--state',type=Path,required=True);parser.add_argument('--object');parser.add_argument('--version-id');parser.add_argument('--attempt-sha256');parser.add_argument('--reason',choices=['object_absence_verified','backup_unrecoverable']);parser.add_argument('--keep-newest',type=int);parser.add_argument('--older-than-days',type=int);parser.add_argument('--as-of');parser.add_argument('--apply',action='store_true');parser.add_argument('--plan-sha256');args=parser.parse_args()
    try:
        require(sys.platform=='linux','scheduled backup runner requires Linux locking and offline store protocol');os.umask(0o077)
        require(args.command=='retain' or not any([args.apply,args.plan_sha256,args.as_of,args.keep_newest is not None,args.older_than_days is not None]),'retention options require retain command')
        config=configuration(read_json(args.config));now=datetime.now(timezone.utc)
        with locked_state(args.state,config) as root:
            journal=root/'journal.json'
            if not journal.exists():require(not (root/'catalog.json').exists() and not (root/'catalog.json').is_symlink(),'unmanaged catalog exists without schedule journal')
            state=read_json(journal) if journal.exists() else new_state(config);slot=due(config,state,now)
            def persist(value):atomic_json(root,'journal.json',value)
            if not journal.exists():persist(state)
            # Journal is authoritative; repair a catalog publication interrupted
            # after a confirmed journal commit before considering another slot.
            atomic_json(root,'catalog.json',catalog(config,state))
            if args.command=='status':result={'success':True,'status':'status','due_slot':slot,'last_slot':state['last_slot'],'current':state['current'],'confirmed_backups':len(state['receipts'])}
            elif args.command=='retain':
                evaluation=retention.instant(args.as_of) if args.as_of else now
                planned=retention.plan(catalog(config,state),args.keep_newest,args.older_than_days,evaluation)
                if args.plan_sha256:require(args.plan_sha256==planned['plan_sha256'],'reviewed plan changed')
                # The state lock remains held through preflight and deletion,
                # serializing capture, pin changes, and catalog publication.
                result=retention.apply(retention.client(config['endpoint'],config['region']),planned) if args.apply else {'success':True,'operation':'retention_plan',**planned}
            elif args.command=='skip':result=skip_attempt(state,args.attempt_sha256,args.reason,persist)
            elif args.command in ['pin','unpin']:result=set_pin(state,args.object,args.version_id,args.command=='pin',persist)
            elif args.command=='confirm':
                require(args.version_id not in [None,'null'] and state['current'] is not None and state['current']['phase'] in ['upload_pending','uncertain'],'confirmation requires uncertain attempt and exact version')
                receipt={**state['current']['attempt_receipt'],'version_id':args.version_id};retention.verify_object(retention.client(config['endpoint'],config['region']),config['bucket'],receipt,False)
                result=confirmed(config,state,receipt,persist)
            else:
                backup=load('scheduled_offline_backup',Path(__file__).with_name('backup-snapshot-store.py'))
                def capture(options,sink):
                    storage=backup.client(options.endpoint,options.region)
                    require(storage.get_bucket_versioning(Bucket=options.bucket).get('Status')=='Enabled','scheduled capture requires versioned bucket')
                    return backup.backup(options,receipt_sink=sink)
                result=run_once(config,state,now,persist,capture)
            try:atomic_json(root,'catalog.json',catalog(config,state))
            except Exception as error:
                result={'success':False,'status':'catalog_publish_failed','backup_result':result,'error':type(error).__name__}
        print(json.dumps(result,indent=2));return 0 if result['success'] else (130 if result.get('interrupted') else 1)
    except Exception as error:
        print(json.dumps({'success':False,'error':str(error) if type(error) is ValueError else type(error).__name__}),file=sys.stderr);return 1


if __name__=='__main__':raise SystemExit(main())
