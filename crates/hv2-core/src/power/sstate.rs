//! ACPI S-state (System Power State) management
//!
//! This module provides management of system-wide power states (S0-S5)
//! including suspend, hibernate, and shutdown transitions.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use super::types::{PowerEvent, PowerStats, SState, WakeEvent, WakeSource};

/// S-state transition result
pub type SStateResult<T> = Result<T, SStateError>;

/// S-state management error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SStateError {
    /// Transition not allowed from current state
    InvalidTransition { from: SState, to: SState },
    /// State not supported
    StateNotSupported(SState),
    /// Transition in progress
    TransitionInProgress,
    /// Wake source not enabled
    WakeSourceDisabled(WakeSource),
    /// Device blocking transition
    DeviceBlocking(String),
    /// Timeout during transition
    Timeout,
    /// Memory save failed
    MemorySaveFailed,
    /// Resume failed
    ResumeFailed,
}

impl std::fmt::Display for SStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SStateError::InvalidTransition { from, to } => {
                write!(f, "Invalid transition from {} to {}", from, to)
            }
            SStateError::StateNotSupported(s) => write!(f, "State {} not supported", s),
            SStateError::TransitionInProgress => write!(f, "Transition already in progress"),
            SStateError::WakeSourceDisabled(w) => write!(f, "Wake source {} disabled", w),
            SStateError::DeviceBlocking(d) => write!(f, "Device {} blocking transition", d),
            SStateError::Timeout => write!(f, "Transition timeout"),
            SStateError::MemorySaveFailed => write!(f, "Failed to save memory state"),
            SStateError::ResumeFailed => write!(f, "Failed to resume"),
        }
    }
}

impl std::error::Error for SStateError {}

/// S-state transition phase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionPhase {
    /// No transition in progress
    None,
    /// Preparing devices for sleep
    PreparingDevices,
    /// Saving device state
    SavingDeviceState,
    /// Saving CPU state
    SavingCpuState,
    /// Saving memory (S4 only)
    SavingMemory,
    /// Entering target state
    EnteringState,
    /// In sleep state
    Sleeping,
    /// Restoring memory (S4 only)
    RestoringMemory,
    /// Restoring CPU state
    RestoringCpuState,
    /// Restoring device state
    RestoringDeviceState,
    /// Finalizing wake
    Finalizing,
}

impl std::fmt::Display for TransitionPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransitionPhase::None => write!(f, "None"),
            TransitionPhase::PreparingDevices => write!(f, "Preparing Devices"),
            TransitionPhase::SavingDeviceState => write!(f, "Saving Device State"),
            TransitionPhase::SavingCpuState => write!(f, "Saving CPU State"),
            TransitionPhase::SavingMemory => write!(f, "Saving Memory"),
            TransitionPhase::EnteringState => write!(f, "Entering State"),
            TransitionPhase::Sleeping => write!(f, "Sleeping"),
            TransitionPhase::RestoringMemory => write!(f, "Restoring Memory"),
            TransitionPhase::RestoringCpuState => write!(f, "Restoring CPU State"),
            TransitionPhase::RestoringDeviceState => write!(f, "Restoring Device State"),
            TransitionPhase::Finalizing => write!(f, "Finalizing"),
        }
    }
}

/// Wake source enable flags
#[derive(Debug, Clone, Default)]
pub struct WakeSourceConfig {
    /// Power button can wake
    pub power_button: bool,
    /// Sleep button can wake
    pub sleep_button: bool,
    /// Lid can wake
    pub lid: bool,
    /// RTC alarm can wake
    pub rtc_alarm: bool,
    /// PCI PME can wake (bitmask of BDFs)
    pub pci_pme: Vec<u16>,
    /// USB can wake
    pub usb: bool,
    /// Network (WoL) can wake
    pub network: bool,
    /// Keyboard can wake
    pub keyboard: bool,
    /// Mouse can wake
    pub mouse: bool,
}

impl WakeSourceConfig {
    /// Create with all sources enabled
    pub fn all_enabled() -> Self {
        Self {
            power_button: true,
            sleep_button: true,
            lid: true,
            rtc_alarm: true,
            pci_pme: Vec::new(),
            usb: true,
            network: true,
            keyboard: true,
            mouse: true,
        }
    }

    /// Create with minimum sources (power button only)
    pub fn minimal() -> Self {
        Self {
            power_button: true,
            ..Default::default()
        }
    }

