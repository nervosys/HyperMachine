//! Decoding the x86 instruction behind a memory-mapped I/O exit.
//!
//! WHPX reports a guest access to an unmapped address with the faulting
//! instruction's bytes and leaves RIP on it: the monitor has to work out what
//! the instruction was doing, supply or consume the data, and step over it.
//! This decodes the forms an operating system's MMIO accessors compile to --
//! Linux's `readb`..`readq` and `writeb`..`writeq` are single `mov`
//! instructions (with `movzx`/`movsx` for narrow loads) -- and nothing else.
//! It is not an emulator: an instruction outside that set is reported as
//! unsupported, with its bytes, instead of being guessed at. `rep movs` (what
//! `memcpy_toio` can become) and read-modify-write forms are in that set of
//! things not handled.
//!
//! The effective address is not computed; the hypervisor already reports the
//! guest-physical address. Only what moves, in which direction, and how wide.

/// Where a store takes its value from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A general register, by encoding (0 = RAX .. 15 = R15). `high8` selects
    /// AH/CH/DH/BH, which a byte-wide encoding of 4..=7 means without a REX
    /// prefix.
    Reg { index: u8, high8: bool },
    /// An immediate, already sign-extended to 64 bits.
    Imm(u64),
}

/// How a load fills its destination register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extend {
    /// The access width is the register width (the register keeps its upper
    /// bits for 8- and 16-bit writes, and zero-extends for 32-bit ones).
    None,
    Zero,
    Sign,
}

/// What an MMIO instruction does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Write `size` bytes of `src` to the address.
    Store { src: Source, size: u8 },
    /// Read `size` bytes from the address into register `index`, whose own
    /// width is `reg_size` bytes.
    Load {
        index: u8,
        high8: bool,
        size: u8,
        reg_size: u8,
        extend: Extend,
    },
}

/// A decoded instruction: what it does, and how many bytes it occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    pub access: Access,
    pub length: usize,
}

/// Bytes taken by the ModRM byte at `at`, with its SIB byte and displacement.
fn modrm_len(insn: &[u8], at: usize) -> Option<usize> {
    let modrm = *insn.get(at)?;
    let (md, rm) = (modrm >> 6, modrm & 7);
    if md == 3 {
        // A register operand cannot be the memory access that exited.
        return None;
    }
    let mut len = 1;
    let mut disp = match md {
        1 => 1,
        2 => 4,
        _ => 0,
    };
    if rm == 4 {
        let sib = *insn.get(at + 1)?;
        len += 1;
        if md == 0 && sib & 7 == 5 {
            disp = 4;
        }
    } else if md == 0 && rm == 5 {
        // RIP-relative
        disp = 4;
    }
    Some(len + disp)
}

