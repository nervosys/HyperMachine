//! State that belongs to the machine rather than to a vCPU or a device.
//!
//! The interrupt controllers and the timer that sit between devices and
//! vCPUs, and the paravirtual clock. A guest programs all of these while it
//! boots and never again, which is why a snapshot without them looks fine
//! and is not: the unikernel Phase 2 restored programs none of them, and a
//! Linux guest programs every one. Measured on the reference guest here,
//! every interrupt it takes goes through the legacy PICs (`XT-PIC` in
//! `/proc/interrupts`, no IOAPIC), its tick is the PIT, and its clocksource
//! is the TSC with the kvm-clock MSRs registered. A restore into fresh
//! controllers delivers its timer and its virtio interrupts at vectors the
//! guest never set up -- the PICs' vector base is part of what it programmed.
//!
//! Stored as the hypervisor's own bytes. These are KVM's `kvm_irqchip` and
//! `kvm_pit_state2` payloads, not a portable description, and a snapshot is
//! restored by the backend that took it.

use serde::{Deserialize, Serialize};

/// Machine-level state, as the backend that captured it represents it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineState {
    /// Which backend these bytes are for. A restore on another is refused.
    pub backend: String,
    /// The master and slave 8259 PICs.
    pub pic_master: Vec<u8>,
    pub pic_slave: Vec<u8>,
    pub ioapic: Vec<u8>,
    /// The 8254 PIT.
    pub pit: Vec<u8>,
    /// The paravirtual clock's value, in nanoseconds, when captured.
    pub clock_ns: Option<u64>,
}

/// What a restored guest's clock should read.
///
/// A choice with no right answer in general, which is why it is the
/// caller's:
///
/// - [`Self::Fresh`] leaves the new VM's clocks as they are. The guest's
///   time jumps to wherever this host's counters happen to be -- for a TSC
///   clocksource, backwards, since a new vCPU's TSC starts near zero. Right
///   for a guest that keeps no time, which is what Phase 2 restored.
/// - [`Self::Continue`] restores the TSC, the kvm-clock and its MSRs, so
///   the guest resumes as though no time had passed since the snapshot. Its
///   *wall* clock is then stale by that long, which the caller fixes after
///   resuming -- a sandbox daemon sets the date through its guest agent.
///   Right for anything that schedules by time, which is every Linux guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockOnRestore {
    #[default]
    Fresh,
    Continue,
}

/// The MSRs that carry a guest's sense of time. Captured always, restored
/// only under [`ClockOnRestore::Continue`].
///
/// `IA32_TSC`, the kvm-clock wall-clock and system-time registrations, and
/// the other paravirtual registrations that name guest pages KVM writes to
/// (async page fault, steal time, PV EOI), plus the TSC deadline, which is
/// meaningless against a TSC that was not restored.
///
/// Order is restore order. `ASYNC_PF_INT` -- the vector "page ready" is
/// delivered on -- goes before `ASYNC_PF_EN`, which a Linux guest enables
/// with delivery by interrupt. It was missing once: a restored guest then had
/// async page faults enabled on vector 0, which is not a vector. The first
/// page-ready notification could never be delivered, KVM kept it queued, and
/// a queued completion counts as a pending event -- so every `HLT` returned
/// at once: 315,000 halt exits a second from an idle guest, a host core
/// each. It showed only on a guest restored from a snapshot of a guest that
/// had itself been restored, since mapping memory from a file is what makes
/// async page faults happen at all.
pub const CLOCK_MSRS: &[u32] = &[
    0x0000_0010, // IA32_TSC
    0x4b56_4d00, // MSR_KVM_WALL_CLOCK_NEW
    0x4b56_4d01, // MSR_KVM_SYSTEM_TIME_NEW
    0x4b56_4d06, // MSR_KVM_ASYNC_PF_INT
    0x4b56_4d02, // MSR_KVM_ASYNC_PF_EN
    0x4b56_4d03, // MSR_KVM_STEAL_TIME
    0x4b56_4d04, // MSR_KVM_PV_EOI_EN
    0x0000_06e0, // IA32_TSC_DEADLINE
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot written before this existed has no `machine` field; it must
    /// still read, as "nothing captured" rather than as an error.
    #[test]
    fn a_header_without_machine_state_still_reads() {
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            machine: Option<MachineState>,
        }
        let old: Old = serde_json::from_str("{}").unwrap();
        assert!(old.machine.is_none());
    }

    #[test]
    fn fresh_is_the_default_because_it_was_the_behaviour() {
        assert_eq!(ClockOnRestore::default(), ClockOnRestore::Fresh);
    }
}
