#!/usr/bin/env python3
"""Plan or apply receipt-driven retention of exact versioned S3 backups."""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import re
import sys

MAX_OBJECT=64*1024**3


def require(value,message):
    if not value:raise ValueError(message)


def unique(pairs):
    result={}
    for key,value in pairs:
        require(key not in result,'duplicate JSON field');result[key]=value
    return result


def canonical(value):return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()


def instant(value):
    require(isinstance(value,str) and len(value)<=64,'invalid receipt time')
    parsed=datetime.fromisoformat(value.replace('Z','+00:00'))
    require(parsed.tzinfo is not None,'receipt time requires timezone')
    return parsed.astimezone(timezone.utc)


def key(value):
    require(isinstance(value,str) and 0<len(value.encode())<=1024 and not value.startswith('/') and '\\' not in value and all(part not in ['', '.', '..'] for part in value.split('/')) and all(ord(c)>=32 and ord(c)!=127 for c in value),'invalid scoped object key')
    return value


def plan(catalog,keep,days,now):
    require(type(keep) is int and 1<=keep<=10000 and type(days) is int and 1<=days<=36500,'invalid retention policy')
    require(catalog['version']==1 and isinstance(catalog['bucket'],str) and re.fullmatch(r'[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]',catalog['bucket']) is not None,'invalid retention bucket/catalog')
    prefix=catalog['prefix'];require(isinstance(prefix,str) and prefix.endswith('/'),'retention prefix must end in slash');key(prefix[:-1])
    receipts=catalog['receipts'];require(isinstance(receipts,list) and 1<=len(receipts)<=10000,'receipt count out of bounds')
    rows=[];seen=set()
    for item in receipts:
        receipt=item['receipt'];require(receipt.get('success') is True and receipt.get('operation')=='backup' and receipt.get('upload_confirmed',True) is True,'unconfirmed backup receipt')
        name=key(receipt['object']);require(name.startswith(prefix),'receipt outside retention scope')
        version=receipt['version_id'];require(isinstance(version,str) and 0<len(version.encode())<=4096 and version!='null' and all(ord(c)>=32 and ord(c)!=127 for c in version),'requires immutable non-null version ID')
        require((name,version) not in seen,'duplicate object version');seen.add((name,version))
        require(type(receipt['encrypted_bytes']) is int and 36<=receipt['encrypted_bytes']<=MAX_OBJECT and isinstance(receipt['sha256'],str) and re.fullmatch('[0-9a-f]{64}',receipt['sha256']) is not None,'invalid receipt ciphertext identity')
        created=instant(item['created_at']);require(created<=now,'future receipt time')
        pinned=item.get('pinned',False);require(type(pinned) is bool,'invalid pin')
        rows.append({'object':name,'version_id':version,'encrypted_bytes':receipt['encrypted_bytes'],'sha256':receipt['sha256'],'created_at':created.isoformat(),'pinned':pinned})
    rows.sort(key=lambda row:(row['created_at'],row['object'],row['version_id']),reverse=True)
    cutoff=now-timedelta(days=days);retained=[];expired=[]
    for index,row in enumerate(rows):
        reasons=[]
        if index<keep:reasons.append('newest')
        if row['pinned']:reasons.append('pinned')
        if instant(row['created_at'])>=cutoff:reasons.append('minimum_age')
        (retained if reasons else expired).append({**row,'retain_reasons':reasons})
    result={'version':1,'bucket':catalog['bucket'],'prefix':prefix,'keep_newest':keep,'older_than_days':days,'as_of':now.isoformat(),'retained':retained,'expired':expired}
    return {**result,'plan_sha256':hashlib.sha256(canonical(result)).hexdigest()}


