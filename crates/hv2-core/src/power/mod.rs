//! Power Management Module
//!
//! This module provides comprehensive power management for the hypervisor,
//! including ACPI S-states (system power states), CPU C-states (idle states),
//! CPU P-states (performance states), device power states, and wake event handling.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Power Management                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │  S-State Manager     │  C-State Manager  │  P-State Manager │
//! │  (System Power)      │  (CPU Idle)       │  (CPU Frequency) │
//! ├──────────────────────┼───────────────────┼──────────────────┤
//! │  S0 (Working)        │  C0 (Active)      │  P0 (Max Freq)   │
//! │  S1 (Standby)        │  C1 (Halt)        │  P1              │
//! │  S2 (Sleep)          │  C1E (Enhanced)   │  P2              │
//! │  S3 (Suspend-RAM)    │  C2 (Stop-Clock)  │  ...             │
//! │  S4 (Hibernate)      │  C3 (Sleep)       │  Pn (Min Freq)   │
//! │  S5 (Soft-Off)       │  C6/C7/C8/C10     │                  │
//! ├──────────────────────┴───────────────────┴──────────────────┤
//! │                    Wake Sources & Events                     │
//! │  PowerButton, RTC, PCI-PME, USB, Network, Keyboard, etc.    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - **S-State Management**: System-wide power states for suspend/hibernate
//! - **C-State Management**: Per-CPU idle states with governor algorithms
//! - **P-State Management**: CPU frequency scaling for performance/power balance
//! - **Wake Event Handling**: Configurable wake sources and event tracking
//! - **Power Statistics**: Comprehensive tracking of power state usage
//!
//! # Example
//!
//! ```rust
//! use hv2_core::power::{SStateManager, CStateManager, PStateManager};
//! use hv2_core::power::{CStateGovernor, PStateGovernor};
//!
//! // Create managers
//! let mut sstate = SStateManager::new();
//! let mut cstate = CStateManager::new(4);
//! let mut pstate = PStateManager::new(4);
//!
//! // Configure governors
//! cstate.set_governor(CStateGovernor::Menu);
//! pstate.set_governor(PStateGovernor::Ondemand);
//!
//! // Enter CPU idle
//! let idle_state = cstate.enter_idle(0).unwrap();
//!
//! // Update CPU utilization and run frequency governor
//! pstate.update_utilization(0, 75).unwrap();
//! pstate.run_governor(0).unwrap();
//! ```

mod cstate;
mod pstate;
mod sstate;
mod types;

