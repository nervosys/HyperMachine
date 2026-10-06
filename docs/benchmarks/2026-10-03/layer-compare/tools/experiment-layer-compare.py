#!/usr/bin/env python3
"""Generate an isolated layered-restore candidate that avoids identical writes."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

def generate(root, context, patch):
    root=root.resolve(strict=True)
    if root==Path(__file__).resolve().parent.parent or (root/'.git').exists():
        raise ValueError('requires isolated accepted source')
    catalog=json.loads(context.read_text())['accepted_source_sha256']
    for name,sha in catalog.items():
        path=(root/name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest()!=sha:
            raise ValueError('accepted source differs: '+name)
    name='crates/hv2-core/src/vm.rs'
    original=(root/name).read_bytes().decode('utf-8')
    start=original.index('        } else if let Some(base) = &snapshot.header.memory_base {')
    end=original.index('        } else {\n            let page_size = snapshot_file::PAGE_SIZE;',start)
    layer=original[start:end]
    anchor='            let mut page = vec![0u8; page_size as usize];'
    write='                        memory.write_bytes(region.guest_addr + at, &page[..take])?;'
    if layer.count(anchor)!=1 or layer.count(write)!=1: raise ValueError('layered restore anchors differ')
    layer=layer.replace(anchor,anchor+'\n            let mut current = vec![0u8; page_size as usize];')
    layer=layer.replace(write,'''                        // The captured page may have been dirtied and then restored
                        // to its base contents. Preserve the shared mapping when
                        // it already has exactly the bytes this layer requires.
                        memory.read_bytes_into(region.guest_addr + at, &mut current[..take])?;
                        if current[..take] != page[..take] {
                            memory.write_bytes(region.guest_addr + at, &page[..take])?;
                        }''')
    changed=original[:start]+layer+original[end:]
    if patch.exists(): raise ValueError('preserve prior patch')
    patch.write_bytes(''.join(difflib.unified_diff(original.splitlines(True),changed.splitlines(True),fromfile='accepted/'+name,tofile='candidate/'+name)).encode())
    (root/name).write_bytes(changed.encode())
    return {'intervention':'layered_restore_compare_before_write','production_runtime_changed':False,
            'source_files_verified':len(catalog),'candidate_source_sha256':{name:hashlib.sha256(changed.encode()).hexdigest()},
            'adoption_requirements':['Layered guest state and lifecycle','Matched restore latency and held memory','No capture or snapshot format change']}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('--accepted-context',type=Path,required=True)
    parser.add_argument('--patch',type=Path,required=True)
    args=parser.parse_args()
    print(json.dumps(generate(args.source,args.accepted_context,args.patch),indent=2))
