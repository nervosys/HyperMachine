//! eBPF instructions, assembled by hand.
//!
//! There is no compiler for BPF on the machines this runs on, and the one
//! program the engine needs is short, so it is written as a list of
//! `struct bpf_insn` and the instruction set is the small part of it that
//! program uses. The encodings are those of `linux/bpf.h` and
//! `linux/bpf_common.h`.

// The assembler is portable and tested everywhere; only Linux loads what it makes.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// One eBPF instruction, byte-for-byte what the kernel reads (little-endian
/// hosts: the register nibbles are `dst` low, `src` high).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Insn {
    pub code: u8,
    pub regs: u8,
    pub off: i16,
    pub imm: i32,
}

impl Insn {
    /// The eight bytes the kernel sees, in the order it sees them.
    #[cfg(test)]
    pub(crate) fn to_bytes(self) -> [u8; 8] {
        let o = self.off.to_le_bytes();
        let i = self.imm.to_le_bytes();
        [self.code, self.regs, o[0], o[1], i[0], i[1], i[2], i[3]]
    }
}

pub(crate) const R0: u8 = 0;
pub(crate) const R1: u8 = 1;
pub(crate) const R2: u8 = 2;
pub(crate) const R3: u8 = 3;
pub(crate) const R4: u8 = 4;
/// The read-only frame pointer.
pub(crate) const R10: u8 = 10;

// Instruction classes.
const LD: u8 = 0x00;
const LDX: u8 = 0x01;
const ST: u8 = 0x02;
const STX: u8 = 0x03;
const ALU64: u8 = 0x07;
const JMP: u8 = 0x05;

// Sizes and modes of memory instructions.
pub(crate) const W: u8 = 0x00;
pub(crate) const DW: u8 = 0x18;
const IMM: u8 = 0x00;
const MEM: u8 = 0x60;
const ATOMIC: u8 = 0xc0;

// Operand source.
const K: u8 = 0x00;
const X: u8 = 0x08;

// ALU operations.
pub(crate) const ADD: u8 = 0x00;
pub(crate) const SUB: u8 = 0x10;
pub(crate) const AND: u8 = 0x50;
#[cfg(test)]
pub(crate) const RSH: u8 = 0x70;
const MOV: u8 = 0xb0;

// Jump operations.
pub(crate) const JEQ: u8 = 0x10;
pub(crate) const JNE: u8 = 0x50;
const CALL: u8 = 0x80;
const EXIT: u8 = 0x90;

/// `ld_imm64` with this in `src_reg`: the immediate is a map's fd, and the
/// kernel rewrites it to the map's address.
const PSEUDO_MAP_FD: u8 = 1;

/// The helpers the program calls (`enum bpf_func_id`).
pub(crate) const HELPER_MAP_LOOKUP_ELEM: i32 = 1;
pub(crate) const HELPER_GET_NS_CURRENT_PID_TGID: i32 = 120;
pub(crate) const HELPER_GET_CURRENT_CGROUP_ID: i32 = 80;

const fn regs(dst: u8, src: u8) -> u8 {
    (src << 4) | (dst & 0x0f)
}

/// `dst = src` (64-bit).
pub(crate) const fn mov64_reg(dst: u8, src: u8) -> Insn {
    Insn {
        code: ALU64 | MOV | X,
        regs: regs(dst, src),
        off: 0,
        imm: 0,
    }
}

/// `dst = imm` (64-bit, sign-extended).
pub(crate) const fn mov64_imm(dst: u8, imm: i32) -> Insn {
    Insn {
        code: ALU64 | MOV | K,
        regs: regs(dst, 0),
        off: 0,
        imm,
    }
}

/// `dst op= imm` (64-bit).
pub(crate) const fn alu64_imm(op: u8, dst: u8, imm: i32) -> Insn {
    Insn {
        code: ALU64 | op | K,
        regs: regs(dst, 0),
        off: 0,
        imm,
    }
}

/// `dst op= src` (64-bit).
pub(crate) const fn alu64_reg(op: u8, dst: u8, src: u8) -> Insn {
    Insn {
        code: ALU64 | op | X,
        regs: regs(dst, src),
        off: 0,
        imm: 0,
    }
}

