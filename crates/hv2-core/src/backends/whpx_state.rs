//! A WHPX vCPU's architectural state, read into and written back from a
//! [`VCpuSnapshot`].
//!
//! Captured: the general registers, segments, descriptor tables, control
//! registers, `EFER` and the APIC base, `XCR0`, the x87/SSE registers, and the
//! MSRs listed in [`MSR_MAP`] (the `TSC` among them). When the partition emulates
//! the local APIC (more than one vCPU), also the APIC's register page and the
//! internal activity state (startup suspension, halt), which a restored
//! application processor needs to leave the startup state.
//!
//! Not captured, so the snapshot says so rather than claiming otherwise
//! ([`VCpuSnapshot::is_complete`] stays `false`): the XSAVE area (the AVX
//! halves of the vector registers), the pending-interruption and
//! interrupt-shadow registers, and debug registers. A vCPU is snapshotted between run exits, where an interrupt
//! injected on the previous entry has usually been taken, but that is not
//! guaranteed.

use super::whpx::WhpxVcpu;
use super::whpx_ffi::*;
use crate::error::Result;
use crate::snapshot::vcpu::{
    DescriptorTable, FpuState, GeneralRegisters, Msr, RunState, Segment, SystemRegisters,
    VCpuSnapshot,
};

/// `(architectural MSR number, WHP register)` for the MSRs this captures.
const MSR_MAP: [(u32, WHV_REGISTER_NAME); 14] = [
    (0x10, WHV_REGISTER_NAME::WHvX64RegisterTsc),
    (0x174, WHV_REGISTER_NAME::WHvX64RegisterSysenterCs),
    (0x175, WHV_REGISTER_NAME::WHvX64RegisterSysenterEsp),
    (0x176, WHV_REGISTER_NAME::WHvX64RegisterSysenterEip),
    (0x277, WHV_REGISTER_NAME::WHvX64RegisterPat),
    (0xC000_0081, WHV_REGISTER_NAME::WHvX64RegisterStar),
    (0xC000_0082, WHV_REGISTER_NAME::WHvX64RegisterLstar),
    (0xC000_0083, WHV_REGISTER_NAME::WHvX64RegisterCstar),
    (0xC000_0084, WHV_REGISTER_NAME::WHvX64RegisterSfmask),
    (0xC000_0102, WHV_REGISTER_NAME::WHvX64RegisterKernelGsBase),
    // Supervisor-state components XSAVES/XRSTORS manage and the control-flow
    // enforcement state in them. A guest that enabled CET user state (the
    // kernel logs "Supporting XSAVE feature 0x800") restores its FPU buffers
    // with XRSTORS, which faults on a vCPU whose XSS lacks bit 11.
    (0x0DA0, WHV_REGISTER_NAME::WHvX64RegisterXss),
    (0x06A0, WHV_REGISTER_NAME::WHvX64RegisterUCet),
    (0x06A2, WHV_REGISTER_NAME::WHvX64RegisterSCet),
    (0x06A4, WHV_REGISTER_NAME::WHvX64RegisterPl3Ssp),
];

/// Where `XCR0` sits in [`capture_names`]' order.
const XCR0_INDEX: usize = 35;

/// The first sixteen XMM registers, in order.
fn xmm_name(i: usize) -> WHV_REGISTER_NAME {
    use WHV_REGISTER_NAME::*;
    [
        WHvX64RegisterXmm0,
        WHvX64RegisterXmm1,
        WHvX64RegisterXmm2,
        WHvX64RegisterXmm3,
        WHvX64RegisterXmm4,
        WHvX64RegisterXmm5,
        WHvX64RegisterXmm6,
        WHvX64RegisterXmm7,
        WHvX64RegisterXmm8,
        WHvX64RegisterXmm9,
        WHvX64RegisterXmm10,
        WHvX64RegisterXmm11,
        WHvX64RegisterXmm12,
        WHvX64RegisterXmm13,
        WHvX64RegisterXmm14,
        WHvX64RegisterXmm15,
    ][i]
}