/// Decode `bytes`, the instruction the hypervisor reported.
///
/// `reported_length` is the length the hypervisor gave, or 0 when it gave
/// none (WHP leaves it 0 for memory-access exits); the length is computed from
/// the encoding either way, and a reported one that disagrees is a refusal
/// rather than a guess.
///
/// `None` when the instruction is not one of the supported forms.
pub fn decode(bytes: &[u8], reported_length: usize) -> Option<Decoded> {
    let insn = bytes;

    let mut i = 0;
    let mut operand16 = false;
    // Legacy prefixes. Segment overrides are harmless here (the address is
    // already resolved); a lock or repeat prefix means a different instruction.
    while i < insn.len() {
        match insn[i] {
            0x66 => operand16 = true,
            0x67 | 0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 => {}
            0xF0 | 0xF2 | 0xF3 => return None,
            _ => break,
        }
        i += 1;
    }

    let mut rex = 0u8;
    if i < insn.len() && (0x40..=0x4F).contains(&insn[i]) {
        rex = insn[i];
        i += 1;
    }
    let (w, r) = (rex & 0x08 != 0, rex & 0x04 != 0);
    let has_rex = rex != 0;

    let opcode = *insn.get(i)?;
    i += 1;

    let wide: u8 = if w {
        8
    } else if operand16 {
        2
    } else {
        4
    };

    // The register named by ModRM.reg, with REX.R.
    let modrm_reg = |at: usize| -> Option<u8> {
        let modrm = *insn.get(at)?;
        Some(((modrm >> 3) & 7) | if r { 8 } else { 0 })
    };
    let byte_reg = |index: u8| -> (u8, bool) {
        // Without any REX prefix, encodings 4..=7 are AH, CH, DH, BH.
        if !has_rex && (4..=7).contains(&index) {
            (index - 4, true)
        } else {
            (index, false)
        }
    };

    let mut modrm_at = i;
    let mut imm_len = 0usize;
    let access = match opcode {
        // mov r/m8, r8 / mov r/m, r
        0x88 | 0x89 => {
            let size = if opcode == 0x88 { 1 } else { wide };
            let index = modrm_reg(i)?;
            let (index, high8) = if size == 1 {
                byte_reg(index)
            } else {
                (index, false)
            };
            Access::Store {
                src: Source::Reg { index, high8 },
                size,
            }
        }
        // mov r8, r/m8 / mov r, r/m
        0x8A | 0x8B => {
            let size = if opcode == 0x8A { 1 } else { wide };
            let index = modrm_reg(i)?;
            let (index, high8) = if size == 1 {
                byte_reg(index)
            } else {
                (index, false)
            };
            Access::Load {
                index,
                high8,
                size,
                reg_size: size,
                extend: Extend::None,
            }
        }
        // mov r/m8, imm8 / mov r/m, imm
        0xC6 | 0xC7 => {
            // /0 is the only defined form.
            if (*insn.get(i)? >> 3) & 7 != 0 {
                return None;
            }
            let size = if opcode == 0xC6 { 1 } else { wide };
            // The immediate follows the ModRM, SIB and displacement; for a
            // 64-bit operand it is 32 bits, sign-extended.
            imm_len = match size {
                1 => 1,
                2 => 2,
                _ => 4,
            };
            let imm_at = i + modrm_len(insn, i)?;
            let imm = insn.get(imm_at..imm_at + imm_len)?;
            let mut raw = 0u64;
            for (n, b) in imm.iter().enumerate() {
                raw |= u64::from(*b) << (8 * n);
            }
            let value = if size == 8 {
                i64::from(raw as u32 as i32) as u64
            } else {
                raw
            };
            Access::Store {
                src: Source::Imm(value),
                size,
            }
        }
        // movzx / movsx
        0x0F => {
            let second = *insn.get(i)?;
            i += 1;
            modrm_at = i;
            let (size, extend) = match second {
                0xB6 => (1, Extend::Zero),
                0xB7 => (2, Extend::Zero),
                0xBE => (1, Extend::Sign),
                0xBF => (2, Extend::Sign),
                _ => return None,
            };
            let index = modrm_reg(i)?;
            Access::Load {
                index,
                high8: false,
                size,
                reg_size: wide,
                extend,
            }
        }
        // movsxd r64, r/m32
        0x63 if w => {
            let index = modrm_reg(i)?;
            Access::Load {
                index,
                high8: false,
                size: 4,
                reg_size: 8,
                extend: Extend::Sign,
            }
        }
        _ => return None,
    };

    let length = modrm_at + modrm_len(insn, modrm_at)? + imm_len;
    if length > insn.len() || (reported_length != 0 && reported_length != length) {
        return None;
    }
    Some(Decoded { access, length })
}

/// The value of a load, as the destination register should hold it.
///
/// `old` is the register's current contents, for the widths that leave the
/// upper bits alone. `data` is what the device returned, `access size` bytes
/// of it significant.
pub fn load_value(old: u64, data: u64, size: u8, reg_size: u8, high8: bool, extend: Extend) -> u64 {
    let mask = |bytes: u8| -> u64 {
        if bytes >= 8 {
            u64::MAX
        } else {
            (1u64 << (8 * u32::from(bytes))) - 1
        }
    };
    let raw = data & mask(size);
    let value = match extend {
        Extend::None | Extend::Zero => raw,
        Extend::Sign => {
            let shift = 64 - 8 * u32::from(size);
            (((raw << shift) as i64) >> shift) as u64
        }
    };
    if high8 {
        return (old & !0xFF00) | ((value & 0xFF) << 8);
    }
    match reg_size {
        1 => (old & !0xFF) | (value & 0xFF),
        2 => (old & !0xFFFF) | (value & 0xFFFF),
        // A 32-bit write clears the upper half, as on hardware.
        4 => value & 0xFFFF_FFFF,
        _ => value,
    }
}