def verify_object(s3,bucket,row,allow_absent):
    try:response=s3.get_object(Bucket=bucket,Key=row['object'],VersionId=row['version_id'])
    except Exception as error:
        detail=getattr(error,'response',{});code=detail.get('Error',{}).get('Code')
        if allow_absent and code=='NoSuchVersion':return False
        raise
    stream=response['Body']
    try:
        require(response.get('VersionId')==row['version_id'] and not response.get('DeleteMarker') and response.get('ContentLength')==row['encrypted_bytes'],'S3 version/length differs from receipt')
        digest=hashlib.sha256();length=0
        while block:=stream.read(1024*1024):
            length+=len(block);require(length<=row['encrypted_bytes'],'object exceeds receipt length');digest.update(block)
        require(length==row['encrypted_bytes'] and digest.hexdigest()==row['sha256'],'ciphertext differs from independent receipt')
    finally:stream.close()
    return True


def apply(s3,planned):
    # Validate every retained recovery point and every candidate before the first
    # mutation. Exact non-null version IDs cannot retarget a newer current key.
    for row in planned['retained']:verify_object(s3,planned['bucket'],row,False)
    present=[verify_object(s3,planned['bucket'],row,True) for row in planned['expired']]
    result={'success':False,'operation':'retention','plan_sha256':planned['plan_sha256'],'deleted':[],'already_absent':[],'delete_unconfirmed':None}
    for row,exists in zip(planned['expired'],present):
        identity={'object':row['object'],'version_id':row['version_id']}
        if not exists:result['already_absent'].append(identity);continue
        result['delete_unconfirmed']=identity
        try:
            response=s3.delete_object(Bucket=planned['bucket'],Key=row['object'],VersionId=row['version_id'])
            require(response.get('VersionId')==row['version_id'] and not response.get('DeleteMarker'),'delete acknowledgement differs')
        except (Exception,KeyboardInterrupt) as error:
            result['error']=str(error) if type(error) is ValueError else type(error).__name__
            if isinstance(error,KeyboardInterrupt):result['interrupted']=True
            return result
        result['deleted'].append(identity);result['delete_unconfirmed']=None
    result['success']=True
    return result


def client(endpoint,region):
    from urllib.parse import urlsplit
    if endpoint:
        parsed=urlsplit(endpoint);require(parsed.scheme=='https' or (parsed.scheme=='http' and parsed.hostname in ['127.0.0.1','localhost','::1']),'endpoint requires HTTPS except loopback')
        require(parsed.username is None and parsed.password is None and not parsed.query and not parsed.fragment,'invalid endpoint')
    import boto3
    from botocore.config import Config
    return boto3.client('s3',endpoint_url=endpoint,region_name=region,config=Config(signature_version='s3v4',s3={'addressing_style':'path'},connect_timeout=10,read_timeout=60,retries={'total_max_attempts':1}))


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--catalog',type=Path,required=True);parser.add_argument('--keep-newest',type=int,required=True);parser.add_argument('--older-than-days',type=int,required=True);parser.add_argument('--as-of');parser.add_argument('--apply',action='store_true');parser.add_argument('--plan-sha256');parser.add_argument('--endpoint');parser.add_argument('--region',default='us-east-1');args=parser.parse_args()
    try:
        with args.catalog.open('rb') as stream:data=stream.read(32*1024*1024+1)
        require(len(data)<=32*1024*1024,'catalog oversized')
        now=instant(args.as_of) if args.as_of else datetime.now(timezone.utc)
        planned=plan(json.loads(data,object_pairs_hook=unique),args.keep_newest,args.older_than_days,now)
        if args.plan_sha256:require(args.plan_sha256==planned['plan_sha256'],'reviewed plan changed')
        result=apply(client(args.endpoint,args.region),planned) if args.apply else {'success':True,'operation':'retention_plan',**planned}
        print(json.dumps(result,indent=2));return 0 if result['success'] else (130 if result.get('interrupted') else 1)
    except Exception as error:
        print(json.dumps({'success':False,'operation':'retention','error':str(error) if type(error) is ValueError else type(error).__name__}),file=sys.stderr);return 1


if __name__=='__main__':raise SystemExit(main())