/// The eight x87 registers, in order.
fn fp_name(i: usize) -> WHV_REGISTER_NAME {
    use WHV_REGISTER_NAME::*;
    [
        WHvX64RegisterFpMmx0,
        WHvX64RegisterFpMmx1,
        WHvX64RegisterFpMmx2,
        WHvX64RegisterFpMmx3,
        WHvX64RegisterFpMmx4,
        WHvX64RegisterFpMmx5,
        WHvX64RegisterFpMmx6,
        WHvX64RegisterFpMmx7,
    ][i]
}

/// WHP's segment attribute word -> the snapshot's split fields.
///
/// Bits 0-3 type, 4 non-system (`S`), 5-6 DPL, 7 present, 12 available,
/// 13 long, 14 default size (`D/B`), 15 granularity.
fn segment_from_whp(s: &WHV_X64_SEGMENT_REGISTER) -> Segment {
    let a = s.Attributes;
    Segment {
        base: s.Base,
        limit: s.Limit,
        selector: s.Selector,
        type_: (a & 0xF) as u8,
        s: ((a >> 4) & 1) as u8,
        dpl: ((a >> 5) & 3) as u8,
        present: ((a >> 7) & 1) as u8,
        avl: ((a >> 12) & 1) as u8,
        l: ((a >> 13) & 1) as u8,
        db: ((a >> 14) & 1) as u8,
        g: ((a >> 15) & 1) as u8,
    }
}

/// The inverse of [`segment_from_whp`].
fn segment_to_whp(s: &Segment) -> WHV_X64_SEGMENT_REGISTER {
    let attributes = u16::from(s.type_ & 0xF)
        | u16::from(s.s & 1) << 4
        | u16::from(s.dpl & 3) << 5
        | u16::from(s.present & 1) << 7
        | u16::from(s.avl & 1) << 12
        | u16::from(s.l & 1) << 13
        | u16::from(s.db & 1) << 14
        | u16::from(s.g & 1) << 15;
    WHV_X64_SEGMENT_REGISTER {
        Base: s.base,
        Limit: s.limit,
        Selector: s.selector,
        Attributes: attributes,
    }
}

fn reg64(v: u64) -> WHV_REGISTER_VALUE {
    // SAFETY: an all-zero union is a valid register value; Reg64 is then set.
    let mut r: WHV_REGISTER_VALUE = unsafe { std::mem::zeroed() };
    r.Reg64 = v;
    r
}

fn reg_segment(s: &Segment) -> WHV_REGISTER_VALUE {
    // SAFETY: as for `reg64`.
    let mut r: WHV_REGISTER_VALUE = unsafe { std::mem::zeroed() };
    r.Segment = segment_to_whp(s);
    r
}

fn reg_table(t: &DescriptorTable) -> WHV_REGISTER_VALUE {
    // SAFETY: as for `reg64`.
    let mut r: WHV_REGISTER_VALUE = unsafe { std::mem::zeroed() };
    r.Table = WHV_X64_TABLE_REGISTER {
        Pad: [0; 3],
        Limit: t.limit,
        Base: t.base,
    };
    r
}

/// The registers `capture_snapshot` reads, in the order it reads them.
fn capture_names() -> Vec<WHV_REGISTER_NAME> {
    use WHV_REGISTER_NAME::*;
    let mut n = vec![
        WHvX64RegisterRax,
        WHvX64RegisterRbx,
        WHvX64RegisterRcx,
        WHvX64RegisterRdx,
        WHvX64RegisterRsi,
        WHvX64RegisterRdi,
        WHvX64RegisterRsp,
        WHvX64RegisterRbp,
        WHvX64RegisterR8,
        WHvX64RegisterR9,
        WHvX64RegisterR10,
        WHvX64RegisterR11,
        WHvX64RegisterR12,
        WHvX64RegisterR13,
        WHvX64RegisterR14,
        WHvX64RegisterR15,
        WHvX64RegisterRip,
        WHvX64RegisterRflags,
        WHvX64RegisterCs,
        WHvX64RegisterDs,
        WHvX64RegisterEs,
        WHvX64RegisterFs,
        WHvX64RegisterGs,
        WHvX64RegisterSs,
        WHvX64RegisterTr,
        WHvX64RegisterLdtr,
        WHvX64RegisterGdtr,
        WHvX64RegisterIdtr,
        WHvX64RegisterCr0,
        WHvX64RegisterCr2,
        WHvX64RegisterCr3,
        WHvX64RegisterCr4,
        WHvX64RegisterCr8,
        WHvX64RegisterEfer,
        WHvX64RegisterApicBase,
        WHvX64RegisterXCr0,
        WHvX64RegisterFpControlStatus,
        WHvX64RegisterXmmControlStatus,
    ];
    n.extend((0..16).map(xmm_name));
    n.extend((0..8).map(fp_name));
    n.extend(MSR_MAP.iter().map(|(_, name)| *name));
    n
}

