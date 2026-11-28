//! Just enough BTF to find a kernel function's type id.
//!
//! A BPF LSM program is attached to a hook by the id, in the kernel's own BTF
//! (`/sys/kernel/btf/vmlinux`), of the `FUNC` named `bpf_lsm_<hook>`. This
//! reads the type section far enough to list the functions with a given name
//! prefix. A type kind it does not know is an error rather than a guess: the
//! size of the entry is what finds the next one, so a wrong guess would
//! report ids that belong to other types.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::HashMap;

const MAGIC: u16 = 0xEB9F;
/// `struct btf_header` up to and including `str_len`.
const HEADER_MIN: usize = 24;
const KIND_FUNC: u32 = 12;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The `FUNC` types whose names start with `prefix`: name (prefix kept) to
/// type id. Type ids start at 1; id 0 is `void`.
pub(crate) fn funcs_with_prefix(blob: &[u8], prefix: &str) -> Result<HashMap<String, u32>, String> {
    let short = || "BTF is truncated".to_string();
    if u16_at(blob, 0).ok_or_else(short)? != MAGIC {
        return Err("BTF magic is not 0xEB9F (a big-endian or damaged blob)".into());
    }
    let hdr_len = u32_at(blob, 4).ok_or_else(short)? as usize;
    if hdr_len < HEADER_MIN {
        return Err(format!("BTF header length {hdr_len} is too short"));
    }
    let type_off = u32_at(blob, 8).ok_or_else(short)? as usize;
    let type_len = u32_at(blob, 12).ok_or_else(short)? as usize;
    let str_off = u32_at(blob, 16).ok_or_else(short)? as usize;
    let str_len = u32_at(blob, 20).ok_or_else(short)? as usize;
    let span = |off: usize, len: usize| -> Result<&[u8], String> {
        let start = hdr_len.checked_add(off).ok_or_else(short)?;
        let end = start.checked_add(len).ok_or_else(short)?;
        blob.get(start..end).ok_or_else(short)
    };
    let types = span(type_off, type_len)?;
    let strings = span(str_off, str_len)?;

    let mut found = HashMap::new();
    let mut at = 0usize;
    let mut id = 1u32;
    while at < types.len() {
        let name_off = u32_at(types, at).ok_or_else(short)? as usize;
        let info = u32_at(types, at + 4).ok_or_else(short)?;
        let vlen = (info & 0xffff) as usize;
        let kind = (info >> 24) & 0x1f;
        let extra = match kind {
            1 => 4,             // INT
            2 => 0,             // PTR
            3 => 12,            // ARRAY
            4 | 5 => 12 * vlen, // STRUCT, UNION
            6 => 8 * vlen,      // ENUM
            7 => 0,             // FWD
            8..=11 => 0,        // TYPEDEF, VOLATILE, CONST, RESTRICT
            12 => 0,            // FUNC
            13 => 8 * vlen,     // FUNC_PROTO
            14 => 4,            // VAR
            15 => 12 * vlen,    // DATASEC
            16 => 0,            // FLOAT
            17 => 4,            // DECL_TAG
            18 => 0,            // TYPE_TAG
            19 => 12 * vlen,    // ENUM64
            other => {
                return Err(format!(
                    "BTF type {id} has kind {other}, which this reader does not know"
                ))
            }
        };
        if kind == KIND_FUNC {
            let tail = strings.get(name_off..).ok_or_else(short)?;
            let end = tail.iter().position(|&c| c == 0).ok_or_else(short)?;
            if tail[..end].starts_with(prefix.as_bytes()) {
                if let Ok(name) = std::str::from_utf8(&tail[..end]) {
                    found.insert(name.to_string(), id);
                }
            }
        }
        at = at
            .checked_add(12 + extra)
            .filter(|&n| n <= types.len())
            .ok_or_else(short)?;
        id += 1;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    /// A blob with: 1 INT, 2 PTR, 3 STRUCT (2 members), 4 FUNC_PROTO (1
    /// param), 5 FUNC `bpf_lsm_socket_create`, 6 ENUM (3), 7 FUNC `other`,
    /// 8 FUNC `bpf_lsm_bpf`.
    fn synthetic() -> Vec<u8> {
        let names = b"\0bpf_lsm_socket_create\0other\0bpf_lsm_bpf\0";
        let (n_sock, n_other, n_bpf) = (1u32, 23u32, 29u32);
        let mut t = Vec::new();
        // 1: INT, 4 trailing bytes
        le32(&mut t, 0);
        le32(&mut t, 1 << 24);
        le32(&mut t, 4);
        le32(&mut t, 0x0100_0020);
        // 2: PTR
        le32(&mut t, 0);
        le32(&mut t, 2 << 24);
        le32(&mut t, 1);
        // 3: STRUCT with 2 members of 12 bytes
        le32(&mut t, 0);
        le32(&mut t, (4 << 24) | 2);
        le32(&mut t, 16);
        t.extend_from_slice(&[0u8; 24]);
        // 4: FUNC_PROTO with 1 param of 8 bytes
        le32(&mut t, 0);
        le32(&mut t, (13 << 24) | 1);
        le32(&mut t, 0);
        t.extend_from_slice(&[0u8; 8]);
        // 5: FUNC bpf_lsm_socket_create
        le32(&mut t, n_sock);
        le32(&mut t, 12 << 24);
        le32(&mut t, 4);
        // 6: ENUM with 3 values
        le32(&mut t, 0);
        le32(&mut t, (6 << 24) | 3);
        le32(&mut t, 4);
        t.extend_from_slice(&[0u8; 24]);
        // 7: FUNC other
        le32(&mut t, n_other);
        le32(&mut t, 12 << 24);
        le32(&mut t, 4);
        // 8: FUNC bpf_lsm_bpf
        le32(&mut t, n_bpf);
        le32(&mut t, 12 << 24);
        le32(&mut t, 4);

        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC.to_le_bytes());
        b.push(1); // version
        b.push(0); // flags
        le32(&mut b, 24); // hdr_len
        le32(&mut b, 0); // type_off
        le32(&mut b, t.len() as u32);
        le32(&mut b, t.len() as u32); // str_off
        le32(&mut b, names.len() as u32);
        b.extend_from_slice(&t);
        b.extend_from_slice(names);
        b
    }

    #[test]
    fn finds_funcs_by_prefix_with_the_right_ids() {
        let found = funcs_with_prefix(&synthetic(), "bpf_lsm_").unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found["bpf_lsm_socket_create"], 5);
        assert_eq!(found["bpf_lsm_bpf"], 8);
        let all = funcs_with_prefix(&synthetic(), "").unwrap();
        assert_eq!(all["other"], 7);
    }

    #[test]
    fn finds_socket_create_in_the_running_kernel() {
        let Ok(blob) = std::fs::read("/sys/kernel/btf/vmlinux") else {
            eprintln!("skipping: this host has no /sys/kernel/btf/vmlinux");
            return;
        };
        let found = funcs_with_prefix(&blob, "bpf_lsm_").unwrap();
        let id = found["bpf_lsm_socket_create"];
        assert!(id > 1000, "{id}");
        assert!(found.contains_key("bpf_lsm_ptrace_traceme"));
        // Another run finds the same ids: they are the kernel's, not ours.
        assert_eq!(
            funcs_with_prefix(&blob, "bpf_lsm_socket_create")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn refuses_bad_magic_truncation_and_unknown_kinds() {
        let mut bad = synthetic();
        bad[0] = 0;
        assert!(funcs_with_prefix(&bad, "bpf_lsm_")
            .unwrap_err()
            .contains("magic"));

        let blob = synthetic();
        assert!(funcs_with_prefix(&blob[..blob.len() - 10], "bpf_lsm_").is_err());
        assert!(funcs_with_prefix(&blob[..10], "bpf_lsm_").is_err());

        // Kind 31 is not a BTF kind.
        let mut unknown = synthetic();
        let first_type = 24;
        unknown[first_type + 7] = 31;
        let err = funcs_with_prefix(&unknown, "bpf_lsm_").unwrap_err();
        assert!(err.contains("kind 31"), "{err}");
    }

    #[test]
    fn a_type_that_runs_past_the_section_is_an_error() {
        let mut blob = synthetic();
        // Claim the first INT has 60000 members' worth of trailing data via a STRUCT.
        blob[24 + 4] = 0xff;
        blob[24 + 5] = 0xff;
        blob[24 + 7] = 4; // STRUCT with vlen 65535
        assert!(funcs_with_prefix(&blob, "bpf_lsm_").is_err());
    }
}
