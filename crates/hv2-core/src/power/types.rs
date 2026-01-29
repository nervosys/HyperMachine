//! Power management core types
//!
//! This module provides fundamental types for power management including
//! ACPI S-states, CPU C-states, P-states, and wake events.

use std::fmt;
use std::time::{Duration, Instant};

/// ACPI System Power State (S-states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SState {
    /// S0 - Working state (full power)
    S0 = 0,
    /// S1 - Sleeping with processor context maintained
    S1 = 1,
    /// S2 - Sleeping with processor context lost
    S2 = 2,
    /// S3 - Suspend to RAM (STR)
    S3 = 3,
    /// S4 - Suspend to Disk (Hibernate)
    S4 = 4,
    /// S5 - Soft Off (mechanical off)
    S5 = 5,
}

impl SState {
    /// Check if this is a sleeping state
    pub fn is_sleeping(&self) -> bool {
        matches!(self, SState::S1 | SState::S2 | SState::S3 | SState::S4)
    }

    /// Check if this state preserves memory
    pub fn preserves_memory(&self) -> bool {
        matches!(self, SState::S0 | SState::S1 | SState::S2 | SState::S3)
    }

    /// Check if this state preserves CPU context
    pub fn preserves_cpu_context(&self) -> bool {
        matches!(self, SState::S0 | SState::S1)
    }

    /// Get the typical resume latency for this state
    pub fn resume_latency(&self) -> Duration {
        match self {
            SState::S0 => Duration::from_micros(0),
            SState::S1 => Duration::from_micros(100),
            SState::S2 => Duration::from_millis(1),
            SState::S3 => Duration::from_millis(100),
            SState::S4 => Duration::from_secs(10),
            SState::S5 => Duration::from_secs(30),
        }
    }

    /// Get the power consumption level (0-100, relative to S0)
    pub fn power_level(&self) -> u8 {
        match self {
            SState::S0 => 100,
            SState::S1 => 50,
            SState::S2 => 30,
            SState::S3 => 10,
            SState::S4 => 1,
            SState::S5 => 0,
        }
    }

    /// Get all valid S-states
    pub fn all() -> &'static [SState] {
        &[
            SState::S0,
            SState::S1,
            SState::S2,
            SState::S3,
            SState::S4,
            SState::S5,
        ]
    }
}

impl fmt::Display for SState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SState::S0 => write!(f, "S0 (Working)"),
            SState::S1 => write!(f, "S1 (Standby)"),
            SState::S2 => write!(f, "S2 (Sleep)"),
            SState::S3 => write!(f, "S3 (Suspend)"),
            SState::S4 => write!(f, "S4 (Hibernate)"),
            SState::S5 => write!(f, "S5 (Off)"),
        }
    }
}

impl From<u8> for SState {
    fn from(value: u8) -> Self {
        match value {
            0 => SState::S0,
            1 => SState::S1,
            2 => SState::S2,
            3 => SState::S3,
            4 => SState::S4,
            _ => SState::S5,
        }
    }
}

/// CPU C-State (idle power states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CState {
    /// C0 - Active/Running state
    C0 = 0,
    /// C1 - Halt (clock stopped)
    C1 = 1,
    /// C1E - Enhanced Halt (reduced voltage)
    C1E = 2,
    /// C2 - Stop Clock
    C2 = 3,
    /// C3 - Sleep (L1/L2 cache flushed)
    C3 = 4,
    /// C6 - Deep Power Down (context saved)
    C6 = 5,
    /// C7 - Deeper Power Down
    C7 = 6,
    /// C8 - Deepest Power Down
    C8 = 7,
    /// C10 - Package C10 (lowest power)
    C10 = 8,
}

impl CState {
    /// Check if CPU is executing instructions
    pub fn is_active(&self) -> bool {
        matches!(self, CState::C0)
    }