impl WhpxVcpu {
    /// Read this vCPU's state. The vCPU must not be running.
    pub fn capture_snapshot(&self, id: u32) -> Result<VCpuSnapshot> {
        let names = capture_names();
        let v = self.get_registers(&names)?;
        // SAFETY: each value is read through the union member that matches the
        // register name it was requested with.
        unsafe {
            let general = GeneralRegisters {
                rax: v[0].Reg64,
                rbx: v[1].Reg64,
                rcx: v[2].Reg64,
                rdx: v[3].Reg64,
                rsi: v[4].Reg64,
                rdi: v[5].Reg64,
                rsp: v[6].Reg64,
                rbp: v[7].Reg64,
                r8: v[8].Reg64,
                r9: v[9].Reg64,
                r10: v[10].Reg64,
                r11: v[11].Reg64,
                r12: v[12].Reg64,
                r13: v[13].Reg64,
                r14: v[14].Reg64,
                r15: v[15].Reg64,
                rip: v[16].Reg64,
                rflags: v[17].Reg64,
            };
            let table = |i: usize| DescriptorTable {
                base: v[i].Table.Base,
                limit: v[i].Table.Limit,
            };
            let system = SystemRegisters {
                cs: segment_from_whp(&v[18].Segment),
                ds: segment_from_whp(&v[19].Segment),
                es: segment_from_whp(&v[20].Segment),
                fs: segment_from_whp(&v[21].Segment),
                gs: segment_from_whp(&v[22].Segment),
                ss: segment_from_whp(&v[23].Segment),
                tr: segment_from_whp(&v[24].Segment),
                ldt: segment_from_whp(&v[25].Segment),
                gdt: table(26),
                idt: table(27),
                cr0: v[28].Reg64,
                cr2: v[29].Reg64,
                cr3: v[30].Reg64,
                cr4: v[31].Reg64,
                cr8: v[32].Reg64,
                efer: v[33].Reg64,
                apic_base: v[34].Reg64,
            };
            let xcr0 = v[35].Reg64;
            let fcs = v[36].FpControlStatus;
            let xcs = v[37].XmmControlStatus;
            let mut xmm = Vec::with_capacity(16 * 16);
            for r in &v[38..54] {
                xmm.extend_from_slice(&r.Reg128.Low64.to_le_bytes());
                xmm.extend_from_slice(&r.Reg128.High64.to_le_bytes());
            }
            let mut fpr = Vec::with_capacity(8 * 16);
            for r in &v[54..62] {
                fpr.extend_from_slice(&r.Fp.Mantissa.to_le_bytes());
                fpr.extend_from_slice(&r.Fp.BiasedExponent.to_le_bytes());
            }
            let fpu = FpuState {
                fpr,
                xmm,
                fcw: fcs.FpControl,
                fsw: fcs.FpStatus,
                ftwx: fcs.FpTag,
                last_opcode: fcs.LastFpOp,
                last_ip: fcs.LastFpRip,
                last_dp: xcs.LastFpRdp,
                mxcsr: xcs.XmmStatusControl,
            };
            let msrs = MSR_MAP
                .iter()
                .enumerate()
                .map(|(i, (index, _))| Msr {
                    index: *index,
                    value: v[62 + i].Reg64,
                })
                .collect();
            Ok(VCpuSnapshot {
                id,
                general,
                system,
                fpu,
                msrs,
                lapic: self.capture_lapic(),
                xsave: Vec::new(),
                xcrs: vec![Msr {
                    index: 0,
                    value: xcr0,
                }],
                kvm_events: Vec::new(),
                run_state: self.capture_run_state(),
            })
        }
    }

