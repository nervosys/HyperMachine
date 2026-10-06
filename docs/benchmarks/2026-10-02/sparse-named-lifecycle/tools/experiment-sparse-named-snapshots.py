#!/usr/bin/env python3
"""Generate full sparse named-snapshot candidate only in isolated accepted sources."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

HELPER = '''/// The header and its uniquely named full-image sidecar, when present.
fn remove_snapshot_files(file: &std::path::Path) {
    let _ = std::fs::remove_file(file);
    let mut image = file.as_os_str().to_os_string();
    image.push(".mem");
    let _ = std::fs::remove_file(std::path::PathBuf::from(image));
}

'''

def replace(text, anchor, replacement, count=1):
    if text.count(anchor) != count:
        raise ValueError('sparse snapshot anchor differs: ' + anchor)
    return text.replace(anchor, replacement)

def generate(root, context, patch):
    root = root.resolve(strict=True)
    if root == Path(__file__).resolve().parent.parent or (root / '.git').exists():
        raise ValueError('requires isolated source copy')
    catalog = json.loads(context.read_text())['accepted_source_sha256']
    for name, sha in catalog.items():
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('accepted source differs: ' + name)
    name = 'crates/hv2-sandboxd/src/snapshots.rs'
    original = (root / name).read_bytes().decode('utf-8')
    changed = replace(original, '        if let Err(e) = vm.checkpoint_to(&file).await {',
                                '        if let Err(e) = vm.snapshot_to(&file).await {')
    changed = replace(changed, '/// A snapshot offered as a template.', HELPER + '/// A snapshot offered as a template.')
    for expression, count in (('&self.file', 1), ('&file', 1), ('&snapshot.file', 2)):
        changed = replace(changed, f'let _ = std::fs::remove_file({expression});',
                                  f'remove_snapshot_files({expression});', count)
    changed = replace(changed, '''//! A snapshot is a layered checkpoint -- only the pages a sandbox changed
//! since its template -- plus the template it is layered over. A sandbox
//! created from one is restored as a fork is: the template mapped, the
//! snapshot's pages copied in. So a snapshot costs what the sandbox
//! changed, not its RAM, and creating from one costs a restore.''',
'''//! Experimental named snapshots capture a full sparse memory image once.
//! Children map that prepared image copy-on-write instead of applying a layer.
//! The originating template still supplies resource sizes and guest devices.
//! Capture/storage cost and shared-store deletion races require verification.''')
    if patch.exists(): raise ValueError('preserve previous patch')
    patch.write_bytes(''.join(difflib.unified_diff(original.splitlines(True), changed.splitlines(True), fromfile='accepted/' + name, tofile='candidate/' + name)).encode('utf-8'))
    (root / name).write_bytes(changed.encode('utf-8'))
    return {'intervention': 'sparse_named_snapshot_images', 'production_runtime_changed': False,
            'source_files_verified': len(catalog),
            'candidate_source_sha256': {name: hashlib.sha256((root / name).read_bytes()).hexdigest()},
            'adoption_requirements': ['Matched restored state and latency/memory', 'Capture time and physical storage cost',
                                      'Delete/replacement/failure sidecar cleanup', 'Shared-store create/delete races']}

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--accepted-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.accepted_context, args.patch), indent=2))
