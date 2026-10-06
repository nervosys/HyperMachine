#!/usr/bin/env python3
"""Plan or reclaim unreferenced named-image bases with every store node offline."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys

def require(value, message):
    if not value: raise ValueError(message)

def support():
    require(sys.platform == 'linux', 'offline image collection requires Linux store locks')
    spec=importlib.util.spec_from_file_location('image_gc_backup',Path(__file__).with_name('backup-snapshot-store.py'))
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module

def make_plan(root, maximum, backup):
    manifest=backup.scan(root, maximum)
    backup.dependencies(root, manifest)
    references=set()
    for name in manifest['files']:
        parts=PurePosixPath(name).parts
        if len(parts)==2 and parts[0]=='snapshots' and name.endswith('.json'):
            record=backup.read_json((root/name).read_bytes())
            require(isinstance(record,dict) and isinstance(record.get('file'),str),'invalid named snapshot record')
            relative=(PurePosixPath('snapshots')/backup.safe_name(record['file'])).as_posix()
            require(relative in manifest['files'] and backup.vm_snapshot(relative),'named record snapshot is missing')
    for name,record in manifest['files'].items():
        if not backup.vm_snapshot(name): continue
        header,offset=backup.snapshot_header(root/name)
        required={'vm_name','memory_size','regions','vcpus','total_pages','present_pages','device_state_included','memory_image','memory_base'}
        require(isinstance(header,dict) and required<=header.keys(), 'incomplete snapshot metadata; collection refused')
        require(isinstance(header['vm_name'],str) and header['vm_name'] and type(header['memory_size']) is int and header['memory_size']>0,
                'invalid snapshot identity or memory size')
        require(isinstance(header['regions'],list) and header['regions'] and isinstance(header['vcpus'],list), 'invalid snapshot region/vCPU metadata')
        pages=0
        for region in header['regions']:
            require(isinstance(region,dict) and type(region.get('guest_addr')) is int and region['guest_addr']>=0
                    and type(region.get('size')) is int and region['size']>0 and type(region.get('readonly')) is bool,
                    'invalid snapshot region')
            if not region['readonly']: pages+=(region['size']+4095)//4096
        require(type(header['total_pages']) is int and header['total_pages']==pages
                and type(header['present_pages']) is int and 0<=header['present_pages']<=pages,
                'invalid snapshot page counts')
        require(record['size']==offset+(pages+7)//8+header['present_pages']*4096, 'invalid snapshot file length')
        require(not (header['memory_image'] is not None and header['memory_base'] is not None), 'ambiguous snapshot image references')
        for field in ('memory_image','memory_base'):
            value=header[field]
            if value is None: continue
            target=(root/name).parent/value if field=='memory_image' else Path(value)
            relative=target.resolve(strict=True).relative_to(root).as_posix()
            require(manifest['files'][relative]['size']==header['memory_size'], 'snapshot image length mismatch')
            references.add(relative)
    candidates=[]
    for name,record in sorted(manifest['files'].items()):
        parts=PurePosixPath(name).parts
        if len(parts)==2 and parts[0]=='snapshots' and re.fullmatch(r'[A-Za-z0-9_.-]+-[0-9a-f]{32}\.snap\.mem',parts[1]) and name not in references:
            info=(root/name).stat()
            require(info.st_nlink==1, 'unreferenced named image has multiple hard links')
            candidates.append({'path':name,**record,'allocated_bytes':info.st_blocks*512})
    return {'version':1,'source_root':str(root),'manifest_sha256':hashlib.sha256(backup.canonical(manifest)).hexdigest(),
            'referenced_images':sorted(references),'delete':candidates,
            'delete_logical_bytes':sum(c['size'] for c in candidates),
            'delete_allocated_bytes':sum(c['allocated_bytes'] for c in candidates)}

def plan(store, maximum):
    backup=support();root=store.resolve(strict=True)
    require(root.is_dir(),'store is not a directory')
    with backup.offline_lock(root): return make_plan(root,maximum,backup)

def apply(store, raw_plan, expected_sha256, maximum):
    backup=support();root=store.resolve(strict=True)
    require(len(raw_plan)<=backup.MAX_MANIFEST,'collection plan exceeds bound')
    require(hashlib.sha256(raw_plan).hexdigest()==expected_sha256,'collection plan checksum differs')
    proposed=backup.read_json(raw_plan)
    require(isinstance(proposed,dict) and type(proposed.get('version')) is int and proposed['version']==1,
            'unsupported collection plan')
    with backup.offline_lock(root):
        current=make_plan(root,maximum,backup)
        require(current==proposed,'store changed since plan; collection refused')
        deleted=[]
        try:
            for item in current['delete']:
                (root/item['path']).unlink()
                deleted.append(item['path'])
        except OSError:
            return {'success':False,'deleted_paths':deleted,'error':'image unlink failed; create a fresh plan before retrying'}
        directory=root/'snapshots'
        if deleted:
            fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY)
            try: os.fsync(fd)
            finally: os.close(fd)
        return {'success':True,'deleted_paths':deleted,'removed_logical_bytes':current['delete_logical_bytes'],
                'removed_allocated_bytes':current['delete_allocated_bytes']}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    for command in ('plan','apply'):
        p=sub.add_parser(command);p.add_argument('--store',type=Path,required=True)
        p.add_argument('--max-expanded-bytes',type=int,default=64*1024**3)
        if command=='plan': p.add_argument('--output',type=Path,required=True)
        else:
            p.add_argument('--plan',type=Path,required=True)
            p.add_argument('--expected-sha256',required=True)
    args=parser.parse_args();os.umask(0o077)
    require(args.max_expanded_bytes>0,'maximum expanded bytes must be positive')
    if args.command=='plan':
        require(not args.output.exists(),'preserve earlier collection plan')
        require(not args.output.resolve().is_relative_to(args.store.resolve()),'collection plan must be outside the store')
        result=plan(args.store,args.max_expanded_bytes)
        raw=json.dumps(result,indent=2).encode()+b'\n'
        with args.output.open('xb') as stream: stream.write(raw);stream.flush();os.fsync(stream.fileno())
        print(json.dumps({'plan_sha256':hashlib.sha256(raw).hexdigest(),'delete_count':len(result['delete']),
                          'delete_allocated_bytes':result['delete_allocated_bytes']}))
        return 0
    with args.plan.open('rb') as stream: raw=stream.read(32*1024*1024+1)
    result=apply(args.store,raw,args.expected_sha256,args.max_expanded_bytes)
    print(json.dumps(result));return 0 if result['success'] else 1

if __name__=='__main__': raise SystemExit(main())