    /// The vCPU's activity: runnable, halted by `HLT` or idle (the emulated
    /// local APIC absorbs those), or still waiting for its first INIT/SIPI,
    /// which is kept as the raw suspension bit so a restore can put it back.
    fn capture_run_state(&self) -> RunState {
        let Ok(v) = self.get_registers(&[WHV_REGISTER_NAME::WHvRegisterInternalActivityState])
        else {
            return RunState::Runnable;
        };
        // SAFETY: the activity state is read through its 64-bit member.
        let activity = unsafe { v[0].Reg64 };
        if activity & 1 != 0 {
            RunState::Other(1)
        } else if activity & 0b110 != 0 {
            RunState::Halted
        } else {
            RunState::Runnable
        }
    }

    /// Write a snapshot's state back. The vCPU must not be running.
    pub fn apply_snapshot(&self, s: &VCpuSnapshot) -> Result<()> {
        let names = capture_names();
        let g = &s.general;
        let y = &s.system;
        let xcr0 = s.xcrs.iter().find(|x| x.index == 0).map_or(1, |x| x.value);
        let mut values: Vec<WHV_REGISTER_VALUE> = vec![
            reg64(g.rax),
            reg64(g.rbx),
            reg64(g.rcx),
            reg64(g.rdx),
            reg64(g.rsi),
            reg64(g.rdi),
            reg64(g.rsp),
            reg64(g.rbp),
            reg64(g.r8),
            reg64(g.r9),
            reg64(g.r10),
            reg64(g.r11),
            reg64(g.r12),
            reg64(g.r13),
            reg64(g.r14),
            reg64(g.r15),
            reg64(g.rip),
            reg64(g.rflags),
            reg_segment(&y.cs),
            reg_segment(&y.ds),
            reg_segment(&y.es),
            reg_segment(&y.fs),
            reg_segment(&y.gs),
            reg_segment(&y.ss),
            reg_segment(&y.tr),
            reg_segment(&y.ldt),
            reg_table(&y.gdt),
            reg_table(&y.idt),
            reg64(y.cr0),
            reg64(y.cr2),
            reg64(y.cr3),
            reg64(y.cr4),
            reg64(y.cr8),
            reg64(y.efer),
            reg64(y.apic_base),
            reg64(xcr0),
        ];
        let f = &s.fpu;
        // SAFETY: zeroed unions are valid; the member written is the one that
        // matches the register name in the same position of `names`.
        unsafe {
            let mut fcs: WHV_REGISTER_VALUE = std::mem::zeroed();
            fcs.FpControlStatus = WHV_X64_FP_CONTROL_STATUS_REGISTER {
                FpControl: f.fcw,
                FpStatus: f.fsw,
                FpTag: f.ftwx,
                Reserved: 0,
                LastFpOp: f.last_opcode,
                LastFpRip: f.last_ip,
            };
            values.push(fcs);
            let mut xcs: WHV_REGISTER_VALUE = std::mem::zeroed();
            xcs.XmmControlStatus = WHV_X64_XMM_CONTROL_STATUS_REGISTER {
                LastFpRdp: f.last_dp,
                XmmStatusControl: f.mxcsr,
                XmmStatusControlMask: 0xFFFF_FFFF,
            };
            values.push(xcs);
            for i in 0..16 {
                let mut r: WHV_REGISTER_VALUE = std::mem::zeroed();
                let b = f.xmm.get(i * 16..i * 16 + 16).unwrap_or(&[0; 16]);
                r.Reg128 = WHV_UINT128 {
                    Low64: u64::from_le_bytes(b[0..8].try_into().unwrap()),
                    High64: u64::from_le_bytes(b[8..16].try_into().unwrap()),
                };
                values.push(r);
            }
            for i in 0..8 {
                let mut r: WHV_REGISTER_VALUE = std::mem::zeroed();
                let b = f.fpr.get(i * 16..i * 16 + 16).unwrap_or(&[0; 16]);
                r.Fp = WHV_X64_FP_REGISTER {
                    Mantissa: u64::from_le_bytes(b[0..8].try_into().unwrap()),
                    BiasedExponent: u64::from_le_bytes(b[8..16].try_into().unwrap()),
                };
                values.push(r);
            }
        }
        // An MSR the snapshot lacks is written as zero would be wrong for the
        // TSC and PAT; keep the vCPU's current value instead by reading it.
        let current = self.get_registers(&names[62..])?;
        for (i, (index, _)) in MSR_MAP.iter().enumerate() {
            match s.msrs.iter().find(|m| m.index == *index) {
                Some(m) => values.push(reg64(m.value)),
                None => values.push(current[i]),
            }
        }
        // XCR0 is accepted only once CR4.OSXSAVE is set, and a new vCPU starts
        // with it clear: written in the same call as CR4 it did not take, and
        // the guest's first XRSTOR of an AVX-wide save area faulted ("Bad FPU
        // state"). So everything else first, then XCR0 alone, then read it back.
        let mut names = names;
        let xcr0_name = names.remove(XCR0_INDEX);
        let xcr0_value = values.remove(XCR0_INDEX);
        self.set_registers(&names, &values)?;
        self.set_registers(&[xcr0_name], &[xcr0_value])?;
        let back = self.get_registers(&[xcr0_name])?;
        // SAFETY: XCR0 is read through its 64-bit member.
        let got = unsafe { back[0].Reg64 };
        if got != xcr0 {
            return Err(crate::error::Error::VM(format!(
                "XCR0 was written as {xcr0:#x} and reads back as {got:#x}"
            )));
        }
        // The emulated local APIC (several vCPUs): its timer, its pending and
        // in-service vectors. After the registers, since the APIC base is one.
        if !s.lapic.is_empty() {
            self.apply_lapic(&s.lapic)?;
            // A vCPU of a fresh partition waits for its INIT/SIPI, and writing
            // its registers does not end that: the activity state has to be
            // written too. A halted vCPU is released (its HLT has retired and
            // it executes the idle loop's next one); one that never started
            // keeps waiting.
            let activity = match s.run_state {
                RunState::Other(n) => u64::from(n),
                RunState::Runnable | RunState::Halted => 0,
            };
            self.set_registers_64(&[(
                WHV_REGISTER_NAME::WHvRegisterInternalActivityState,
                activity,
            )])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_survives_the_trip_through_the_attribute_word() {
        // a 64-bit code segment: type 0xB, S, DPL 0, present, long, granularity
        let code = Segment {
            base: 0,
            limit: 0xFFFF_FFFF,
            selector: 0x10,
            type_: 0xB,
            s: 1,
            dpl: 0,
            present: 1,
            avl: 0,
            l: 1,
            db: 0,
            g: 1,
        };
        assert_eq!(segment_to_whp(&code).Attributes, 0xA09B);
        assert_eq!(segment_from_whp(&segment_to_whp(&code)), code);
        // every bit set in every field it has room for
        let all = Segment {
            base: 0xFFFF_8000_0000_0000,
            limit: 0x000F_FFFF,
            selector: 0xFFFB,
            type_: 0xF,
            s: 1,
            dpl: 3,
            present: 1,
            avl: 1,
            l: 1,
            db: 1,
            g: 1,
        };
        assert_eq!(segment_from_whp(&segment_to_whp(&all)), all);
    }

    #[test]
    fn the_names_and_the_value_positions_line_up() {
        // 38 fixed registers, 16 XMM, 8 x87, then the MSRs: the offsets
        // `capture_snapshot` indexes with.
        assert_eq!(capture_names().len(), 38 + 16 + 8 + MSR_MAP.len());
        assert_eq!(capture_names()[62], WHV_REGISTER_NAME::WHvX64RegisterTsc);
        assert_eq!(
            capture_names()[XCR0_INDEX],
            WHV_REGISTER_NAME::WHvX64RegisterXCr0
        );
    }
}
