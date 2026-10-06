#!/usr/bin/env python3
"""Generate a bounded prepared-restore admission candidate in an isolated copy."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


BASE_SHA256='5b8d92902845f748bcfd497cdaa349a7509b4ed27fd6dd867935c8f27e59fb52'


def require(value,message):
    if not value:raise ValueError(message)


def render(text,limit):
    require(isinstance(limit,int) and not isinstance(limit,bool) and 1<=limit<=128,'invalid restore admission bound')
    replacements=[
        ('    cold_boot_slots: Option<Arc<tokio::sync::Semaphore>>,',
         '    cold_boot_slots: Option<Arc<tokio::sync::Semaphore>>,\n    // Isolated experiment: bound active prepared launches through Restored acknowledgement.\n    restore_boot_slots: Arc<tokio::sync::Semaphore>,'),
        ('    let cold_boot_permit = if snapshot.is_none() {',
         '''    let restore_boot_permit = if snapshot.is_some() {
        Some(Arc::clone(&state.restore_boot_slots).acquire_owned().await.map_err(|_| {
            (StatusCode::SERVICE_UNAVAILABLE, "prepared restore admission unavailable".to_string())
        })?)
    } else {
        None
    };
    let cold_boot_permit = if snapshot.is_none() {'''),
        ('    drop(cold_boot_permit);','    drop(cold_boot_permit);\n    drop(restore_boot_permit);'),
        ('        cold_boot_slots,','        cold_boot_slots,\n        restore_boot_slots: Arc::new(tokio::sync::Semaphore::new('+str(limit)+')),')]
    for anchor,replacement in replacements:
        require(text.count(anchor)==1,'restore admission anchor differs: '+anchor)
        text=text.replace(anchor,replacement)
    return text


def generate(root,context,limit,patch):
    root=root.resolve(strict=True)
    require(root!=Path(__file__).resolve().parent.parent and not (root/'.git').exists(),'requires isolated source copy')
    catalog=json.loads(context.read_text())['accepted_source_sha256']
    for name,sha in catalog.items():
        path=(root/name).resolve(strict=True)
        require(path.is_relative_to(root),'source catalog escapes isolated root')
        require(hashlib.sha256(path.read_bytes()).hexdigest()==sha,'accepted source differs: '+name)
    main=root/'crates/hv2-sandboxd/src/main.rs';before=main.read_bytes()
    require(hashlib.sha256(before).hexdigest()==BASE_SHA256,'requires exact accepted main')
    text=render(before.decode(),limit)
    require(not patch.exists(),'patch exists; preserve previous candidate')
    patch.write_text(''.join(difflib.unified_diff(before.decode().splitlines(True),text.splitlines(True),fromfile='accepted-main.rs',tofile='candidate-main.rs')),encoding='utf-8')
    main.write_bytes(text.encode())
    for name,sha in catalog.items():
        if name!='crates/hv2-sandboxd/src/main.rs':require(hashlib.sha256((root/name).read_bytes()).hexdigest()==sha,'unrelated isolated source changed: '+name)
    return {'limit':limit,'accepted_source_files':len(catalog),'only_changed_file':'crates/hv2-sandboxd/src/main.rs','candidate_main_sha256':hashlib.sha256(main.read_bytes()).hexdigest(),'runtime_change_adopted':False,'performance_win_established':False,'hypothesis':'Bound simultaneous prepared launch/readiness work; admission wait remains part of client readiness'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path);parser.add_argument('--source-context',type=Path,required=True);parser.add_argument('--limit',type=int,default=16);parser.add_argument('--patch',type=Path,required=True);args=parser.parse_args()
    print(json.dumps(generate(args.source,args.source_context,args.limit,args.patch),indent=2))
