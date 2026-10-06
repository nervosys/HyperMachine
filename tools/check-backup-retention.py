#!/usr/bin/env python3
"""Verify receipt-scoped retention with an owned Moto versioned S3 fixture."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    if args.output.exists():raise ValueError('output exists; retain prior evidence')
    import boto3,moto
    tool=Path(__file__).with_name('retain-object-backups.py');spec=importlib.util.spec_from_file_location('owned_retention',tool);retention=importlib.util.module_from_spec(spec);spec.loader.exec_module(retention)
    report={'success':False,'storage':'owned in-process Moto S3 emulator','managed_storage_verified':False,'artifact_sha256':{'tool':hashlib.sha256(tool.read_bytes()).hexdigest(),'coordinator':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},'versions':{'boto3':boto3.__version__,'moto':moto.__version__},'checks':[]}
    with moto.mock_aws():
        s3=boto3.client('s3',region_name='us-east-1',aws_access_key_id='owned-fixture',aws_secret_access_key='owned-fixture')
        bucket='hm-owned-retention-fixture';s3.create_bucket(Bucket=bucket);s3.put_bucket_versioning(Bucket=bucket,VersioningConfiguration={'Status':'Enabled'})
        catalog={'version':1,'bucket':bucket,'prefix':'cluster-a/','receipts':[]}
        for index,month in enumerate([6,7,8,9]):
            name='cluster-a/reused.hmb' if index<3 else 'cluster-a/newest.hmb';body=b'HMBACK01'+bytes([index])*64
            version=s3.put_object(Bucket=bucket,Key=name,Body=body)['VersionId']
            catalog['receipts'].append({'created_at':f'2026-0{month}-01T00:00:00Z','pinned':index==0,'receipt':{'operation':'backup','success':True,'object':name,'version_id':version,'encrypted_bytes':len(body),'sha256':hashlib.sha256(body).hexdigest()}})
        unknown=s3.put_object(Bucket=bucket,Key='cluster-a/reused.hmb',Body=b'unregistered-new-version')['VersionId']
        marker=s3.delete_object(Bucket=bucket,Key='cluster-a/reused.hmb')['VersionId']
        s3.put_object(Bucket=bucket,Key='cluster-ab/outside',Body=b'outside')
        planned=retention.plan(catalog,1,30,datetime(2026,10,2,tzinfo=timezone.utc));retention.require(len(planned['retained'])==2 and len(planned['expired'])==2,'policy selection differs')
        report['checks'].append('newest_and_pin_retained')
        applied=retention.apply(s3,planned);retention.require(applied['success'] and len(applied['deleted'])==2,'exact-version application failed');report['checks'].append('expired_versions_deleted')
        listed=s3.list_object_versions(Bucket=bucket)
        versions={v['VersionId'] for v in listed.get('Versions',[])};markers={v['VersionId'] for v in listed.get('DeleteMarkers',[])}
        retention.require(unknown in versions and marker in markers,'unregistered version/delete marker changed');report['checks'].append('unregistered_version_and_marker_preserved')
        for row in planned['retained']:retention.verify_object(s3,bucket,row,False)
        report['checks'].append('retained_ciphertext_checksums_verified')
        retention.require(s3.get_object(Bucket=bucket,Key='cluster-ab/outside')['Body'].read()==b'outside','outside scope changed');report['checks'].append('adjacent_prefix_preserved')
        repeated=retention.apply(s3,planned);retention.require(repeated['success'] and len(repeated['already_absent'])==2 and not repeated['deleted'],'repeat application not idempotent');report['checks'].append('repeat_application_idempotent')
        report['success']=True;report['result']=applied
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'success':report['success'],'checks':report['checks']}))


if __name__=='__main__':main()
