//! Failure-only formatting of owner and machine-level interrupt samples.

use hv2_core::hypervisor::VCpuDiagnostic;
use hv2_core::snapshot::machine::MachineState;
use hv2_core::snapshot::vcpu::VCpuSnapshot;

pub(crate) fn owner_diagnostic(sample: &VCpuDiagnostic) -> String {
    let architecture = owner_sample(&sample.architecture);
    let events = match &sample.interrupts {
        Ok(Some(state)) => format!(
            "EVENTS FLAGS={:#x} IRQ_INJECTED={} VECTOR={:#x} SOFT={} SHADOW={:?} EXCEPTION_INJECTED={} EXCEPTION_VECTOR={} EXCEPTION_PENDING={:?} NMI_INJECTED={} NMI_PENDING={:?} NMI_MASKED={}",
            state.flags, state.injected, state.vector, state.soft, state.shadow,
            state.exception_injected, state.exception_vector, state.exception_pending,
            state.nmi_injected, state.nmi_pending, state.nmi_masked,
        ),
        Ok(None) => "EVENTS unavailable (unsupported backend)".into(),
        Err(error) => format!("EVENTS unavailable: {error}"),
    };
    let retries = match sample.run_retries {
        Some(state) => format!("RUN_RETRIES EINTR={} EAGAIN={}", state.eintr, state.eagain),
        None => "RUN_RETRIES unavailable".into(),
    };
    format!("{architecture}; {events}; {retries}")
}

pub(crate) fn machine_sample(state: &MachineState) -> String {
    if state.backend != "kvm" {
        return "pre-kick PIC/PIT sample unavailable (non-KVM backend)".into();
    }
    // KVM's 512-byte irqchip union starts with the 16-byte kvm_pic_state.
    // Capture each controller separately; this is not an atomic snapshot.
    let pic = |name: &str, bytes: &[u8]| {
        if bytes.len() != 512 {
            return format!("{name} unavailable ({} bytes)", bytes.len());
        }
        format!(
            "{name} LAST_IRR={:#x} IRR={:#x} IMR={:#x} ISR={:#x} PRIORITY={:#x} BASE={:#x} INIT={} AUTO_EOI={} ELCR={:#x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[9], bytes[10], bytes[14],
        )
    };
    let pit = if state.pit.len() == 112 {
        // Three 24-byte channel structs, then u32 flags and nine reserved u32s.
        // count is u32 (65536 is valid), not the guest's 16-bit port value.
        let channels = (0..3)
            .map(|index| {
                let bytes = &state.pit[index * 24..(index + 1) * 24];
                let count = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                let loaded = i64::from_le_bytes(bytes[16..24].try_into().unwrap());
                format!(
                    "CH{index} COUNT={count} MODE={} GATE={} RW={} READ={} WRITE={} LOADED_NS={loaded}",
                    bytes[13], bytes[15], bytes[12], bytes[9], bytes[10],
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let flags = u32::from_le_bytes(state.pit[72..76].try_into().unwrap());
        format!("PIT FLAGS={flags:#x} {channels}")
    } else {
        format!("PIT unavailable ({} bytes)", state.pit.len())
    };
    format!(
        "pre-kick machine sample: {}; {}; {pit}",
        pic("PIC_MASTER", &state.pic_master),
        pic("PIC_SLAVE", &state.pic_slave),
    )
}

pub(crate) fn owner_sample(state: &VCpuSnapshot) -> String {
    // Already captured by the owner-safe snapshot. Reads of different MSRs
    // are sequential, so these are evidence, not an atomic timer comparison.
    let clock = |index: u32| match state.msrs.iter().find(|msr| msr.index == index) {
        Some(msr) => format!("{:#x}", msr.value),
        None => "unavailable".into(),
    };
    let architecture = format!(
        "vCPU {} owner sample: RIP={:#x} RFLAGS={:#x} CR3={:#x} run_state={:?} RSP={:#x} APIC_BASE={:#x} CR8={:#x} TSC={} TSC_DEADLINE={}",
        state.id,
        state.general.rip,
        state.general.rflags,
        state.system.cr3,
        state.run_state,
        state.general.rsp,
        state.system.apic_base,
        state.system.cr8,
        clock(0x10),
        clock(0x6e0),
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
    fn clock_samples_distinguish_missing_zero_and_large_values() {
        use hv2_core::snapshot::vcpu::Msr;
        let mut state = VCpuSnapshot::default();
        assert!(owner_sample(&state).contains("TSC=unavailable TSC_DEADLINE=unavailable"));
        state.msrs = vec![
            Msr {
                index: 0x6e0,
                value: 0,
            },
            Msr {
                index: 0x10,
                value: u64::MAX,
            },
        ];
        assert!(owner_sample(&state).contains("TSC=0xffffffffffffffff TSC_DEADLINE=0x0"));
        state.msrs[0].value = 0x1234;
        assert!(owner_sample(&state).contains("TSC_DEADLINE=0x1234"));
    }

    #[test]
    fn unavailable_events_preserve_architecture_without_inventing_zero_state() {
        for interrupts in [
            Ok(None),
            Err(hv2_core::Error::NotSupported("event read failed".into())),
        ] {
            let mut architecture = VCpuSnapshot::default();
            architecture.general.rip = 0x1234;
            let report = owner_diagnostic(&VCpuDiagnostic {
                architecture,
                interrupts,
                run_retries: None,
            });
            assert!(report.contains("RIP=0x1234"));
            assert!(report.contains("EVENTS unavailable"));
            assert!(!report.contains("IRQ_INJECTED="));
        }
    }

    #[test]
    fn machine_sample_refuses_foreign_and_truncated_layouts() {
        let mut state = MachineState::default();
        assert!(machine_sample(&state).contains("non-KVM backend"));
        state.backend = "kvm".into();
        for len in [0, 111, 113] {
            state.pit = vec![0; len];
            state.pic_master = vec![0; 16];
            let report = machine_sample(&state);
            assert!(report.contains("PIT unavailable"));
            assert!(report.contains("PIC_MASTER unavailable"));
            assert!(!report.contains("COUNT="));
            assert!(!report.contains("IRR="));
        }
    }

    #[test]
    fn machine_sample_preserves_pic_irq_bits_and_full_pit_count() {
        let mut state = MachineState {
            backend: "kvm".into(),
            pic_master: vec![0; 512],
            pic_slave: vec![0; 512],
            pit: vec![0; 112],
            ..Default::default()
        };
        state.pic_master[1..4].copy_from_slice(&[1, 0xfe, 0x10]);
        state.pit[..4].copy_from_slice(&65536u32.to_le_bytes());
        state.pit[13] = 2;
        state.pit[15] = 1;
        state.pit[24..28].copy_from_slice(&123u32.to_le_bytes());
        state.pit[48..52].copy_from_slice(&456u32.to_le_bytes());
        state.pit[72..76].copy_from_slice(&2u32.to_le_bytes());
        let report = machine_sample(&state);
        assert!(report.contains("IRR=0x1 IMR=0xfe ISR=0x10"));
        assert!(report.contains("CH0 COUNT=65536 MODE=2 GATE=1"));
        assert!(report.contains("CH1 COUNT=123"));
        assert!(report.contains("CH2 COUNT=456"));
        assert!(report.contains("PIT FLAGS=0x2"));
    }

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
        state.system.apic_base = 0xfee00900;
        state.system.cr8 = 1;
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
        assert!(report.contains("APIC_BASE=0xfee00900 CR8=0x1"));
    }
}