    /// Get the exit latency in microseconds
    pub fn exit_latency_us(&self) -> u32 {
        match self {
            CState::C0 => 0,
            CState::C1 => 1,
            CState::C1E => 2,
            CState::C2 => 10,
            CState::C3 => 100,
            CState::C6 => 200,
            CState::C7 => 300,
            CState::C8 => 400,
            CState::C10 => 1000,
        }
    }

    /// Get the target residency in microseconds (minimum time to stay in state)
    pub fn target_residency_us(&self) -> u32 {
        match self {
            CState::C0 => 0,
            CState::C1 => 1,
            CState::C1E => 10,
            CState::C2 => 100,
            CState::C3 => 500,
            CState::C6 => 1000,
            CState::C7 => 2000,
            CState::C8 => 5000,
            CState::C10 => 10000,
        }
    }

    /// Get the power consumption (relative, 0-100)
    pub fn power_level(&self) -> u8 {
        match self {
            CState::C0 => 100,
            CState::C1 => 70,
            CState::C1E => 60,
            CState::C2 => 50,
            CState::C3 => 30,
            CState::C6 => 15,
            CState::C7 => 10,
            CState::C8 => 5,
            CState::C10 => 1,
        }
    }

    /// Get the MWAIT hint for this C-state
    pub fn mwait_hint(&self) -> u32 {
        match self {
            CState::C0 => 0x00,
            CState::C1 => 0x00,
            CState::C1E => 0x01,
            CState::C2 => 0x10,
            CState::C3 => 0x20,
            CState::C6 => 0x40,
            CState::C7 => 0x50,
            CState::C8 => 0x60,
            CState::C10 => 0x70,
        }
    }

    /// Get all C-states in order of increasing depth
    pub fn all() -> &'static [CState] {
        &[
            CState::C0,
            CState::C1,
            CState::C1E,
            CState::C2,
            CState::C3,
            CState::C6,
            CState::C7,
            CState::C8,
            CState::C10,
        ]
    }
}

impl fmt::Display for CState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CState::C0 => write!(f, "C0 (Active)"),
            CState::C1 => write!(f, "C1 (Halt)"),
            CState::C1E => write!(f, "C1E (Enhanced)"),
            CState::C2 => write!(f, "C2 (Stop Clock)"),
            CState::C3 => write!(f, "C3 (Sleep)"),
            CState::C6 => write!(f, "C6 (Deep Sleep)"),
            CState::C7 => write!(f, "C7 (Deeper)"),
            CState::C8 => write!(f, "C8 (Deepest)"),
            CState::C10 => write!(f, "C10 (Package)"),
        }
    }
}

/// CPU P-State (performance/frequency states)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PState {
    /// P-state index (0 = highest performance)
    pub index: u8,
    /// Core frequency in MHz
    pub frequency_mhz: u32,
    /// Core voltage in millivolts
    pub voltage_mv: u32,
    /// Power consumption in milliwatts
    pub power_mw: u32,
    /// Latency to transition to this state in microseconds
    pub latency_us: u32,
}

impl PState {
    /// Create a new P-state
    pub fn new(index: u8, frequency_mhz: u32, voltage_mv: u32, power_mw: u32) -> Self {
        Self {
            index,
            frequency_mhz,
            voltage_mv,
            power_mw,
            latency_us: 10, // Default 10us transition
        }
    }

    /// Create with custom latency
    pub fn with_latency(mut self, latency_us: u32) -> Self {
        self.latency_us = latency_us;
        self
    }

    /// Calculate the frequency ratio relative to a base frequency
    pub fn frequency_ratio(&self, base_mhz: u32) -> f64 {
        if base_mhz == 0 {
            0.0
        } else {
            self.frequency_mhz as f64 / base_mhz as f64
        }
    }

    /// Estimate power at a given utilization (0.0-1.0)
    pub fn power_at_utilization(&self, utilization: f64) -> u32 {
        let utilization = utilization.clamp(0.0, 1.0);
        // Power roughly scales with V^2 * F, but simplified here
        (self.power_mw as f64 * (0.3 + 0.7 * utilization)) as u32
    }
}

