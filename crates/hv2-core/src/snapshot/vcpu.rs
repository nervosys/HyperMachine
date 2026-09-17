//! A vCPU's architectural state, captured and restored.
//!
//! # Why this is its own type
//!
//! [`crate::vcpu::RegisterSet`] is the host's *idea* of a vCPU's registers --
//! what a caller set, what an exit reported. This is the hardware's, read back
//! from the hypervisor at a moment when the guest is not running. The two are
//! not interchangeable: a guest that has executed a million instructions since
//! the last thing the host set has a `RegisterSet` describing none of them.
//!
//! # What it holds, and what that is enough for
//!
//! General-purpose registers and `RFLAGS`/`RIP`; the segment, descriptor-table
//! and control registers; the x87/SSE state; and the multiprocessing state that
//! says whether this vCPU is running or halted waiting for an interrupt.
//!
//! That is enough to put a vCPU back exactly where it was **for the guests this
//! runs today**. It is not enough in general, and the gaps are named here
//! rather than found later:
//!
//! MSRs are captured too: `SYSCALL`'s entry point and flag mask, the `FS`/`GS`
//! bases, the `SYSENTER` trio and `PAT`. Those are the ones a guest notices
//! losing -- a 64-bit Linux guest restored without `LSTAR` jumps somewhere
//! that is not its system-call entry within microseconds.
//!
//! What is still missing, and named here rather than found later:
//!
//! - **The local APIC is not captured.** A restored vCPU loses in-flight
//!   interrupt state, so a timer already armed does not fire.
//! - **`XSAVE` is not captured**, only the legacy FPU area, so AVX register
//!   contents are lost.
//! - **The TSC is deliberately not captured.** Restoring it makes the guest's
//!   clock jump by however long the snapshot sat on disk; not restoring it
//!   makes the clock jump to the host's uptime. Both are wrong, and choosing
//!   needs a caller who knows what the guest does with time.
//!
//! Each of those has its ioctl already defined in `kvm_ffi`, and
//! [`VCpuSnapshot::is_complete`] answers `false` so that a caller can tell
//! this apart from a full capture rather than assuming.

use serde::{Deserialize, Serialize};

/// The general-purpose register file, plus `RIP` and `RFLAGS`.
///
/// Field order and names follow the hardware's, not the hypervisor's struct
/// layout, so this stays readable when written to a file and diffed by a
/// human trying to work out why a restore went wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

/// One segment register, as the hardware holds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    pub type_: u8,
    pub present: u8,
    pub dpl: u8,
    pub db: u8,
    pub s: u8,
    pub l: u8,
    pub g: u8,
    pub avl: u8,
}

/// A descriptor table register (`GDTR`/`IDTR`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptorTable {
    pub base: u64,
    pub limit: u16,
}

/// Segment, descriptor-table and control registers.
///
/// The ones that decide what the general-purpose registers *mean*: paging
/// mode, privilege level, and where every segment points. Restoring the
/// general registers without these puts correct values into a machine that
/// interprets them differently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemRegisters {
    pub cs: Segment,
    pub ds: Segment,
    pub es: Segment,
    pub fs: Segment,
    pub gs: Segment,
    pub ss: Segment,
    pub tr: Segment,
    pub ldt: Segment,
    pub gdt: DescriptorTable,
    pub idt: DescriptorTable,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
    pub apic_base: u64,
}

/// x87 and SSE state.
///
/// Stored as raw bytes rather than parsed fields: nothing here interprets it,
/// and a structure that is only ever written back verbatim is one more thing
/// to get subtly wrong for no benefit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpuState {
    /// The eight x87 registers, 16 bytes each.
    pub fpr: Vec<u8>,
    /// The sixteen XMM registers, 16 bytes each.
    pub xmm: Vec<u8>,
    pub fcw: u16,
    pub fsw: u16,
    pub ftwx: u8,
    pub last_opcode: u16,
    pub last_ip: u64,
    pub last_dp: u64,
    pub mxcsr: u32,
}

impl Default for FpuState {
    fn default() -> Self {
        Self {
            fpr: vec![0; 8 * 16],
            xmm: vec![0; 16 * 16],
            fcw: 0,
            fsw: 0,
            ftwx: 0,
            last_opcode: 0,
            last_ip: 0,
            last_dp: 0,
            mxcsr: 0,
        }
    }
}