    /// Check if a wake source is enabled
    pub fn is_enabled(&self, source: &WakeSource) -> bool {
        match source {
            WakeSource::PowerButton => self.power_button,
            WakeSource::SleepButton => self.sleep_button,
            WakeSource::LidOpen => self.lid,
            WakeSource::RtcAlarm => self.rtc_alarm,
            WakeSource::PciPme(bdf) => self.pci_pme.contains(bdf),
            WakeSource::Usb => self.usb,
            WakeSource::Network => self.network,
            WakeSource::Keyboard => self.keyboard,
            WakeSource::Mouse => self.mouse,
            WakeSource::Timer => true, // Always enabled
            WakeSource::ExternalInterrupt(_) => true,
            WakeSource::Software => true,
        }
    }

    /// Enable a PCI device for wake
    pub fn enable_pci_wake(&mut self, bdf: u16) {
        if !self.pci_pme.contains(&bdf) {
            self.pci_pme.push(bdf);
        }
    }

    /// Disable a PCI device for wake
    pub fn disable_pci_wake(&mut self, bdf: u16) {
        self.pci_pme.retain(|&b| b != bdf);
    }
}

/// S-state manager for system power state control
#[derive(Debug)]
pub struct SStateManager {
    /// Current power state
    current_state: AtomicU8,
    /// Target state for transition
    target_state: AtomicU8,
    /// Transition in progress
    transitioning: AtomicBool,
    /// Current transition phase
    transition_phase: TransitionPhase,
    /// Supported S-states
    supported_states: [bool; 6],
    /// Wake source configuration
    wake_config: WakeSourceConfig,
    /// Recent wake events
    wake_history: VecDeque<WakeEvent>,
    /// Maximum wake history entries
    max_wake_history: usize,
    /// State entry timestamps
    state_entry_time: Option<Instant>,
    /// Statistics
    stats: PowerStats,
    /// Event queue
    events: VecDeque<PowerEvent>,
    /// Devices that must be notified
    device_count: usize,
    /// Devices that have completed transition
    devices_ready: usize,
}

impl SStateManager {
    /// Create a new S-state manager
    pub fn new() -> Self {
        Self {
            current_state: AtomicU8::new(SState::S0 as u8),
            target_state: AtomicU8::new(SState::S0 as u8),
            transitioning: AtomicBool::new(false),
            transition_phase: TransitionPhase::None,
            supported_states: [true, true, false, true, true, true], // S0, S1, S3, S4, S5
            wake_config: WakeSourceConfig::all_enabled(),
            wake_history: VecDeque::new(),
            max_wake_history: 100,
            state_entry_time: Some(Instant::now()),
            stats: PowerStats::new(1, 1),
            events: VecDeque::new(),
            device_count: 0,
            devices_ready: 0,
        }
    }

    /// Create with custom supported states
    pub fn with_supported_states(mut self, supported: [bool; 6]) -> Self {
        self.supported_states = supported;
        self
    }

    /// Create with specific wake configuration
    pub fn with_wake_config(mut self, config: WakeSourceConfig) -> Self {
        self.wake_config = config;
        self
    }

    /// Get the current S-state
    pub fn current_state(&self) -> SState {
        SState::from(self.current_state.load(Ordering::Acquire))
    }

    /// Get the target S-state (during transition)
    pub fn target_state(&self) -> SState {
        SState::from(self.target_state.load(Ordering::Acquire))
    }

    /// Check if a transition is in progress
    pub fn is_transitioning(&self) -> bool {
        self.transitioning.load(Ordering::Acquire)
    }

    /// Get the current transition phase
    pub fn transition_phase(&self) -> TransitionPhase {
        self.transition_phase
    }

    /// Check if a state is supported
    pub fn is_state_supported(&self, state: SState) -> bool {
        let idx = state as usize;
        idx < 6 && self.supported_states[idx]
    }

    /// Get wake configuration
    pub fn wake_config(&self) -> &WakeSourceConfig {
        &self.wake_config
    }

    /// Get mutable wake configuration
    pub fn wake_config_mut(&mut self) -> &mut WakeSourceConfig {
        &mut self.wake_config
    }

    /// Request transition to a new S-state
    pub fn request_transition(&mut self, target: SState) -> SStateResult<()> {
        let current = self.current_state();

        // Check if transition is allowed
        if !self.is_transition_valid(current, target) {
            return Err(SStateError::InvalidTransition {
                from: current,
                to: target,
            });
        }

        // Check if state is supported
        if !self.is_state_supported(target) {
            return Err(SStateError::StateNotSupported(target));
        }

        // Check if already transitioning
        if self.transitioning.swap(true, Ordering::AcqRel) {
            return Err(SStateError::TransitionInProgress);
        }

        // Record time in current state
        if let Some(entry_time) = self.state_entry_time {
            let duration = entry_time.elapsed();
            self.stats
                .record_s_state(current, duration.as_micros() as u64);
        }

        self.target_state.store(target as u8, Ordering::Release);
        self.transition_phase = TransitionPhase::PreparingDevices;
        self.devices_ready = 0;

        // Queue the sleep request event
        self.events.push_back(PowerEvent::SleepRequest(target));

        Ok(())
    }