impl fmt::Display for PState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "P{}: {}MHz @ {}mV ({}mW)",
            self.index, self.frequency_mhz, self.voltage_mv, self.power_mw
        )
    }
}

/// Device Power State (D-states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DState {
    /// D0 - Fully On
    D0 = 0,
    /// D1 - Light Sleep
    D1 = 1,
    /// D2 - Deeper Sleep
    D2 = 2,
    /// D3hot - Off with power
    D3Hot = 3,
    /// D3cold - Off without power
    D3Cold = 4,
}

impl DState {
    /// Check if device is operational
    pub fn is_operational(&self) -> bool {
        matches!(self, DState::D0)
    }

    /// Check if device context is preserved
    pub fn preserves_context(&self) -> bool {
        matches!(self, DState::D0 | DState::D1 | DState::D2)
    }

    /// Check if device has power
    pub fn has_power(&self) -> bool {
        !matches!(self, DState::D3Cold)
    }

    /// Get typical resume latency
    pub fn resume_latency(&self) -> Duration {
        match self {
            DState::D0 => Duration::from_micros(0),
            DState::D1 => Duration::from_micros(100),
            DState::D2 => Duration::from_millis(1),
            DState::D3Hot => Duration::from_millis(10),
            DState::D3Cold => Duration::from_millis(100),
        }
    }

    /// Get power consumption (relative, 0-100)
    pub fn power_level(&self) -> u8 {
        match self {
            DState::D0 => 100,
            DState::D1 => 50,
            DState::D2 => 20,
            DState::D3Hot => 5,
            DState::D3Cold => 0,
        }
    }
}

impl fmt::Display for DState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DState::D0 => write!(f, "D0 (On)"),
            DState::D1 => write!(f, "D1 (Light Sleep)"),
            DState::D2 => write!(f, "D2 (Sleep)"),
            DState::D3Hot => write!(f, "D3hot"),
            DState::D3Cold => write!(f, "D3cold"),
        }
    }
}

/// Wake event source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WakeSource {
    /// Power button
    PowerButton,
    /// Sleep button
    SleepButton,
    /// Lid open
    LidOpen,
    /// RTC alarm
    RtcAlarm,
    /// PCI device wake (PME)
    PciPme(u16), // BDF
    /// USB device
    Usb,
    /// Network wake (Wake-on-LAN)
    Network,
    /// Keyboard
    Keyboard,
    /// Mouse
    Mouse,
    /// Timer
    Timer,
    /// External interrupt
    ExternalInterrupt(u8),
    /// Software request
    Software,
}

impl fmt::Display for WakeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WakeSource::PowerButton => write!(f, "Power Button"),
            WakeSource::SleepButton => write!(f, "Sleep Button"),
            WakeSource::LidOpen => write!(f, "Lid Open"),
            WakeSource::RtcAlarm => write!(f, "RTC Alarm"),
            WakeSource::PciPme(bdf) => write!(f, "PCI PME ({:04x})", bdf),
            WakeSource::Usb => write!(f, "USB"),
            WakeSource::Network => write!(f, "Network"),
            WakeSource::Keyboard => write!(f, "Keyboard"),
            WakeSource::Mouse => write!(f, "Mouse"),
            WakeSource::Timer => write!(f, "Timer"),
            WakeSource::ExternalInterrupt(irq) => write!(f, "IRQ {}", irq),
            WakeSource::Software => write!(f, "Software"),
        }
    }
}

/// Wake event record
#[derive(Debug, Clone)]
pub struct WakeEvent {
    /// Source of the wake event
    pub source: WakeSource,
    /// Timestamp when the event occurred
    pub timestamp: Instant,
    /// State we were waking from
    pub from_state: SState,
    /// Additional event data
    pub data: u64,
}

impl WakeEvent {
    /// Create a new wake event
    pub fn new(source: WakeSource, from_state: SState) -> Self {
        Self {
            source,
            timestamp: Instant::now(),
            from_state,
            data: 0,
        }
    }

    /// Create with additional data
    pub fn with_data(mut self, data: u64) -> Self {
        self.data = data;
        self
    }
}

