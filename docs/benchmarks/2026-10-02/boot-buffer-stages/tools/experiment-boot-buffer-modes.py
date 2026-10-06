#!/usr/bin/env python3
"""Add owned/borrowed boot-buffer modes to a verified isolated candidate."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


MODE = '''
// Isolated counterfactual: one executable, explicit host-only buffer mode.
pub(crate) fn borrowed_images_enabled() -> bool {
    static MODE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        let borrowed = std::env::var_os("HM_BOOT_IMAGE_MODE").as_deref()
            != Some(std::ffi::OsStr::new("owned"));
        eprintln!("HM_BOOT_IMAGE_MODE={}", if borrowed { "borrowed" } else { "owned" });
        borrowed
    })
}

'''

TEST = '''
    #[test]
    fn explicit_buffer_modes_preserve_linux_wire_bytes_and_validation() {
        let params = LinuxBootParams {
            kernel_image: create_minimal_bzimage(), initrd: Some(vec![0x51; 8192]),
            cmdline: "console=ttyS0 quiet".into(), memory_size: 64 * 1024 * 1024,
            ..Default::default()
        };
        let borrowed = LinuxBootProtocol::borrowed_guest_memory_with_mode(&params, true).unwrap();
        let owned = LinuxBootProtocol::borrowed_guest_memory_with_mode(&params, false).unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed.iter().filter(|(_, bytes)| matches!(bytes, std::borrow::Cow::Borrowed(_))).count(), 2);
        assert!(owned.iter().all(|(_, bytes)| matches!(bytes, std::borrow::Cow::Owned(_))));
        assert_eq!(borrowed[0].1.as_ptr(), params.initrd.as_ref().unwrap().as_ptr());
        assert_ne!(owned[0].1.as_ptr(), params.initrd.as_ref().unwrap().as_ptr());
        for case in 0..4 {
            let mut invalid = params.clone();
            match case {
                0 => invalid.memory_size = 0,
                1 => invalid.kernel_image[0x202] = 0,
                2 => invalid.cmdline = "x".repeat(4097),
                _ => invalid.initrd = Some(vec![0; 65 * 1024 * 1024]),
            }
            let a = LinuxBootProtocol::borrowed_guest_memory_with_mode(&invalid, true).unwrap_err();
            let b = LinuxBootProtocol::borrowed_guest_memory_with_mode(&invalid, false).unwrap_err();
            assert_eq!(a.to_string(), b.to_string());
        }
    }
'''


def replace(text, anchor, replacement):
    if text.count(anchor) != 1:
        raise ValueError('mode anchor differs: ' + anchor)
    return text.replace(anchor, replacement)


def generate(root, accepted, candidate, patch):
    root = root.resolve(strict=True)
    if root == Path(__file__).resolve().parent.parent or (root / '.git').exists():
        raise ValueError('requires isolated source copy')
    catalog = json.loads(accepted.read_text())['accepted_source_sha256']
    candidate_catalog = json.loads(candidate.read_text())['candidate_sha256']
    expected = {**catalog, **candidate_catalog}
    for name, sha in expected.items():
        path = (root / name).resolve(strict=True)
        if not path.is_relative_to(root) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('verified candidate source differs: ' + name)
    linux = 'crates/hv2-core/src/boot/linux.rs'
    source = 'crates/hv2-core/src/boot/source.rs'
    original = {n: (root / n).read_bytes().decode('utf-8') for n in (linux, source)}
    changed = dict(original)
    changed[source] = replace(changed[source], 'use std::path::{Path, PathBuf};', 'use std::path::{Path, PathBuf};\n' + MODE)
    changed[source] = replace(changed[source], 'Self::Raw { data, load_addr, .. } => Ok(vec![(*load_addr, Cow::Borrowed(data))]),', '''Self::Raw { data, load_addr, .. } => Ok(vec![(*load_addr,
                if borrowed_images_enabled() { Cow::Borrowed(data) } else { Cow::Owned(data.clone()) }
            )]),''')
    anchor = "    pub(crate) fn borrowed_guest_memory(params: &LinuxBootParams) -> Result<Vec<(u64, std::borrow::Cow<'_, [u8]>)>> {"
    changed[linux] = replace(changed[linux], anchor, anchor + '''
        Self::borrowed_guest_memory_with_mode(params, crate::boot::source::borrowed_images_enabled())
    }

    pub(crate) fn borrowed_guest_memory_with_mode(params: &LinuxBootParams, borrow_images: bool) -> Result<Vec<(u64, std::borrow::Cow<'_, [u8]>)>> {''')
    changed[linux] = replace(changed[linux], 'regions.push((addr, Cow::Borrowed(initrd.as_slice())));', '''regions.push((addr, if borrow_images { Cow::Borrowed(initrd.as_slice()) } else { Cow::Owned(initrd.clone()) }));''')
    changed[linux] = replace(changed[linux], 'let kernel_data = Cow::Borrowed(&params.kernel_image[kernel_offset..]);', '''let kernel_data = if borrow_images { Cow::Borrowed(&params.kernel_image[kernel_offset..]) }
                else { Cow::Owned(params.kernel_image[kernel_offset..].to_vec()) };''')
    changed[linux] = replace(changed[linux], '    fn create_minimal_bzimage() -> Vec<u8> {', TEST + '\n    fn create_minimal_bzimage() -> Vec<u8> {')
    if patch.exists():
        raise ValueError('preserve existing patch')
    patch.write_bytes(''.join(''.join(difflib.unified_diff(original[n].splitlines(True), changed[n].splitlines(True), fromfile='borrowed/' + n, tofile='mode/' + n)) for n in changed).encode('utf-8'))
    for name, text in changed.items():
        (root / name).write_bytes(text.encode('utf-8'))
    for name, sha in expected.items():
        if name not in changed and hashlib.sha256((root / name).read_bytes()).hexdigest() != sha:
            raise ValueError('unrelated source changed: ' + name)
    return {'experiment_only': True, 'production_runtime_changed': False,
            'intervention': 'same_binary_boot_buffer_modes', 'source_files_verified': len(expected),
            'mode_source_sha256': {n: hashlib.sha256((root / n).read_bytes()).hexdigest() for n in candidate_catalog},
            'mode_environment': 'HM_BOOT_IMAGE_MODE', 'modes': ['owned', 'borrowed'],
            'default_mode': 'borrowed', 'startup_mode_logged_before_scoring': True}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--accepted-context', type=Path, required=True)
    parser.add_argument('--candidate-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.accepted_context, args.candidate_context, args.patch), indent=2))