/// Whether a vCPU is executing or waiting.
///
/// A vCPU halted in `HLT` waiting for an interrupt is not the same as one
/// about to execute, and restoring the second where the first was gives a
/// guest that runs an instruction it already ran.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    /// Executing normally.
    #[default]
    Runnable,
    /// Halted, waiting for an interrupt.
    Halted,
    /// Anything else the hypervisor reported, by its own number. Kept rather
    /// than mapped to `Runnable`, because guessing here restarts a vCPU that
    /// was deliberately not running.
    Other(u32),
}

/// One model-specific register.
///
/// Stored as a pair rather than named fields, so a host that lacks one of them
/// simply records fewer rather than needing a representation for "absent", and
/// so adding another to the captured set does not change this type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Msr {
    /// The architectural MSR number, e.g. `0xc000_0082` for `LSTAR`.
    pub index: u32,
    pub value: u64,
}

/// Everything captured for one vCPU.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VCpuSnapshot {
    pub id: u32,
    pub general: GeneralRegisters,
    pub system: SystemRegisters,
    pub fpu: FpuState,
    /// The model-specific registers listed in `kvm_ffi::SNAPSHOT_MSRS`, minus
    /// any this host does not implement. Empty on a backend that cannot read
    /// them at all, which is why [`VCpuSnapshot::is_complete`] asks rather
    /// than assuming.
    #[serde(default)]
    pub msrs: Vec<Msr>,
    pub run_state: RunState,
}