/// Power management event
#[derive(Debug, Clone)]
pub enum PowerEvent {
    /// Request to enter a sleep state
    SleepRequest(SState),
    /// Wake from sleep
    Wake(WakeEvent),
    /// CPU entering C-state
    CStateEnter(u32, CState), // CPU ID, state
    /// CPU exiting C-state
    CStateExit(u32, CState),
    /// P-state change
    PStateChange(u32, PState), // CPU ID, new state
    /// Device power state change
    DevicePowerChange(u32, DState), // Device ID, new state
    /// Thermal event
    ThermalEvent(ThermalEventType),
    /// Battery event
    BatteryEvent(BatteryEventType),
}

/// Thermal event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalEventType {
    /// Temperature threshold crossed
    ThresholdCrossed { zone: u8, temperature: i32 },
    /// Thermal trip point reached
    TripPoint {
        zone: u8,
        trip_type: ThermalTripType,
    },
    /// Cooling device state change
    CoolingChange { device: u8, level: u8 },
}

/// Thermal trip point types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalTripType {
    /// Active cooling (fan)
    Active,
    /// Passive cooling (throttling)
    Passive,
    /// Hot (warning)
    Hot,
    /// Critical (shutdown)
    Critical,
}

/// Battery event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryEventType {
    /// Battery status change
    StatusChange { charging: bool, level: u8 },
    /// Low battery warning
    LowBattery { level: u8 },
    /// Critical battery (shutdown imminent)
    CriticalBattery,
    /// AC adapter connected/disconnected
    AcAdapter { connected: bool },
}

/// Power statistics
#[derive(Debug, Clone, Default)]
pub struct PowerStats {
    /// Time spent in each S-state (microseconds)
    pub s_state_time: [u64; 6],
    /// Number of transitions to each S-state
    pub s_state_transitions: [u64; 6],
    /// Current S-state
    pub current_s_state: u8,
    /// Time spent in each C-state per CPU (microseconds)
    pub c_state_time: Vec<[u64; 9]>,
    /// Number of C-state transitions per CPU
    pub c_state_transitions: Vec<[u64; 9]>,
    /// Current C-state per CPU
    pub current_c_state: Vec<u8>,
    /// Time spent at each P-state per CPU (microseconds)
    pub p_state_time: Vec<Vec<u64>>,
    /// Current P-state index per CPU
    pub current_p_state: Vec<u8>,
    /// Total wake events
    pub wake_events: u64,
    /// Last wake source
    pub last_wake_source: Option<WakeSource>,
}

impl PowerStats {
    /// Create new power statistics
    pub fn new(cpu_count: usize, p_state_count: usize) -> Self {
        Self {
            s_state_time: [0; 6],
            s_state_transitions: [0; 6],
            current_s_state: 0,
            c_state_time: vec![[0; 9]; cpu_count],
            c_state_transitions: vec![[0; 9]; cpu_count],
            current_c_state: vec![0; cpu_count],
            p_state_time: vec![vec![0; p_state_count]; cpu_count],
            current_p_state: vec![0; cpu_count],
            wake_events: 0,
            last_wake_source: None,
        }
    }

    /// Record S-state transition
    pub fn record_s_state(&mut self, state: SState, duration_us: u64) {
        let idx = state as usize;
        if idx < 6 {
            self.s_state_time[idx] += duration_us;
            self.s_state_transitions[idx] += 1;
            self.current_s_state = idx as u8;
        }
    }

    /// Record C-state transition
    pub fn record_c_state(&mut self, cpu: usize, state: CState, duration_us: u64) {
        if cpu < self.c_state_time.len() {
            let idx = state as usize;
            if idx < 9 {
                self.c_state_time[cpu][idx] += duration_us;
                self.c_state_transitions[cpu][idx] += 1;
                self.current_c_state[cpu] = idx as u8;
            }
        }
    }