/// `dst = *(size *)(src + off)`.
pub(crate) const fn ldx(size: u8, dst: u8, src: u8, off: i16) -> Insn {
    Insn {
        code: LDX | MEM | size,
        regs: regs(dst, src),
        off,
        imm: 0,
    }
}

/// `*(size *)(dst + off) = src`.
pub(crate) const fn stx(size: u8, dst: u8, src: u8, off: i16) -> Insn {
    Insn {
        code: STX | MEM | size,
        regs: regs(dst, src),
        off,
        imm: 0,
    }
}

/// `*(size *)(dst + off) = imm`.
pub(crate) const fn st_imm(size: u8, dst: u8, off: i16, imm: i32) -> Insn {
    Insn {
        code: ST | MEM | size,
        regs: regs(dst, 0),
        off,
        imm,
    }
}

/// `lock *(size *)(dst + off) += src`: an atomic add that does not return the
/// old value (`BPF_ATOMIC` with `BPF_ADD` in the immediate).
pub(crate) const fn atomic_add(size: u8, dst: u8, src: u8, off: i16) -> Insn {
    Insn {
        code: STX | ATOMIC | size,
        regs: regs(dst, src),
        off,
        imm: ADD as i32,
    }
}

/// `if dst op imm goto +off` (off counts instruction slots after this one).
pub(crate) const fn jmp_imm(op: u8, dst: u8, imm: i32, off: i16) -> Insn {
    Insn {
        code: JMP | op | K,
        regs: regs(dst, 0),
        off,
        imm,
    }
}

/// Call a kernel helper.
pub(crate) const fn call(helper: i32) -> Insn {
    Insn {
        code: JMP | CALL,
        regs: 0,
        off: 0,
        imm: helper,
    }
}

/// Return `r0`.
pub(crate) const fn exit() -> Insn {
    Insn {
        code: JMP | EXIT,
        regs: 0,
        off: 0,
        imm: 0,
    }
}

/// `dst = <the map behind fd>`: the 16-byte `ld_imm64` with `BPF_PSEUDO_MAP_FD`.
pub(crate) const fn ld_map_fd(dst: u8, fd: i32) -> [Insn; 2] {
    [
        Insn {
            code: LD | IMM | DW,
            regs: regs(dst, PSEUDO_MAP_FD),
            off: 0,
            imm: fd,
        },
        Insn {
            code: 0,
            regs: 0,
            off: 0,
            imm: 0,
        },
    ]
}

/// `dst = imm` for a full 64-bit immediate: the plain two-slot `ld_imm64`.
pub(crate) const fn ld_imm64(dst: u8, imm: u64) -> [Insn; 2] {
    [
        Insn {
            code: LD | IMM | DW,
            regs: regs(dst, 0),
            off: 0,
            imm: imm as u32 as i32,
        },
        Insn {
            code: 0,
            regs: 0,
            off: 0,
            imm: (imm >> 32) as u32 as i32,
        },
    ]
}

/// Bytes in the exempt map's key: a `u32` tgid.
pub(crate) const EXEMPT_KEY: usize = 4;
/// Bytes in the rules map's key: `{ u64 cgroup; u32 hook; u32 pad }`.
pub(crate) const RULE_KEY: usize = 16;
/// Bytes in the rules map's value: `{ u32 errno; u32 enforce; u64 monitored; u64 refused }`.
pub(crate) const RULE_VALUE: usize = 24;