    /// Check if a transition is valid
    fn is_transition_valid(&self, from: SState, to: SState) -> bool {
        match (from, to) {
            // Can always go to S0 (wake)
            (_, SState::S0) => true,
            // Can go from S0 to any sleep state
            (SState::S0, _) => true,
            // Cannot transition between sleep states
            _ => false,
        }
    }

    /// Notify that a device is ready for transition
    pub fn device_ready(&mut self) {
        self.devices_ready += 1;
    }

    /// Set the total number of devices
    pub fn set_device_count(&mut self, count: usize) {
        self.device_count = count;
    }

    /// Advance the transition state machine
    pub fn advance_transition(&mut self) -> SStateResult<bool> {
        if !self.is_transitioning() {
            return Ok(false);
        }

        let target = self.target_state();

        match self.transition_phase {
            TransitionPhase::PreparingDevices => {
                // Wait for all devices to be ready
                if self.devices_ready >= self.device_count || self.device_count == 0 {
                    self.transition_phase = TransitionPhase::SavingDeviceState;
                    self.devices_ready = 0;
                }
            }
            TransitionPhase::SavingDeviceState => {
                self.transition_phase = TransitionPhase::SavingCpuState;
            }
            TransitionPhase::SavingCpuState => {
                if target == SState::S4 {
                    self.transition_phase = TransitionPhase::SavingMemory;
                } else {
                    self.transition_phase = TransitionPhase::EnteringState;
                }
            }
            TransitionPhase::SavingMemory => {
                self.transition_phase = TransitionPhase::EnteringState;
            }
            TransitionPhase::EnteringState => {
                // Actually enter the sleep state
                self.current_state.store(target as u8, Ordering::Release);
                self.state_entry_time = Some(Instant::now());
                self.transition_phase = TransitionPhase::Sleeping;
                self.stats.s_state_transitions[target as usize] += 1;
            }
            TransitionPhase::Sleeping => {
                // Stay here until wake event
            }
            TransitionPhase::RestoringMemory => {
                self.transition_phase = TransitionPhase::RestoringCpuState;
            }
            TransitionPhase::RestoringCpuState => {
                self.transition_phase = TransitionPhase::RestoringDeviceState;
            }
            TransitionPhase::RestoringDeviceState => {
                self.transition_phase = TransitionPhase::Finalizing;
            }
            TransitionPhase::Finalizing => {
                self.complete_transition();
                return Ok(true);
            }
            TransitionPhase::None => {}
        }

        Ok(false)
    }

    /// Process a wake event
    pub fn wake(&mut self, source: WakeSource) -> SStateResult<()> {
        let current = self.current_state();

        // Check if we're in a sleep state
        if !current.is_sleeping() {
            return Ok(()); // Already awake
        }

        // Check if wake source is enabled
        if !self.wake_config.is_enabled(&source) {
            return Err(SStateError::WakeSourceDisabled(source));
        }

        // Record time in sleep state
        if let Some(entry_time) = self.state_entry_time {
            let duration = entry_time.elapsed();
            self.stats
                .record_s_state(current, duration.as_micros() as u64);
        }

        // Create wake event
        let wake_event = WakeEvent::new(source, current);

        // Record in history
        if self.wake_history.len() >= self.max_wake_history {
            self.wake_history.pop_front();
        }
        self.wake_history.push_back(wake_event.clone());

        // Update stats
        self.stats.record_wake(source);

        // Queue wake event
        self.events.push_back(PowerEvent::Wake(wake_event));

        // Start wake transition
        self.target_state.store(SState::S0 as u8, Ordering::Release);
        self.transitioning.store(true, Ordering::Release);

        // Set appropriate wake phase based on current state
        if current == SState::S4 {
            self.transition_phase = TransitionPhase::RestoringMemory;
        } else {
            self.transition_phase = TransitionPhase::RestoringCpuState;
        }

        Ok(())
    }

    /// Complete the current transition
    fn complete_transition(&mut self) {
        let target = self.target_state();
        self.current_state.store(target as u8, Ordering::Release);
        self.state_entry_time = Some(Instant::now());
        self.transition_phase = TransitionPhase::None;
        self.transitioning.store(false, Ordering::Release);
    }