/// The bytes a store writes, from the register file value `reg` it names.
pub fn store_bytes(src: Source, size: u8, reg: u64) -> Vec<u8> {
    let value = match src {
        Source::Imm(v) => v,
        Source::Reg { high8: true, .. } => (reg >> 8) & 0xFF,
        Source::Reg { .. } => reg,
    };
    value.to_le_bytes()[..usize::from(size)].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(bytes: &[u8]) -> Option<Decoded> {
        decode(bytes, bytes.len())
    }

    #[test]
    fn writeb_is_a_byte_store_from_a_register() {
        // mov %dl,(%rsi)  -- what iowrite8 compiles to
        let got = d(&[0x88, 0x16]).unwrap();
        assert_eq!(
            got.access,
            Access::Store {
                src: Source::Reg {
                    index: 2,
                    high8: false
                },
                size: 1
            }
        );
    }

    #[test]
    fn a_byte_register_without_rex_can_be_a_high_byte() {
        // mov %ah,(%rsi): reg field 4, no REX
        let got = d(&[0x88, 0x26]).unwrap();
        assert_eq!(
            got.access,
            Access::Store {
                src: Source::Reg {
                    index: 0,
                    high8: true
                },
                size: 1
            }
        );
        // with any REX it is %spl
        let got = d(&[0x40, 0x88, 0x26]).unwrap();
        assert_eq!(
            got.access,
            Access::Store {
                src: Source::Reg {
                    index: 4,
                    high8: false
                },
                size: 1
            }
        );
    }

    #[test]
    fn the_length_is_computed_when_the_hypervisor_gives_none() {
        // The 16 bytes WHP reported for a real iowrite8: mov %dil,(%rsi) and
        // whatever followed it.
        let bytes = [
            0x40, 0x88, 0x3e, 0xe9, 0x3b, 0x6a, 0x5a, 0x00, 0x48, 0x81, 0xfe, 0, 0, 1, 0, 0x76,
        ];
        let got = decode(&bytes, 0).unwrap();
        assert_eq!(got.length, 3);
        // SIB + disp8, then disp32, then RIP-relative
        assert_eq!(
            decode(&[0x8B, 0x44, 0x24, 0x08, 0x90], 0).unwrap().length,
            4
        );
        assert_eq!(
            decode(&[0x8B, 0x80, 1, 2, 3, 4, 0x90], 0).unwrap().length,
            6
        );
        assert_eq!(
            decode(&[0x8B, 0x05, 1, 2, 3, 4, 0x90], 0).unwrap().length,
            6
        );
        // an immediate store after a displacement
        let got = decode(&[0xC7, 0x40, 0x10, 0x78, 0x56, 0x34, 0x12, 0x90], 0).unwrap();
        assert_eq!(got.length, 7);
        assert_eq!(
            got.access,
            Access::Store {
                src: Source::Imm(0x1234_5678),
                size: 4
            }
        );
    }

    #[test]
    fn writel_and_writeq() {
        // mov %edi,(%rax)
        assert_eq!(
            d(&[0x89, 0x38]).unwrap().access,
            Access::Store {
                src: Source::Reg {
                    index: 7,
                    high8: false
                },
                size: 4
            }
        );
        // mov %r9,(%rax): REX.W + REX.R
        assert_eq!(
            d(&[0x4C, 0x89, 0x08]).unwrap().access,
            Access::Store {
                src: Source::Reg {
                    index: 9,
                    high8: false
                },
                size: 8
            }
        );
        // mov %cx,(%rax): operand-size prefix
        assert_eq!(
            d(&[0x66, 0x89, 0x08]).unwrap().access,
            Access::Store {
                src: Source::Reg {
                    index: 1,
                    high8: false
                },
                size: 2
            }
        );
    }

    #[test]
    fn immediate_stores_take_the_trailing_bytes() {
        // movb $0x2a,(%rax)
        assert_eq!(
            d(&[0xC6, 0x00, 0x2A]).unwrap().access,
            Access::Store {
                src: Source::Imm(0x2A),
                size: 1
            }
        );
        // movl $0x12345678,0x10(%rax)
        assert_eq!(
            d(&[0xC7, 0x40, 0x10, 0x78, 0x56, 0x34, 0x12])
                .unwrap()
                .access,
            Access::Store {
                src: Source::Imm(0x1234_5678),
                size: 4
            }
        );
        // movq $-1,(%rax): imm32 sign-extended
        assert_eq!(
            d(&[0x48, 0xC7, 0x00, 0xFF, 0xFF, 0xFF, 0xFF])
                .unwrap()
                .access,
            Access::Store {
                src: Source::Imm(u64::MAX),
                size: 8
            }
        );
    }

    #[test]
    fn loads_and_extending_loads() {
        // mov (%rsi),%eax
        assert_eq!(
            d(&[0x8B, 0x06]).unwrap().access,
            Access::Load {
                index: 0,
                high8: false,
                size: 4,
                reg_size: 4,
                extend: Extend::None
            }
        );
        // movzbl (%rsi),%eax
        assert_eq!(
            d(&[0x0F, 0xB6, 0x06]).unwrap().access,
            Access::Load {
                index: 0,
                high8: false,
                size: 1,
                reg_size: 4,
                extend: Extend::Zero
            }
        );
        // movswq (%rsi),%rax
        assert_eq!(
            d(&[0x48, 0x0F, 0xBF, 0x06]).unwrap().access,
            Access::Load {
                index: 0,
                high8: false,
                size: 2,
                reg_size: 8,
                extend: Extend::Sign
            }
        );
    }

    #[test]
    fn what_is_not_a_plain_mov_is_refused() {
        // rep movsb, lock add, add %eax,(%rsi), a ModRM /1 under C7
        assert_eq!(d(&[0xF3, 0xA4]), None);
        assert_eq!(d(&[0xF0, 0x01, 0x06]), None);
        assert_eq!(d(&[0x01, 0x06]), None);
        assert_eq!(d(&[0xC7, 0x08, 1, 0, 0, 0]), None);
        assert_eq!(decode(&[0x88], 0), None);
        // a reported length that disagrees with the encoding
        assert_eq!(decode(&[0x88, 0x16], 5), None);
        // a register operand is not a memory access
        assert_eq!(d(&[0x88, 0xC2]), None);
    }

    #[test]
    fn load_values_follow_the_register_width_rules() {
        let old = 0xAAAA_BBBB_CCCC_DDDD;
        // 8-bit load keeps the rest of the register
        assert_eq!(
            load_value(old, 0x12, 1, 1, false, Extend::None),
            0xAAAA_BBBB_CCCC_DD12
        );
        // AH
        assert_eq!(
            load_value(old, 0x12, 1, 1, true, Extend::None),
            0xAAAA_BBBB_CCCC_12DD
        );
        // 16-bit keeps the upper 48
        assert_eq!(
            load_value(old, 0x1234, 2, 2, false, Extend::None),
            0xAAAA_BBBB_CCCC_1234
        );
        // 32-bit clears the upper 32
        assert_eq!(
            load_value(old, 0x1234_5678, 4, 4, false, Extend::None),
            0x1234_5678
        );
        // movzx byte -> 32
        assert_eq!(load_value(old, 0xFF, 1, 4, false, Extend::Zero), 0xFF);
        // movsx byte -> 64
        assert_eq!(
            load_value(old, 0x80, 1, 8, false, Extend::Sign),
            0xFFFF_FFFF_FFFF_FF80
        );
        // device returned garbage above the access width: ignored
        assert_eq!(load_value(old, 0xFFFF_FF01, 1, 4, false, Extend::Zero), 1);
    }

    #[test]
    fn store_bytes_are_little_endian_and_as_wide_as_the_access() {
        let reg = Source::Reg {
            index: 0,
            high8: false,
        };
        assert_eq!(store_bytes(reg, 2, 0x1122_3344), vec![0x44, 0x33]);
        assert_eq!(
            store_bytes(
                Source::Reg {
                    index: 0,
                    high8: true
                },
                1,
                0x1122_3344
            ),
            vec![0x33]
        );
        assert_eq!(store_bytes(Source::Imm(0x2A), 1, 0), vec![0x2A]);
    }
}
