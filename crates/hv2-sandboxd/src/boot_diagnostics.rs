//! Failure-only formatting of state read by the vCPU execution owner.

use hv2_core::snapshot::vcpu::VCpuSnapshot;

pub(crate) fn owner_sample(state: &VCpuSnapshot) -> String {
    let architecture = format!(
        "vCPU {} owner sample: RIP={:#x} RFLAGS={:#x} CR3={:#x} run_state={:?} RSP={:#x}",
        state.id,
        state.general.rip,
        state.general.rflags,
        state.system.cr3,
        state.run_state,
        state.general.rsp,
    );
    // KVM_GET_LAPIC exports a 1024-byte xAPIC register image. Registers are
    // little endian, with 16-byte spacing even in the ISR/IRR bitmaps.
    // Keep absent/malformed images distinct from a captured all-zero image.
    if state.lapic.len() != 1024 {
        return format!(
            "{architecture}; LAPIC unavailable ({} bytes)",
            state.lapic.len()
        );
    }
    let register =
        |offset: usize| u32::from_le_bytes(state.lapic[offset..offset + 4].try_into().unwrap());
    let bitmap = |base: usize| {
        (0usize..8)
            .map(|word| format!("{:08x}", register(base + word * 16)))
            .collect::<Vec<_>>()
            .join("/")
    };
    // These are raw exported values, not a claim about the live countdown
    // after the owner resumes. The kick can also change the reported MP state.
    format!(
        "{architecture}; LAPIC TPR={:#x} PPR={:#x} SVR={:#x} LVT_TIMER={:#x} LVT0={:#x} TIMER_INITIAL={:#x} TIMER_CURRENT={:#x} TIMER_DIVIDE={:#x} ISR={} IRR={}",
        register(0x80), register(0xa0), register(0xf0), register(0x320),
        register(0x350), register(0x380), register(0x390), register(0x3e0),
        bitmap(0x100), bitmap(0x200),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_apic_is_not_reported_as_no_pending_interrupts() {
        for len in [0, 1023, 1025] {
            let state = VCpuSnapshot {
                lapic: vec![0; len],
                ..Default::default()
            };
            let report = owner_sample(&state);
            assert!(report.contains("LAPIC unavailable"));
            assert!(!report.contains("IRR="));
        }
    }

    #[test]
    fn irq_bitmaps_use_register_spacing_and_preserve_all_words() {
        let mut state = VCpuSnapshot {
            lapic: vec![0; 1024],
            ..Default::default()
        };
        // Poison the padding: reading contiguous u32 words invents IRQs.
        state.lapic[0x104..0x110].fill(0xff);
        state.lapic[0x100..0x104].copy_from_slice(&0x80000000u32.to_le_bytes());
        state.lapic[0x270..0x274].copy_from_slice(&1u32.to_le_bytes());
        state.lapic[0x320..0x324].copy_from_slice(&0x10020u32.to_le_bytes());
        let report = owner_sample(&state);
        assert!(report.contains(
            "ISR=80000000/00000000/00000000/00000000/00000000/00000000/00000000/00000000"
        ));
        assert!(report.contains(
            "IRR=00000000/00000000/00000000/00000000/00000000/00000000/00000000/00000001"
        ));
        assert!(report.contains("LVT_TIMER=0x10020"));
    }
}