pub use cstate::{CStateError, CStateGovernor, CStateManager, CStateResult, CpuCState};
pub use pstate::{CpuPState, PStateError, PStateGovernor, PStateManager, PStateResult};
pub use sstate::{SStateError, SStateManager, SStateResult, TransitionPhase, WakeSourceConfig};
pub use types::{
    BatteryEventType, CState, DState, PState, PowerEvent, PowerStats, SState, ThermalEventType,
    ThermalTripType, WakeEvent, WakeSource,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_module_integration() {
        // Create all managers
        let sstate = SStateManager::new();
        let cstate = CStateManager::new(4);
        let pstate = PStateManager::new(4);

        // All should start in active states
        assert_eq!(sstate.current_state(), SState::S0);
        assert_eq!(cstate.current_state(0).unwrap(), CState::C0);
        assert_eq!(pstate.current_frequency(0).unwrap(), 800);
    }

    #[test]
    fn test_s_state_suspend_wake_cycle() {
        let mut manager = SStateManager::new();

        // Request suspend
        manager.request_transition(SState::S3).unwrap();

        // Advance to sleeping
        while manager.transition_phase() != TransitionPhase::Sleeping {
            manager.advance_transition().unwrap();
        }

        assert_eq!(manager.current_state(), SState::S3);

        // Wake up
        manager.wake(WakeSource::PowerButton).unwrap();

        // Complete wake
        while manager.is_transitioning() {
            manager.advance_transition().unwrap();
        }

        assert_eq!(manager.current_state(), SState::S0);
    }

    #[test]
    fn test_c_state_idle_cycle() {
        let mut manager = CStateManager::new(2);

        // Enter idle on CPU 0
        let state = manager.enter_idle(0).unwrap();
        assert!(!state.is_active() || state == CState::C0);

        // Exit idle
        manager.exit_idle(0).unwrap();
        assert_eq!(manager.current_state(0).unwrap(), CState::C0);
    }

    #[test]
    fn test_p_state_frequency_scaling() {
        let mut manager = PStateManager::new(1);
        manager.set_governor(PStateGovernor::Ondemand);

        // Simulate high load
        manager.update_utilization(0, 95).unwrap();
        manager.run_governor(0).unwrap();

        // Should be at higher frequency
        let freq = manager.current_frequency(0).unwrap();
        assert!(freq > 800);
    }

    #[test]
    fn test_power_event_flow() {
        let mut sstate = SStateManager::new();
        let mut cstate = CStateManager::new(1);
        let mut pstate = PStateManager::new(1);

        // Generate events
        sstate.request_transition(SState::S1).unwrap();
        cstate.enter_idle(0).unwrap();
        pstate.set_governor(PStateGovernor::Performance);
        pstate.run_governor(0).unwrap();

        // All should have events
        assert!(sstate.poll_event().is_some());
        assert!(cstate.poll_event().is_some());
        assert!(pstate.poll_event().is_some());
    }

    #[test]
    fn test_wake_source_configuration() {
        let mut config = WakeSourceConfig::default();
        config.power_button = true;
        config.rtc_alarm = true;
        config.enable_pci_wake(0x0100);

        assert!(config.is_enabled(&WakeSource::PowerButton));
        assert!(config.is_enabled(&WakeSource::RtcAlarm));
        assert!(config.is_enabled(&WakeSource::PciPme(0x0100)));
        assert!(!config.is_enabled(&WakeSource::Keyboard));
    }

    #[test]
    fn test_power_stats_tracking() {
        let mut sstate = SStateManager::new();

        // Do a sleep/wake cycle
        sstate.request_transition(SState::S3).unwrap();
        while sstate.transition_phase() != TransitionPhase::Sleeping {
            sstate.advance_transition().unwrap();
        }

        sstate.wake(WakeSource::RtcAlarm).unwrap();
        while sstate.is_transitioning() {
            sstate.advance_transition().unwrap();
        }

        let stats = sstate.stats();
        assert_eq!(stats.wake_events, 1);
        assert_eq!(stats.last_wake_source, Some(WakeSource::RtcAlarm));
    }

    #[test]
    fn test_governor_selection() {
        let mut cstate = CStateManager::new(1);
        let mut pstate = PStateManager::new(1);

        // Test C-state governors
        cstate.set_governor(CStateGovernor::Menu);
        assert_eq!(cstate.governor(), CStateGovernor::Menu);

        cstate.set_governor(CStateGovernor::Ladder);
        assert_eq!(cstate.governor(), CStateGovernor::Ladder);

        // Test P-state governors
        pstate.set_governor(PStateGovernor::Ondemand);
        assert_eq!(pstate.governor(), PStateGovernor::Ondemand);

        pstate.set_governor(PStateGovernor::Performance);
        assert_eq!(pstate.governor(), PStateGovernor::Performance);
    }

    #[test]
    fn test_device_power_states() {
        let states = [
            DState::D0,
            DState::D1,
            DState::D2,
            DState::D3Hot,
            DState::D3Cold,
        ];

        for state in &states {
            if *state == DState::D0 {
                assert!(state.is_operational());
            }
            if *state == DState::D3Cold {
                assert!(!state.has_power());
            }
        }
    }

    #[test]
    fn test_thermal_events() {
        let event = PowerEvent::ThermalEvent(ThermalEventType::TripPoint {
            zone: 0,
            trip_type: ThermalTripType::Hot,
        });

        if let PowerEvent::ThermalEvent(event_type) = event {
            assert!(matches!(
                event_type,
                ThermalEventType::TripPoint {
                    zone: 0,
                    trip_type: ThermalTripType::Hot
                }
            ));
        }
    }
}