    /// Get recent wake history
    pub fn wake_history(&self) -> &VecDeque<WakeEvent> {
        &self.wake_history
    }

    /// Get the last wake event
    pub fn last_wake_event(&self) -> Option<&WakeEvent> {
        self.wake_history.back()
    }

    /// Get statistics
    pub fn stats(&self) -> &PowerStats {
        &self.stats
    }

    /// Poll for pending events
    pub fn poll_event(&mut self) -> Option<PowerEvent> {
        self.events.pop_front()
    }

    /// Get time in current state
    pub fn time_in_state(&self) -> Duration {
        self.state_entry_time
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    /// Force immediate transition to S5 (shutdown)
    pub fn shutdown(&mut self) {
        // Record time in current state
        let current = self.current_state();
        if let Some(entry_time) = self.state_entry_time {
            let duration = entry_time.elapsed();
            self.stats
                .record_s_state(current, duration.as_micros() as u64);
        }

        self.current_state
            .store(SState::S5 as u8, Ordering::Release);
        self.transition_phase = TransitionPhase::None;
        self.transitioning.store(false, Ordering::Release);
        self.stats.s_state_transitions[5] += 1;
    }

    /// Request suspend to RAM (S3)
    pub fn suspend(&mut self) -> SStateResult<()> {
        self.request_transition(SState::S3)
    }

    /// Request hibernate (S4)
    pub fn hibernate(&mut self) -> SStateResult<()> {
        self.request_transition(SState::S4)
    }

    /// Request soft off (S5)
    pub fn soft_off(&mut self) -> SStateResult<()> {
        self.request_transition(SState::S5)
    }
}

impl Default for SStateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s_state_error_display() {
        let err = SStateError::InvalidTransition {
            from: SState::S3,
            to: SState::S4,
        };
        assert!(format!("{}", err).contains("Invalid transition"));
    }

    #[test]
    fn test_transition_phase_display() {
        assert_eq!(format!("{}", TransitionPhase::Sleeping), "Sleeping");
    }

    #[test]
    fn test_wake_source_config_default() {
        let config = WakeSourceConfig::default();
        assert!(!config.power_button);
        assert!(config.pci_pme.is_empty());
    }

    #[test]
    fn test_wake_source_config_all_enabled() {
        let config = WakeSourceConfig::all_enabled();
        assert!(config.power_button);
        assert!(config.keyboard);
        assert!(config.is_enabled(&WakeSource::PowerButton));
    }

    #[test]
    fn test_wake_source_config_minimal() {
        let config = WakeSourceConfig::minimal();
        assert!(config.power_button);
        assert!(!config.keyboard);
    }

    #[test]
    fn test_wake_source_config_pci() {
        let mut config = WakeSourceConfig::default();
        config.enable_pci_wake(0x0100);
        assert!(config.is_enabled(&WakeSource::PciPme(0x0100)));
        assert!(!config.is_enabled(&WakeSource::PciPme(0x0200)));

        config.disable_pci_wake(0x0100);
        assert!(!config.is_enabled(&WakeSource::PciPme(0x0100)));
    }

    #[test]
    fn test_s_state_manager_creation() {
        let manager = SStateManager::new();
        assert_eq!(manager.current_state(), SState::S0);
        assert!(!manager.is_transitioning());
        assert_eq!(manager.transition_phase(), TransitionPhase::None);
    }

    #[test]
    fn test_s_state_manager_supported_states() {
        let manager = SStateManager::new();
        assert!(manager.is_state_supported(SState::S0));
        assert!(manager.is_state_supported(SState::S3));
        assert!(!manager.is_state_supported(SState::S2));
    }

    #[test]
    fn test_s_state_manager_request_transition() {
        let mut manager = SStateManager::new();
        assert!(manager.request_transition(SState::S3).is_ok());
        assert!(manager.is_transitioning());
        assert_eq!(manager.target_state(), SState::S3);
        assert_eq!(
            manager.transition_phase(),
            TransitionPhase::PreparingDevices
        );
    }

    #[test]
    fn test_s_state_manager_invalid_transition() {
        let mut manager = SStateManager::new();
        // First go to S3
        manager.request_transition(SState::S3).unwrap();

        // Try to request another transition while one is in progress
        let result = manager.request_transition(SState::S4);
        assert!(matches!(result, Err(SStateError::TransitionInProgress)));
    }

    #[test]
    fn test_s_state_manager_unsupported_state() {
        let mut manager = SStateManager::new();
        let result = manager.request_transition(SState::S2);
        assert!(matches!(result, Err(SStateError::StateNotSupported(_))));
    }