/// The program for one hook.
///
/// 1. The caller's tgid in the engine creator's pid namespace
///    (`bpf_get_ns_current_pid_tgid`); a tgid in the exempt map is never
///    judged: return 0. A caller with no pid in that namespace is never
///    exempt, because its number there does not exist and any other number
///    (the kernel's global tgid) could equal an exempt one by chance.
/// 2. Look up `{ cgroup id, hook index, 0 }` in the rules map; if there is no
///    such rule, look up the same key with cgroup id 0 (every cgroup); if
///    there is still none, return 0.
/// 3. An enforcing rule counts a refusal and returns `-errno`; a monitoring
///    rule counts a match and returns 0.
///
/// The errno is masked to 0..4095 before it is negated, because the verifier
/// requires an LSM program to return a value it can prove is in
/// `[-4095, 0]` and a map value is any 32-bit number to it.
pub(crate) fn lsm_program(
    exempt_fd: i32,
    rules_fd: i32,
    hook_index: u32,
    pidns: (u64, u64),
) -> Vec<Insn> {
    let mut p: Vec<Insn> = Vec::with_capacity(56);
    // Stack: `{ pid, tgid }` at fp-32, so the tgid is at fp-28; the rule key
    // at fp-16 (cgroup), fp-8 (hook), fp-4 (pad).
    //
    // The tgid is the one in the pid namespace of whoever made the engine
    // (`bpf_get_ns_current_pid_tgid`), which is what that caller can name. A
    // host that runs it in a pid namespace of its own (a container, WSL) has
    // other numbers than the kernel's global ones. A task with no pid in that
    // namespace is judged: it skips the exempt lookup.
    p.extend(ld_imm64(R1, pidns.0));
    p.extend(ld_imm64(R2, pidns.1));
    p.push(mov64_reg(R3, R10));
    p.push(alu64_imm(ADD, R3, -32));
    p.push(mov64_imm(R4, 8));
    p.push(call(HELPER_GET_NS_CURRENT_PID_TGID));
    let to_judge = p.len();
    p.push(jmp_imm(JNE, R0, 0, 0));
    p.extend(ld_map_fd(R1, exempt_fd));
    p.push(mov64_reg(R2, R10));
    p.push(alu64_imm(ADD, R2, -28));
    p.push(call(HELPER_MAP_LOOKUP_ELEM));
    let to_ret0_exempt = p.len();
    p.push(jmp_imm(JNE, R0, 0, 0));

    let judge = p.len();
    p.push(call(HELPER_GET_CURRENT_CGROUP_ID));
    p.push(stx(DW, R10, R0, -16));
    p.push(st_imm(W, R10, -8, hook_index as i32));
    p.push(st_imm(W, R10, -4, 0));
    p.extend(ld_map_fd(R1, rules_fd));
    p.push(mov64_reg(R2, R10));
    p.push(alu64_imm(ADD, R2, -16));
    p.push(call(HELPER_MAP_LOOKUP_ELEM));
    let to_found = p.len();
    p.push(jmp_imm(JNE, R0, 0, 0));

    // No rule for this cgroup: try the rule for every cgroup (id 0).
    p.push(st_imm(DW, R10, -16, 0));
    p.extend(ld_map_fd(R1, rules_fd));
    p.push(mov64_reg(R2, R10));
    p.push(alu64_imm(ADD, R2, -16));
    p.push(call(HELPER_MAP_LOOKUP_ELEM));
    let to_ret0_none = p.len();
    p.push(jmp_imm(JEQ, R0, 0, 0));

    let found = p.len();
    p.push(ldx(W, R1, R0, 4)); // enforce
    let to_monitor = p.len();
    p.push(jmp_imm(JEQ, R1, 0, 0));
    p.push(mov64_imm(R2, 1));
    p.push(atomic_add(DW, R0, R2, 16)); // refused += 1
    p.push(ldx(W, R1, R0, 0)); // errno
    p.push(alu64_imm(AND, R1, 0xfff));
    p.push(mov64_imm(R0, 0));
    p.push(alu64_reg(SUB, R0, R1));
    p.push(exit());

    let monitor = p.len();
    p.push(mov64_imm(R2, 1));
    p.push(atomic_add(DW, R0, R2, 8)); // monitored += 1
    let ret0 = p.len();
    p.push(mov64_imm(R0, 0));
    p.push(exit());

    let patch = |p: &mut Vec<Insn>, at: usize, to: usize| {
        // Every jump here is forward and a few dozen slots long.
        p[at].off = (to - at - 1) as i16;
    };
    patch(&mut p, to_judge, judge);
    patch(&mut p, to_ret0_exempt, ret0);
    patch(&mut p, to_found, found);
    patch(&mut p, to_ret0_none, ret0);
    patch(&mut p, to_monitor, monitor);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_match_the_kernel_header() {
        // mov r0, 0
        assert_eq!(mov64_imm(R0, 0).to_bytes(), [0xb7, 0, 0, 0, 0, 0, 0, 0]);
        // exit
        assert_eq!(exit().to_bytes(), [0x95, 0, 0, 0, 0, 0, 0, 0]);
        // call bpf_map_lookup_elem
        assert_eq!(
            call(HELPER_MAP_LOOKUP_ELEM).to_bytes(),
            [0x85, 0, 0, 0, 1, 0, 0, 0]
        );
        // r0 >>= 32
        assert_eq!(
            alu64_imm(RSH, R0, 32).to_bytes(),
            [0x77, 0, 0, 0, 0x20, 0, 0, 0]
        );
        // r0 -= r1
        assert_eq!(
            alu64_reg(SUB, R0, R1).to_bytes(),
            [0x1f, 0x10, 0, 0, 0, 0, 0, 0]
        );
        // *(u64 *)(r10 - 8) = r1
        assert_eq!(
            stx(DW, R10, R1, -8).to_bytes(),
            [0x7b, 0x1a, 0xf8, 0xff, 0, 0, 0, 0]
        );
        // r1 = *(u32 *)(r0 + 4)
        assert_eq!(
            ldx(W, R1, R0, 4).to_bytes(),
            [0x61, 0x01, 0x04, 0, 0, 0, 0, 0]
        );
        // *(u32 *)(r10 - 4) = 0
        assert_eq!(
            st_imm(W, R10, -4, 0).to_bytes(),
            [0x62, 0x0a, 0xfc, 0xff, 0, 0, 0, 0]
        );
        // lock *(u64 *)(r0 + 16) += r2
        assert_eq!(
            atomic_add(DW, R0, R2, 16).to_bytes(),
            [0xdb, 0x20, 0x10, 0, 0, 0, 0, 0]
        );
        // if r0 != 0 goto +3
        assert_eq!(
            jmp_imm(JNE, R0, 0, 3).to_bytes(),
            [0x55, 0, 3, 0, 0, 0, 0, 0]
        );
        // r2 = 0x1_0000_0002
        let [a, b] = ld_imm64(R2, 0x1_0000_0002);
        assert_eq!(a.to_bytes(), [0x18, 0x02, 0, 0, 2, 0, 0, 0]);
        assert_eq!(b.to_bytes(), [0, 0, 0, 0, 1, 0, 0, 0]);
        // r1 = map(fd 5): the two-slot ld_imm64 with the pseudo-map-fd marker
        let [a, b] = ld_map_fd(R1, 5);
        assert_eq!(a.to_bytes(), [0x18, 0x11, 0, 0, 5, 0, 0, 0]);
        assert_eq!(b.to_bytes(), [0; 8]);
    }

    #[test]
    fn instruction_is_eight_bytes() {
        assert_eq!(std::mem::size_of::<Insn>(), 8);
    }

    #[test]
    fn program_jumps_land_inside_and_end_in_exit() {
        let p = lsm_program(3, 4, 7, (0x1_0000_0002, 0x30));
        assert_eq!(p.last().unwrap().to_bytes(), exit().to_bytes());
        for (i, insn) in p.iter().enumerate() {
            if insn.code == (JMP | JNE | K) || insn.code == (JMP | JEQ | K) {
                let target = i as i64 + 1 + i64::from(insn.off);
                assert!(
                    target > i as i64 && (target as usize) < p.len(),
                    "jump at {i} lands at {target}"
                );
            }
        }
        // The hook index is an immediate of exactly one store.
        let stores = p
            .iter()
            .filter(|i| i.code == (ST | MEM | W) && i.off == -8)
            .collect::<Vec<_>>();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0].imm, 7);
        // Both maps' fds are in the program.
        let fds: Vec<i32> = p
            .iter()
            .filter(|i| i.code == (LD | IMM | DW) && i.regs >> 4 == PSEUDO_MAP_FD)
            .map(|i| i.imm)
            .collect();
        assert_eq!(fds, [3, 4, 4]);
        // The namespace is the plain 64-bit loads, low half first.
        let plain: Vec<i32> = p
            .iter()
            .enumerate()
            .filter(|(_, i)| i.code == (LD | IMM | DW) && i.regs >> 4 == 0)
            .flat_map(|(n, i)| [i.imm, p[n + 1].imm])
            .collect();
        assert_eq!(plain, [2, 1, 0x30, 0]);
    }
}
