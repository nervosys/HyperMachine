#!/usr/bin/env python3
"""Instrument accepted boot buffers in an isolated GNU/Linux source copy."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


PROBE = '''
// Diagnostic only: normal-thread GNU allocator observations, never a signal handler.
pub(crate) fn allocation_probe(stage: &str, bytes: usize) {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        if std::env::var_os("HM_BOOT_ALLOC_DIAGNOSTIC").as_deref() != Some(std::ffi::OsStr::new("1")) {
            return;
        }
        #[repr(C)]
        struct Mallinfo2 {
            arena: usize, ordblks: usize, smblks: usize, hblks: usize,
            hblkhd: usize, usmblks: usize, fsmblks: usize, uordblks: usize,
            fordblks: usize, keepcost: usize,
        }
        extern "C" { fn mallinfo2() -> Mallinfo2; }
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        // SAFETY: ABI matches GNU malloc.h; called during normal execution.
        // No allocator tuning is performed by this diagnostic.
        let m = unsafe { mallinfo2() };
        let seq = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        eprintln!("HM_BOOT_ALLOC seq={seq} stage={stage} bytes={bytes} arena={} mapped={} used={} free={} top={}",
            m.arena, m.hblkhd, m.uordblks, m.fordblks, m.keepcost);
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = (stage, bytes);
}

'''


def replace(text, anchor, changed):
    if text.count(anchor) != 1:
        raise ValueError('accepted allocation anchor differs: ' + anchor)
    return text.replace(anchor, changed)


def generate(root, context, patch):
    root = root.resolve(strict=True)
    if root == Path(__file__).resolve().parent.parent or (root / '.git').exists():
        raise ValueError('requires isolated source copy')
    catalog = json.loads(context.read_text())['accepted_source_sha256']
    for name, sha in catalog.items():
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('accepted source differs: ' + name)
    source = 'crates/hv2-core/src/boot/source.rs'
    linux = 'crates/hv2-core/src/boot/linux.rs'
    original = {name: (root / name).read_text() for name in (source, linux)}
    changed = dict(original)
    changed[source] = replace(changed[source], 'use std::path::{Path, PathBuf};', 'use std::path::{Path, PathBuf};\n' + PROBE)
    changed[source] = replace(changed[source], '    let data = std::fs::read(path)', '    allocation_probe("image-read-before", 0);\n    let data = std::fs::read(path)')
    changed[source] = replace(changed[source], '    if data.is_empty() {', '    allocation_probe("image-read-live", data.len());\n    if data.is_empty() {')
    anchor = '''        Ok(self
            .memory_regions()?
            .iter()
            .map(|(addr, data)| addr + data.len() as u64)
            .max()
            .unwrap_or(0))'''
    changed[source] = replace(changed[source], anchor, '''        allocation_probe("highest-before", 0);
        let regions = self.memory_regions()?;
        let bytes = regions.iter().map(|(_, data)| data.len()).sum();
        allocation_probe("highest-live", bytes);
        let highest = regions.iter().map(|(addr, data)| addr + data.len() as u64).max().unwrap_or(0);
        drop(regions);
        allocation_probe("highest-dropped", bytes);
        Ok(highest)''')
    prefix = 'crate::boot::source::allocation_probe'
    changed[linux] = replace(changed[linux], '            regions.push((addr, initrd.clone()));', f'''            {prefix}("initrd-copy-before", initrd.len());
            regions.push((addr, initrd.clone()));
            {prefix}("initrd-copy-live", initrd.len());''')
    changed[linux] = replace(changed[linux], '            let kernel_data = params.kernel_image[kernel_offset..].to_vec();', f'''            {prefix}("kernel-copy-before", params.kernel_image.len() - kernel_offset);
            let kernel_data = params.kernel_image[kernel_offset..].to_vec();
            {prefix}("kernel-copy-live", kernel_data.len());''')
    if patch.exists():
        raise ValueError('patch exists; preserve previous evidence')
    patch.write_text(''.join(''.join(difflib.unified_diff(original[n].splitlines(True), changed[n].splitlines(True), fromfile='accepted/' + n, tofile='diagnostic/' + n)) for n in changed))
    for name, text in changed.items():
        (root / name).write_text(text)
    for name, sha in catalog.items():
        if name not in changed and hashlib.sha256((root / name).read_bytes()).hexdigest() != sha:
            raise ValueError('unrelated source changed: ' + name)
    return {'diagnostic_only': True, 'runtime_change_adopted': False,
            'performance_win_established': False, 'accepted_source_files': len(catalog),
            'changed_sha256': {n: hashlib.sha256((root / n).read_bytes()).hexdigest() for n in changed},
            'limitations': ['process-wide allocator totals; concurrent activity can confound deltas',
                            'logging affects allocations and timing; exclude from rankings',
                            'allocator bytes are not resident memory or PSS']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--source-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.source_context, args.patch), indent=2))