    /// Record P-state usage
    pub fn record_p_state(&mut self, cpu: usize, p_state: u8, duration_us: u64) {
        if cpu < self.p_state_time.len() {
            let idx = p_state as usize;
            if idx < self.p_state_time[cpu].len() {
                self.p_state_time[cpu][idx] += duration_us;
                self.current_p_state[cpu] = p_state;
            }
        }
    }

    /// Record wake event
    pub fn record_wake(&mut self, source: WakeSource) {
        self.wake_events += 1;
        self.last_wake_source = Some(source);
    }

    /// Get total time in sleeping states
    pub fn total_sleep_time(&self) -> u64 {
        self.s_state_time[1..5].iter().sum()
    }

    /// Get average C-state depth for a CPU
    pub fn average_c_state(&self, cpu: usize) -> f64 {
        if cpu >= self.c_state_time.len() {
            return 0.0;
        }

        let total_time: u64 = self.c_state_time[cpu].iter().sum();
        if total_time == 0 {
            return 0.0;
        }

        let weighted_sum: u64 = self.c_state_time[cpu]
            .iter()
            .enumerate()
            .map(|(i, &t)| i as u64 * t)
            .sum();

        weighted_sum as f64 / total_time as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s_state_properties() {
        assert!(!SState::S0.is_sleeping());
        assert!(SState::S3.is_sleeping());
        assert!(SState::S0.preserves_memory());
        assert!(SState::S3.preserves_memory());
        assert!(!SState::S5.preserves_memory());
        assert!(SState::S1.preserves_cpu_context());
        assert!(!SState::S3.preserves_cpu_context());
    }

    #[test]
    fn test_s_state_power_levels() {
        assert_eq!(SState::S0.power_level(), 100);
        assert_eq!(SState::S5.power_level(), 0);
        assert!(SState::S3.power_level() < SState::S1.power_level());
    }

    #[test]
    fn test_s_state_display() {
        assert_eq!(format!("{}", SState::S0), "S0 (Working)");
        assert_eq!(format!("{}", SState::S3), "S3 (Suspend)");
    }

    #[test]
    fn test_s_state_from_u8() {
        assert_eq!(SState::from(0), SState::S0);
        assert_eq!(SState::from(3), SState::S3);
        assert_eq!(SState::from(10), SState::S5); // Invalid maps to S5
    }

    #[test]
    fn test_c_state_properties() {
        assert!(CState::C0.is_active());
        assert!(!CState::C3.is_active());
        assert!(CState::C6.exit_latency_us() > CState::C1.exit_latency_us());
        assert!(CState::C6.target_residency_us() > CState::C1.target_residency_us());
    }

    #[test]
    fn test_c_state_mwait_hints() {
        assert_eq!(CState::C1.mwait_hint(), 0x00);
        assert_eq!(CState::C3.mwait_hint(), 0x20);
        assert_eq!(CState::C6.mwait_hint(), 0x40);
    }

    #[test]
    fn test_c_state_display() {
        assert_eq!(format!("{}", CState::C0), "C0 (Active)");
        assert_eq!(format!("{}", CState::C6), "C6 (Deep Sleep)");
    }

    #[test]
    fn test_p_state_creation() {
        let p_state = PState::new(0, 3500, 1100, 95000);
        assert_eq!(p_state.index, 0);
        assert_eq!(p_state.frequency_mhz, 3500);
        assert_eq!(p_state.voltage_mv, 1100);
        assert_eq!(p_state.power_mw, 95000);
    }

    #[test]
    fn test_p_state_frequency_ratio() {
        let p_state = PState::new(0, 3500, 1100, 95000);
        assert!((p_state.frequency_ratio(2000) - 1.75).abs() < 0.01);
    }

    #[test]
    fn test_p_state_power_estimation() {
        let p_state = PState::new(0, 3500, 1100, 100000);
        let power_idle = p_state.power_at_utilization(0.0);
        let power_full = p_state.power_at_utilization(1.0);
        assert!(power_idle < power_full);
        assert_eq!(power_full, 100000);
    }

    #[test]
    fn test_p_state_display() {
        let p_state = PState::new(2, 2500, 900, 50000);
        assert_eq!(format!("{}", p_state), "P2: 2500MHz @ 900mV (50000mW)");
    }

    #[test]
    fn test_d_state_properties() {
        assert!(DState::D0.is_operational());
        assert!(!DState::D3Hot.is_operational());
        assert!(DState::D1.preserves_context());
        assert!(!DState::D3Hot.preserves_context());
        assert!(DState::D3Hot.has_power());
        assert!(!DState::D3Cold.has_power());
    }

    #[test]
    fn test_d_state_display() {
        assert_eq!(format!("{}", DState::D0), "D0 (On)");
        assert_eq!(format!("{}", DState::D3Cold), "D3cold");
    }

    #[test]
    fn test_wake_source_display() {
        assert_eq!(format!("{}", WakeSource::PowerButton), "Power Button");
        assert_eq!(format!("{}", WakeSource::PciPme(0x0100)), "PCI PME (0100)");
    }

    #[test]
    fn test_wake_event_creation() {
        let event = WakeEvent::new(WakeSource::RtcAlarm, SState::S3);
        assert_eq!(event.source, WakeSource::RtcAlarm);
        assert_eq!(event.from_state, SState::S3);
    }

    #[test]
    fn test_wake_event_with_data() {
        let event = WakeEvent::new(WakeSource::Timer, SState::S3).with_data(12345);
        assert_eq!(event.data, 12345);
    }

    #[test]
    fn test_thermal_event() {
        let event = ThermalEventType::ThresholdCrossed {
            zone: 0,
            temperature: 85,
        };
        if let ThermalEventType::ThresholdCrossed { zone, temperature } = event {
            assert_eq!(zone, 0);
            assert_eq!(temperature, 85);
        }
    }

    #[test]
    fn test_battery_event() {
        let event = BatteryEventType::StatusChange {
            charging: true,
            level: 75,
        };
        if let BatteryEventType::StatusChange { charging, level } = event {
            assert!(charging);
            assert_eq!(level, 75);
        }
    }

    #[test]
    fn test_power_stats_creation() {
        let stats = PowerStats::new(4, 8);
        assert_eq!(stats.c_state_time.len(), 4);
        assert_eq!(stats.p_state_time.len(), 4);
        assert_eq!(stats.p_state_time[0].len(), 8);
    }

    #[test]
    fn test_power_stats_s_state() {
        let mut stats = PowerStats::new(2, 4);
        stats.record_s_state(SState::S3, 1000000);
        assert_eq!(stats.s_state_time[3], 1000000);
        assert_eq!(stats.s_state_transitions[3], 1);
        assert_eq!(stats.current_s_state, 3);
    }

    #[test]
    fn test_power_stats_c_state() {
        let mut stats = PowerStats::new(2, 4);
        stats.record_c_state(0, CState::C6, 5000);
        stats.record_c_state(0, CState::C1, 1000);
        assert_eq!(stats.c_state_time[0][5], 5000); // C6 is index 5
        assert_eq!(stats.c_state_transitions[0][5], 1);
    }

    #[test]
    fn test_power_stats_wake() {
        let mut stats = PowerStats::new(2, 4);
        stats.record_wake(WakeSource::PowerButton);
        assert_eq!(stats.wake_events, 1);
        assert_eq!(stats.last_wake_source, Some(WakeSource::PowerButton));
    }

    #[test]
    fn test_power_stats_total_sleep() {
        let mut stats = PowerStats::new(2, 4);
        stats.record_s_state(SState::S1, 1000);
        stats.record_s_state(SState::S3, 5000);
        assert_eq!(stats.total_sleep_time(), 6000);
    }

    #[test]
    fn test_power_stats_average_c_state() {
        let mut stats = PowerStats::new(1, 4);
        stats.c_state_time[0][0] = 5000; // C0
        stats.c_state_time[0][4] = 5000; // C3
        let avg = stats.average_c_state(0);
        assert!((avg - 2.0).abs() < 0.01); // Average of 0 and 4
    }
}
