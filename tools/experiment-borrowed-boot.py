#!/usr/bin/env python3
"""Generate borrowed Linux/raw boot-region candidate in accepted isolated sources."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


BORROWED_SOURCE = '''    /// Borrow image buffers where their original validated layout permits it.
    /// Generated boot structures and the existing Multiboot path remain owned.
    fn borrowed_memory_regions(&self) -> Result<Vec<(u64, std::borrow::Cow<'_, [u8]>)>> {
        use std::borrow::Cow;
        match self {
            Self::Linux(params) => LinuxBootProtocol::borrowed_guest_memory(params),
            Self::Raw { data, load_addr, .. } => Ok(vec![(*load_addr, Cow::Borrowed(data))]),
            Self::Multiboot(_) => Ok(self.memory_regions()?.into_iter()
                .map(|(addr, data)| (addr, Cow::Owned(data))).collect()),
        }
    }

    /// The data-only form of borrowed regions, preserving zero-range filtering.
    pub(crate) fn borrowed_data_regions(&self) -> Result<Vec<(u64, std::borrow::Cow<'_, [u8]>)>> {
        let zeroed = self.zero_ranges()?;
        Ok(self.borrowed_memory_regions()?.into_iter().filter(|(addr, data)| {
            !zeroed.iter().any(|(zaddr, zlen)| zaddr == addr && *zlen == data.len() as u64)
        }).collect())
    }

'''

TESTS = '''
    #[test]
    fn borrowed_linux_images_reference_original_buffers_and_keep_wire_layout() {
        let params = LinuxBootParams {
            kernel_image: valid_bzimage(), initrd: Some(vec![0x6d; 8192]),
            cmdline: "console=ttyS0 quiet".into(), memory_size: 256 * 1024 * 1024,
            ..Default::default()
        };
        let boot = LoadedBoot::Linux(Box::new(params));
        let LoadedBoot::Linux(params) = &boot else { unreachable!() };
        let borrowed = boot.borrowed_data_regions().unwrap();
        let kernel = borrowed.iter().find(|(addr, _)| *addr == params.kernel_addr).unwrap();
        assert!(matches!(kernel.1, std::borrow::Cow::Borrowed(_)));
        assert_eq!(kernel.1.as_ptr(), params.kernel_image[2560..].as_ptr());
        assert_eq!(kernel.1.as_ref(), &params.kernel_image[2560..]);
        let initrd = &borrowed[0];
        assert!(matches!(initrd.1, std::borrow::Cow::Borrowed(_)));
        assert_eq!(initrd.1.as_ptr(), params.initrd.as_ref().unwrap().as_ptr());
        assert_eq!(initrd.0, 256 * 1024 * 1024 - 8192);
        let cmdline = borrowed.iter().find(|(addr, _)| *addr == params.setup_addr + 4096).unwrap();
        assert_eq!(cmdline.1.as_ref(), b"console=ttyS0 quiet\\0");
        let setup = borrowed.iter().find(|(addr, _)| *addr == params.setup_addr).unwrap();
        assert_eq!(setup.1.len(), 4096);
        assert_eq!(u32::from_le_bytes(setup.1[0x218..0x21c].try_into().unwrap()), initrd.0 as u32);
        assert_eq!(u32::from_le_bytes(setup.1[0x21c..0x220].try_into().unwrap()), 8192);
        assert_eq!(boot.highest_address().unwrap(), initrd.0 + 8192);
    }

    #[test]
    fn borrowed_layout_retains_linux_validation_errors() {
        let valid = LinuxBootParams { kernel_image: valid_bzimage(), memory_size: 64 * 1024 * 1024, ..Default::default() };
        for case in 0..4 {
            let mut params = valid.clone();
            match case {
                0 => params.memory_size = 0,
                1 => params.kernel_image[0x202] = 0,
                2 => params.cmdline = "x".repeat(4097),
                _ => params.initrd = Some(vec![0; 65 * 1024 * 1024]),
            }
            let boot = LoadedBoot::Linux(Box::new(params));
            let owned = boot.memory_regions().unwrap_err().to_string();
            assert_eq!(boot.borrowed_data_regions().unwrap_err().to_string(), owned);
            assert_eq!(boot.highest_address().unwrap_err().to_string(), owned);
        }
    }

    #[test]
    fn raw_layout_borrows_and_owned_api_stays_independent() {
        let boot = LoadedBoot::Raw { data: vec![0xf4, 0xeb, 0xfd], load_addr: 0x7c00, entry: 0x7c00 };
        let LoadedBoot::Raw { data, .. } = &boot else { unreachable!() };
        let borrowed = boot.borrowed_data_regions().unwrap();
        assert_eq!(borrowed[0].1.as_ptr(), data.as_ptr());
        assert!(matches!(borrowed[0].1, std::borrow::Cow::Borrowed(_)));
        let mut owned = boot.memory_regions().unwrap();
        owned[0].1[0] = 0;
        assert_eq!(data[0], 0xf4);
        assert_eq!(boot.highest_address().unwrap(), 0x7c03);
    }

    #[test]
    fn multiboot_fallback_preserves_bss_filtering_and_highest_address() {
        let magic = 0x1bad_b002u32;
        let flags = 0x0001_0000u32;
        let mut image = Vec::new();
        for word in [magic, flags, 0u32.wrapping_sub(magic.wrapping_add(flags)),
                     0x0010_0000, 0x0010_0000, 0x0010_0020, 0x0010_0100, 0x0010_0000] {
            image.extend_from_slice(&word.to_le_bytes());
        }
        image.resize(64, 0xcc);
        let boot = LoadedBoot::Multiboot(Box::new(MultibootInfo {
            kernel_image: image, ..Default::default()
        }));
        assert_eq!(boot.zero_ranges().unwrap(), vec![(0x0010_0020, 0xe0)]);
        let borrowed = boot.borrowed_data_regions().unwrap();
        assert!(borrowed.iter().all(|(_, bytes)| matches!(bytes, std::borrow::Cow::Owned(_))));
        assert!(!borrowed.iter().any(|(addr, _)| *addr == 0x0010_0020));
        assert_eq!(borrowed.iter().find(|(addr, _)| *addr == 0x0010_0000).unwrap().1.len(), 32);
        assert_eq!(boot.highest_address().unwrap(), 0x0010_0100);
        assert_eq!(borrowed.into_iter().map(|(addr, bytes)| (addr, bytes.into_owned())).collect::<Vec<_>>(),
                   boot.data_regions().unwrap());
    }
'''


def replace(text, anchor, replacement):
    if text.count(anchor) != 1:
        raise ValueError('candidate anchor differs: ' + anchor)
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
    names = ['crates/hv2-core/src/boot/linux.rs', 'crates/hv2-core/src/boot/source.rs', 'crates/hv2-core/src/backends/kvm.rs']
    original = {name: (root / name).read_bytes().decode('utf-8') for name in names}
    changed = dict(original)
    linux, source, kvm = names
    anchor = '    pub fn prepare_guest_memory(params: &LinuxBootParams) -> Result<Vec<(u64, Vec<u8>)>> {'
    changed[linux] = replace(changed[linux], anchor, anchor + '''
        Ok(Self::borrowed_guest_memory(params)?.into_iter()
            .map(|(addr, data)| (addr, data.into_owned())).collect())
    }

    /// Prepare the same validated layout without duplicating kernel/initrd bytes.
    pub(crate) fn borrowed_guest_memory(params: &LinuxBootParams) -> Result<Vec<(u64, std::borrow::Cow<'_, [u8]>)>> {
        use std::borrow::Cow;''')
    changed[linux] = replace(changed[linux], '        let mut regions: Vec<(u64, Vec<u8>)> = Vec::new();', "        let mut regions: Vec<(u64, Cow<'_, [u8]>)> = Vec::new();")
    changed[linux] = replace(changed[linux], 'regions.push((addr, initrd.clone()));', 'regions.push((addr, Cow::Borrowed(initrd.as_slice())));')
    changed[linux] = replace(changed[linux], 'regions.push((params.setup_addr, boot_params));', 'regions.push((params.setup_addr, Cow::Owned(boot_params)));')
    changed[linux] = replace(changed[linux], 'regions.push((cmdline_addr, cmdline_bytes));', 'regions.push((cmdline_addr, Cow::Owned(cmdline_bytes)));')
    changed[linux] = replace(changed[linux], 'let kernel_data = params.kernel_image[kernel_offset..].to_vec();', 'let kernel_data = Cow::Borrowed(&params.kernel_image[kernel_offset..]);')
    changed[linux] = replace(changed[linux], '        let regions = Self::prepare_guest_memory(params)?;', '        let regions = Self::borrowed_guest_memory(params)?;')
    anchor = '    /// The ranges that must read as zero, without materialising the zeros.'
    changed[source] = replace(changed[source], anchor, BORROWED_SOURCE + anchor)
    changed[source] = replace(changed[source], '        Ok(self\n            .memory_regions()?\n            .iter()', '        Ok(self\n            .borrowed_memory_regions()?\n            .iter()')
    changed[source] = replace(changed[source], '    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {', TESTS + '\n    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {')
    changed[kvm] = replace(changed[kvm], 'for (addr, data) in boot.data_regions()? {', 'for (addr, data) in boot.borrowed_data_regions()? {')
    if patch.exists():
        raise ValueError('patch exists; preserve candidate')
    patch.write_bytes(''.join(''.join(difflib.unified_diff(original[n].splitlines(True), changed[n].splitlines(True), fromfile='accepted/' + n, tofile='candidate/' + n)) for n in names).encode('utf-8'))
    for name, text in changed.items():
        (root / name).write_bytes(text.encode('utf-8'))
    for name, sha in catalog.items():
        if name not in changed and hashlib.sha256((root / name).read_bytes()).hexdigest() != sha:
            raise ValueError('unrelated source changed: ' + name)
    return {'runtime_change_adopted': False, 'performance_win_established': False,
            'accepted_source_files': len(catalog),
            'candidate_sha256': {n: hashlib.sha256((root / n).read_bytes()).hexdigest() for n in names},
            'scope': 'borrow Linux/raw image bytes for layout and KVM loading; retain Multiboot owned path'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--source-context', type=Path, required=True)
    parser.add_argument('--patch', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.source, args.source_context, args.patch), indent=2))