    #[test]
    fn test_s_state_manager_advance_transition() {
        let mut manager = SStateManager::new();
        manager.request_transition(SState::S3).unwrap();

        // Advance through phases
        let mut iterations = 0;
        while manager.transition_phase() != TransitionPhase::Sleeping {
            manager.advance_transition().unwrap();
            iterations += 1;
            assert!(iterations < 10, "Too many iterations");
        }

        assert_eq!(manager.current_state(), SState::S3);
    }

    #[test]
    fn test_s_state_manager_wake() {
        let mut manager = SStateManager::new();

        // Go to sleep
        manager.request_transition(SState::S3).unwrap();
        while manager.transition_phase() != TransitionPhase::Sleeping {
            manager.advance_transition().unwrap();
        }

        assert_eq!(manager.current_state(), SState::S3);

        // Wake up
        manager.wake(WakeSource::PowerButton).unwrap();
        assert!(manager.is_transitioning());
        assert_eq!(manager.target_state(), SState::S0);

        // Complete wake
        while manager.is_transitioning() {
            manager.advance_transition().unwrap();
        }

        assert_eq!(manager.current_state(), SState::S0);
    }

    #[test]
    fn test_s_state_manager_wake_disabled_source() {
        let mut manager = SStateManager::new().with_wake_config(WakeSourceConfig::minimal());

        // Go to sleep
        manager.request_transition(SState::S3).unwrap();
        while manager.transition_phase() != TransitionPhase::Sleeping {
            manager.advance_transition().unwrap();
        }

        // Try to wake with disabled source
        let result = manager.wake(WakeSource::Keyboard);
        assert!(matches!(result, Err(SStateError::WakeSourceDisabled(_))));
    }

    #[test]
    fn test_s_state_manager_wake_history() {
        let mut manager = SStateManager::new();

        // Go to sleep and wake multiple times
        for _ in 0..3 {
            manager.request_transition(SState::S3).unwrap();
            while manager.transition_phase() != TransitionPhase::Sleeping {
                manager.advance_transition().unwrap();
            }
            manager.wake(WakeSource::PowerButton).unwrap();
            while manager.is_transitioning() {
                manager.advance_transition().unwrap();
            }
        }

        assert_eq!(manager.wake_history().len(), 3);
        assert_eq!(
            manager.last_wake_event().unwrap().source,
            WakeSource::PowerButton
        );
    }

    #[test]
    fn test_s_state_manager_shutdown() {
        let mut manager = SStateManager::new();
        manager.shutdown();
        assert_eq!(manager.current_state(), SState::S5);
        assert!(!manager.is_transitioning());
    }

    #[test]
    fn test_s_state_manager_suspend_hibernate() {
        let mut manager = SStateManager::new();

        assert!(manager.suspend().is_ok());
        assert_eq!(manager.target_state(), SState::S3);

        // Reset
        manager.shutdown();
        manager
            .current_state
            .store(SState::S0 as u8, Ordering::Release);
        manager.transitioning.store(false, Ordering::Release);

        assert!(manager.hibernate().is_ok());
        assert_eq!(manager.target_state(), SState::S4);
    }

    #[test]
    fn test_s_state_manager_poll_event() {
        let mut manager = SStateManager::new();
        manager.request_transition(SState::S3).unwrap();

        let event = manager.poll_event();
        assert!(matches!(event, Some(PowerEvent::SleepRequest(SState::S3))));
    }

    #[test]
    fn test_s_state_manager_stats() {
        let mut manager = SStateManager::new();

        // Go to sleep
        manager.request_transition(SState::S3).unwrap();
        while manager.transition_phase() != TransitionPhase::Sleeping {
            manager.advance_transition().unwrap();
        }

        // Wake
        manager.wake(WakeSource::RtcAlarm).unwrap();

        assert_eq!(manager.stats().wake_events, 1);
        assert_eq!(manager.stats().last_wake_source, Some(WakeSource::RtcAlarm));
    }

    #[test]
    fn test_s_state_manager_device_ready() {
        let mut manager = SStateManager::new();
        manager.set_device_count(3);
        manager.request_transition(SState::S3).unwrap();

        // Should stay in PreparingDevices until all ready
        manager.advance_transition().unwrap();
        assert_eq!(
            manager.transition_phase(),
            TransitionPhase::PreparingDevices
        );

        manager.device_ready();
        manager.device_ready();
        manager.advance_transition().unwrap();
        assert_eq!(
            manager.transition_phase(),
            TransitionPhase::PreparingDevices
        );

        manager.device_ready();
        manager.advance_transition().unwrap();
        assert_eq!(
            manager.transition_phase(),
            TransitionPhase::SavingDeviceState
        );
    }
}