impl VCpuSnapshot {
    /// Does this hold everything needed to restore any guest exactly?
    ///
    /// Always `false` today, and deliberately a method rather than a constant
    /// so the answer can become conditional when MSRs, the LAPIC and `XSAVE`
    /// are captured. A caller deciding whether a restore is safe for its
    /// workload should ask rather than assume, and a caller that never asks
    /// gets the conservative answer by never seeing `true`.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        false
    }

    /// What a complete capture would add, for an error message or a log line.
    #[must_use]
    pub fn missing() -> &'static [&'static str] {
        &[
            "local APIC state (in-flight and armed interrupts)",
            "XSAVE area (AVX and later register state)",
            "the TSC, deliberately: restoring it jumps the guest's clock and \
             not restoring it jumps it too, so the choice belongs to a caller",
        ]
    }

    /// One captured MSR's value, if it was captured.
    ///
    /// By index rather than by position: which MSRs a snapshot holds depends
    /// on what its host implemented, so the third entry is not reliably the
    /// same register across two machines.
    #[must_use]
    pub fn msr(&self, index: u32) -> Option<u64> {
        self.msrs
            .iter()
            .find(|msr| msr.index == index)
            .map(|msr| msr.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_survives_being_written_and_read() {
        // It goes to a file and comes back, so a field that does not
        // round-trip is a vCPU restored wrong -- which shows up as a guest
        // that crashes somewhere unrelated, long after the mistake.
        let mut snapshot = VCpuSnapshot {
            id: 3,
            ..Default::default()
        };
        snapshot.general.rip = 0xffff_8000_0000_1234;
        snapshot.general.rsp = 0x7fff_ffff_0000;
        snapshot.system.cr3 = 0x1000;
        snapshot.system.cs.selector = 0x10;
        snapshot.system.cs.l = 1;
        snapshot.run_state = RunState::Halted;
        snapshot.fpu.mxcsr = 0x1f80;

        let encoded = serde_json::to_vec(&snapshot).expect("encode");
        let decoded: VCpuSnapshot = serde_json::from_slice(&encoded).expect("decode");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn a_halted_vcpu_does_not_come_back_runnable() {
        // The distinction is the whole reason `run_state` is captured: a vCPU
        // that was halted waiting for an interrupt, restored as runnable,
        // re-executes the instruction after the HLT it never left.
        let halted = VCpuSnapshot {
            run_state: RunState::Halted,
            ..Default::default()
        };
        let decoded: VCpuSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&halted).expect("encode")).expect("decode");
        assert_eq!(decoded.run_state, RunState::Halted);
        assert_ne!(decoded.run_state, RunState::Runnable);
    }

    #[test]
    fn an_unrecognised_run_state_is_kept_not_flattened() {
        // KVM has more states than these two. Mapping an unknown one to
        // Runnable would restart a vCPU that was deliberately not running --
        // an uninitialised application processor, for instance.
        let other = VCpuSnapshot {
            run_state: RunState::Other(7),
            ..Default::default()
        };
        let decoded: VCpuSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&other).expect("encode")).expect("decode");
        assert_eq!(decoded.run_state, RunState::Other(7));
    }

    #[test]
    fn a_capture_says_it_is_not_complete() {
        // If this ever returns true without the LAPIC and XSAVE being
        // captured, a caller will restore a guest that uses them and get a
        // failure with no connection to its cause.
        assert!(!VCpuSnapshot::default().is_complete());
        assert_eq!(VCpuSnapshot::missing().len(), 3);
    }

    #[test]
    fn msrs_survive_the_round_trip_and_are_found_by_index() {
        // `LSTAR` is where SYSCALL lands. A guest restored with the wrong one
        // jumps somewhere that is not its system-call entry within
        // microseconds, so this travelling correctly is the difference
        // between a restored Linux guest and a crashed one.
        let snapshot = VCpuSnapshot {
            msrs: vec![
                Msr {
                    index: 0xc000_0082,
                    value: 0xffff_ffff_8100_0000,
                },
                Msr {
                    index: 0xc000_0100,
                    value: 0x7f00_0000_0000,
                },
            ],
            ..Default::default()
        };
        let decoded: VCpuSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).expect("encode"))
                .expect("decode");
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.msr(0xc000_0082), Some(0xffff_ffff_8100_0000));
        assert_eq!(decoded.msr(0xc000_0100), Some(0x7f00_0000_0000));
        // Not captured is not the same as zero.
        assert_eq!(decoded.msr(0xc000_0081), None);
    }

    #[test]
    fn a_snapshot_written_before_msrs_existed_still_reads() {
        // `msrs` is `#[serde(default)]`, which is only worth having if a file
        // without the field decodes rather than failing.
        let without = br#"{"id":0,"general":{"rax":0,"rbx":0,"rcx":0,"rdx":0,"rsi":0,"rdi":0,
            "rsp":0,"rbp":0,"r8":0,"r9":0,"r10":0,"r11":0,"r12":0,"r13":0,"r14":0,"r15":0,
            "rip":0,"rflags":0},"system":{"cs":{"base":0,"limit":0,"selector":0,"type_":0,
            "present":0,"dpl":0,"db":0,"s":0,"l":0,"g":0,"avl":0},"ds":{"base":0,"limit":0,
            "selector":0,"type_":0,"present":0,"dpl":0,"db":0,"s":0,"l":0,"g":0,"avl":0},
            "es":{"base":0,"limit":0,"selector":0,"type_":0,"present":0,"dpl":0,"db":0,"s":0,
            "l":0,"g":0,"avl":0},"fs":{"base":0,"limit":0,"selector":0,"type_":0,"present":0,
            "dpl":0,"db":0,"s":0,"l":0,"g":0,"avl":0},"gs":{"base":0,"limit":0,"selector":0,
            "type_":0,"present":0,"dpl":0,"db":0,"s":0,"l":0,"g":0,"avl":0},"ss":{"base":0,
            "limit":0,"selector":0,"type_":0,"present":0,"dpl":0,"db":0,"s":0,"l":0,"g":0,
            "avl":0},"tr":{"base":0,"limit":0,"selector":0,"type_":0,"present":0,"dpl":0,
            "db":0,"s":0,"l":0,"g":0,"avl":0},"ldt":{"base":0,"limit":0,"selector":0,
            "type_":0,"present":0,"dpl":0,"db":0,"s":0,"l":0,"g":0,"avl":0},
            "gdt":{"base":0,"limit":0},"idt":{"base":0,"limit":0},"cr0":0,"cr2":0,"cr3":0,
            "cr4":0,"cr8":0,"efer":0,"apic_base":0},"fpu":{"fpr":[],"xmm":[],"fcw":0,"fsw":0,
            "ftwx":0,"last_opcode":0,"last_ip":0,"last_dp":0,"mxcsr":0},"run_state":"Runnable"}"#;
        let decoded: VCpuSnapshot = serde_json::from_slice(without).expect("decode");
        assert!(decoded.msrs.is_empty());
    }

    #[test]
    fn the_fpu_default_is_the_right_shape() {
        // Sized here rather than at the call site: a short `fpr` written back
        // through KVM_SET_FPU would read past the end of the buffer.
        let fpu = FpuState::default();
        assert_eq!(fpu.fpr.len(), 8 * 16);
        assert_eq!(fpu.xmm.len(), 16 * 16);
    }
}
