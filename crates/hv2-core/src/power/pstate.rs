//! CPU P-state (Performance State) management
//!
//! This module provides management of CPU frequency scaling using ACPI P-states
//! for performance and power optimization.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::types::{PState, PowerEvent, PowerStats};

/// P-state management result
pub type PStateResult<T> = Result<T, PStateError>;

/// P-state management error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PStateError {
    /// Invalid CPU ID
    #[error("Invalid CPU ID: {0}")]
    InvalidCpu(u32),
    /// Invalid P-state index
    #[error("Invalid P-state index: {0}")]
    InvalidPState(u32),
    /// P-state not supported
    #[error("P-state not supported")]
    PStateNotSupported,
    /// Frequency out of range
    #[error("Frequency {requested} MHz out of range [{min} - {max}]")]
    FrequencyOutOfRange { requested: u32, min: u32, max: u32 },
    /// Governor error
    #[error("Governor error: {0}")]
    GovernorError(String),
    /// CPU offline
    #[error("CPU {0} is offline")]
    CpuOffline(u32),
    /// Hardware error
    #[error("Hardware error: {0}")]
    HardwareError(String),
}

/// P-state frequency scaling governor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PStateGovernor {
    /// Performance - always maximum frequency
    Performance,
    /// Powersave - always minimum frequency
    Powersave,
    /// Ondemand - scale based on utilization
    #[default]
    Ondemand,
    /// Conservative - gradual scaling
    Conservative,
    /// Schedutil - scheduler-based scaling
    Schedutil,
    /// Userspace - user-controlled
    Userspace,
}

impl std::fmt::Display for PStateGovernor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PStateGovernor::Performance => write!(f, "performance"),
            PStateGovernor::Powersave => write!(f, "powersave"),
            PStateGovernor::Ondemand => write!(f, "ondemand"),
            PStateGovernor::Conservative => write!(f, "conservative"),
            PStateGovernor::Schedutil => write!(f, "schedutil"),
            PStateGovernor::Userspace => write!(f, "userspace"),
        }
    }
}

/// Per-CPU P-state tracking
#[derive(Debug, Clone)]
pub struct CpuPState {
    /// CPU ID
    pub cpu_id: u32,
    /// Current P-state index
    pub current_pstate: u32,
    /// Available P-states
    pub pstates: Vec<PState>,
    /// Time when current state was entered
    pub state_entered: Instant,
    /// Time spent in each P-state (microseconds)
    pub state_time_us: Vec<u64>,
    /// Number of times each state was entered
    pub state_transitions: Vec<u64>,
    /// Current utilization (0-100)
    pub utilization: u8,
    /// CPU is online
    pub online: bool,
    /// Minimum allowed P-state index
    pub min_pstate: u32,
    /// Maximum allowed P-state index
    pub max_pstate: u32,
    /// Turbo boost enabled
    pub turbo_enabled: bool,
    /// Recent utilization samples
    utilization_history: VecDeque<u8>,
    /// Energy Performance Preference (0-255, higher = more power efficient)
    pub energy_perf_preference: u8,
}

impl CpuPState {
    /// Create new CPU P-state tracker
    pub fn new(cpu_id: u32, pstates: Vec<PState>) -> Self {
        let count = pstates.len();
        Self {
            cpu_id,
            current_pstate: 0,
            pstates,
            state_entered: Instant::now(),
            state_time_us: vec![0; count],
            state_transitions: vec![0; count],
            utilization: 0,
            online: true,
            min_pstate: 0,
            max_pstate: count.saturating_sub(1) as u32,
            turbo_enabled: true,
            utilization_history: VecDeque::with_capacity(8),
            energy_perf_preference: 128, // Balanced
        }
    }

    /// Get current frequency in MHz
    pub fn current_frequency(&self) -> u32 {
        self.pstates
            .get(self.current_pstate as usize)
            .map(|p| p.frequency_mhz)
            .unwrap_or(0)
    }

    /// Get minimum frequency in MHz
    pub fn min_frequency(&self) -> u32 {
        self.pstates
            .get(self.min_pstate as usize)
            .map(|p| p.frequency_mhz)
            .unwrap_or(0)
    }

    /// Get maximum frequency in MHz
    pub fn max_frequency(&self) -> u32 {
        let max_idx = if self.turbo_enabled {
            self.max_pstate
        } else {
            self.max_pstate.saturating_sub(1)
        };
        self.pstates
            .get(max_idx as usize)
            .map(|p| p.frequency_mhz)
            .unwrap_or(0)
    }

    /// Set P-state by index
    pub fn set_pstate(&mut self, index: u32) -> PStateResult<()> {
        if index >= self.pstates.len() as u32 {
            return Err(PStateError::InvalidPState(index));
        }

        if index < self.min_pstate || index > self.max_pstate {
            return Err(PStateError::InvalidPState(index));
        }

        if self.current_pstate != index {
            // Record time in previous state
            let elapsed = self.state_entered.elapsed();
            if let Some(time) = self.state_time_us.get_mut(self.current_pstate as usize) {
                *time += elapsed.as_micros() as u64;
            }

            // Enter new state
            self.current_pstate = index;
            self.state_entered = Instant::now();
            if let Some(trans) = self.state_transitions.get_mut(index as usize) {
                *trans += 1;
            }
        }

        Ok(())
    }

    /// Find P-state index by frequency
    pub fn find_pstate_by_frequency(&self, freq_mhz: u32) -> Option<u32> {
        self.pstates
            .iter()
            .position(|p| p.frequency_mhz == freq_mhz)
            .map(|i| i as u32)
    }

    /// Find closest P-state to target frequency
    pub fn find_closest_pstate(&self, freq_mhz: u32) -> u32 {
        let mut best_idx = 0;
        let mut best_diff = u32::MAX;

        for (i, pstate) in self.pstates.iter().enumerate() {
            let diff = (pstate.frequency_mhz as i64 - freq_mhz as i64).unsigned_abs() as u32;
            if diff < best_diff {
                best_diff = diff;
                best_idx = i as u32;
            }
        }

        best_idx.clamp(self.min_pstate, self.max_pstate)
    }

    /// Update utilization
    pub fn update_utilization(&mut self, utilization: u8) {
        self.utilization = utilization;

        if self.utilization_history.len() >= 8 {
            self.utilization_history.pop_front();
        }
        self.utilization_history.push_back(utilization);
    }

    /// Get average utilization
    pub fn average_utilization(&self) -> u8 {
        if self.utilization_history.is_empty() {
            return self.utilization;
        }
        let sum: u32 = self.utilization_history.iter().map(|&u| u as u32).sum();
        (sum / self.utilization_history.len() as u32) as u8
    }

    /// Get P-state residency ratio
    pub fn state_residency_ratio(&self, index: u32) -> f64 {
        let total: u64 = self.state_time_us.iter().sum();
        if total == 0 {
            return 0.0;
        }
        self.state_time_us
            .get(index as usize)
            .map(|&t| t as f64 / total as f64)
            .unwrap_or(0.0)
    }

    /// Get current power consumption estimate
    pub fn estimated_power_mw(&self) -> u32 {
        self.pstates
            .get(self.current_pstate as usize)
            .map(|p| p.power_at_utilization(self.utilization as f64 / 100.0))
            .unwrap_or(0)
    }

    /// Set frequency limits
    pub fn set_frequency_limits(&mut self, min_mhz: u32, max_mhz: u32) {
        self.min_pstate = self.find_closest_pstate(min_mhz);
        self.max_pstate = self.find_closest_pstate(max_mhz);
    }

    /// Get time in current state
    pub fn time_in_state(&self) -> Duration {
        self.state_entered.elapsed()
    }
}

/// P-state manager for all CPUs
#[derive(Debug)]
pub struct PStateManager {
    /// Per-CPU state tracking
    cpus: Vec<CpuPState>,
    /// Active governor
    governor: PStateGovernor,
    /// Event queue
    events: VecDeque<PowerEvent>,
    /// Global statistics
    stats: PowerStats,
    /// Ondemand governor: up threshold (0-100)
    up_threshold: u8,
    /// Ondemand governor: down threshold (0-100)
    down_threshold: u8,
    /// Sampling rate (microseconds)
    sampling_rate_us: u32,
    /// Last sample time
    last_sample: Instant,
}

impl PStateManager {
    /// Create a new P-state manager with default P-states
    pub fn new(cpu_count: u32) -> Self {
        // Default P-states for a typical CPU
        let default_pstates = vec![
            PState::new(0, 800, 700, 5000), // Min frequency
            PState::new(1, 1200, 800, 10000),
            PState::new(2, 1600, 900, 18000),
            PState::new(3, 2000, 1000, 28000),
            PState::new(4, 2400, 1100, 42000),
            PState::new(5, 2800, 1150, 55000),
            PState::new(6, 3200, 1200, 75000),
            PState::new(7, 3600, 1250, 95000), // Max/turbo frequency
        ];

        Self::with_pstates(cpu_count, default_pstates)
    }

    /// Create with specific P-states
    pub fn with_pstates(cpu_count: u32, pstates: Vec<PState>) -> Self {
        let cpus = (0..cpu_count)
            .map(|id| CpuPState::new(id, pstates.clone()))
            .collect();

        let pstate_count = pstates.len();

        Self {
            cpus,
            governor: PStateGovernor::Ondemand,
            events: VecDeque::new(),
            stats: PowerStats::new(cpu_count as usize, pstate_count),
            up_threshold: 80,
            down_threshold: 20,
            sampling_rate_us: 10000, // 10ms
            last_sample: Instant::now(),
        }
    }

    /// Set the governor
    pub fn set_governor(&mut self, governor: PStateGovernor) {
        self.governor = governor;
    }

    /// Get the current governor
    pub fn governor(&self) -> PStateGovernor {
        self.governor
    }

    /// Get CPU count
    pub fn cpu_count(&self) -> u32 {
        self.cpus.len() as u32
    }

    /// Get CPU state
    pub fn cpu(&self, cpu_id: u32) -> PStateResult<&CpuPState> {
        self.cpus
            .get(cpu_id as usize)
            .ok_or(PStateError::InvalidCpu(cpu_id))
    }

    /// Get mutable CPU state
    pub fn cpu_mut(&mut self, cpu_id: u32) -> PStateResult<&mut CpuPState> {
        self.cpus
            .get_mut(cpu_id as usize)
            .ok_or(PStateError::InvalidCpu(cpu_id))
    }

    /// Get current frequency for a CPU
    pub fn current_frequency(&self, cpu_id: u32) -> PStateResult<u32> {
        Ok(self.cpu(cpu_id)?.current_frequency())
    }

    /// Update CPU utilization
    pub fn update_utilization(&mut self, cpu_id: u32, utilization: u8) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        if !cpu.online {
            return Err(PStateError::CpuOffline(cpu_id));
        }
        cpu.update_utilization(utilization.min(100));
        Ok(())
    }

    /// Run governor to select optimal P-state
    pub fn run_governor(&mut self, cpu_id: u32) -> PStateResult<u32> {
        let cpu = self.cpu(cpu_id)?;
        if !cpu.online {
            return Err(PStateError::CpuOffline(cpu_id));
        }

        let current_pstate = cpu.current_pstate;
        let utilization = cpu.average_utilization();
        let min_pstate = cpu.min_pstate;
        let max_pstate = cpu.max_pstate;

        let new_pstate = match self.governor {
            PStateGovernor::Performance => max_pstate,
            PStateGovernor::Powersave => min_pstate,
            PStateGovernor::Userspace => current_pstate, // User-controlled, don't change
            PStateGovernor::Ondemand => {
                self.ondemand_select(current_pstate, utilization, min_pstate, max_pstate)
            }
            PStateGovernor::Conservative => {
                self.conservative_select(current_pstate, utilization, min_pstate, max_pstate)
            }
            PStateGovernor::Schedutil => self.schedutil_select(utilization, min_pstate, max_pstate),
        };

        if new_pstate != current_pstate {
            let cpu = self.cpu_mut(cpu_id)?;
            let _old_freq = cpu.current_frequency();
            cpu.set_pstate(new_pstate)?;
            let new_pstate_obj = cpu.pstates.get(new_pstate as usize).cloned();

            // Queue event
            if let Some(pstate) = new_pstate_obj {
                self.events
                    .push_back(PowerEvent::PStateChange(cpu_id, pstate));
            }

            // Update stats
            if cpu_id < self.stats.p_state_time.len() as u32 {
                self.stats.p_state_time[cpu_id as usize][new_pstate as usize] += 1;
            }
        }

        Ok(new_pstate)
    }

    /// Ondemand governor selection
    fn ondemand_select(&self, current: u32, utilization: u8, min: u32, max: u32) -> u32 {
        if utilization >= self.up_threshold {
            // Jump to max frequency
            max
        } else if utilization <= self.down_threshold && current > min {
            // Step down
            current - 1
        } else {
            current
        }
    }

    /// Conservative governor selection
    fn conservative_select(&self, current: u32, utilization: u8, min: u32, max: u32) -> u32 {
        if utilization >= self.up_threshold && current < max {
            // Step up
            current + 1
        } else if utilization <= self.down_threshold && current > min {
            // Step down
            current - 1
        } else {
            current
        }
    }

    /// Schedutil governor selection
    fn schedutil_select(&self, utilization: u8, min: u32, max: u32) -> u32 {
        // Linear scaling based on utilization
        let range = max - min;
        min + (range as u32 * utilization as u32 / 100)
    }

    /// Set frequency directly (userspace governor)
    pub fn set_frequency(&mut self, cpu_id: u32, freq_mhz: u32) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        if !cpu.online {
            return Err(PStateError::CpuOffline(cpu_id));
        }

        let min = cpu.min_frequency();
        let max = cpu.max_frequency();
        if freq_mhz < min || freq_mhz > max {
            return Err(PStateError::FrequencyOutOfRange {
                requested: freq_mhz,
                min,
                max,
            });
        }

        let pstate_idx = cpu.find_closest_pstate(freq_mhz);
        let _old_pstate = cpu.current_pstate;
        cpu.set_pstate(pstate_idx)?;

        if let Some(pstate) = cpu.pstates.get(pstate_idx as usize).cloned() {
            self.events
                .push_back(PowerEvent::PStateChange(cpu_id, pstate));
        }

        Ok(())
    }

    /// Set frequency limits
    pub fn set_frequency_limits(
        &mut self,
        cpu_id: u32,
        min_mhz: u32,
        max_mhz: u32,
    ) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.set_frequency_limits(min_mhz, max_mhz);
        Ok(())
    }

    /// Enable/disable turbo boost
    pub fn set_turbo(&mut self, cpu_id: u32, enabled: bool) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.turbo_enabled = enabled;
        Ok(())
    }

    /// Set CPU online status
    pub fn set_cpu_online(&mut self, cpu_id: u32, online: bool) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.online = online;
        Ok(())
    }

    /// Set governor thresholds
    pub fn set_thresholds(&mut self, up: u8, down: u8) {
        self.up_threshold = up.min(100);
        self.down_threshold = down.min(self.up_threshold);
    }

    /// Set sampling rate
    pub fn set_sampling_rate(&mut self, rate_us: u32) {
        self.sampling_rate_us = rate_us.max(1000); // Min 1ms
    }

    /// Check if sampling is due
    pub fn is_sample_due(&self) -> bool {
        self.last_sample.elapsed().as_micros() >= self.sampling_rate_us as u128
    }

    /// Mark sample taken
    pub fn mark_sampled(&mut self) {
        self.last_sample = Instant::now();
    }

    /// Run governor on all CPUs
    pub fn run_governor_all(&mut self) -> Vec<(u32, PStateResult<u32>)> {
        let cpu_ids: Vec<u32> = (0..self.cpus.len() as u32).collect();
        cpu_ids
            .into_iter()
            .map(|id| (id, self.run_governor(id)))
            .collect()
    }

    /// Get total power consumption estimate
    pub fn total_power_mw(&self) -> u32 {
        self.cpus
            .iter()
            .filter(|cpu| cpu.online)
            .map(|cpu| cpu.estimated_power_mw())
            .sum()
    }

    /// Get statistics
    pub fn stats(&self) -> &PowerStats {
        &self.stats
    }

    /// Poll for pending events
    pub fn poll_event(&mut self) -> Option<PowerEvent> {
        self.events.pop_front()
    }

    /// Set energy performance preference for a CPU
    pub fn set_energy_perf_preference(&mut self, cpu_id: u32, epp: u8) -> PStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.energy_perf_preference = epp;
        Ok(())
    }
}

impl Default for PStateManager {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pstates() -> Vec<PState> {
        vec![
            PState::new(0, 800, 700, 5000),
            PState::new(1, 1600, 900, 18000),
            PState::new(2, 2400, 1100, 42000),
            PState::new(3, 3200, 1200, 75000),
        ]
    }

    #[test]
    fn test_p_state_error_display() {
        let err = PStateError::InvalidCpu(5);
        assert!(format!("{}", err).contains("Invalid CPU"));
    }

    #[test]
    fn test_p_state_governor_display() {
        assert_eq!(format!("{}", PStateGovernor::Ondemand), "ondemand");
    }

    #[test]
    fn test_cpu_p_state_creation() {
        let cpu = CpuPState::new(0, test_pstates());
        assert_eq!(cpu.cpu_id, 0);
        assert_eq!(cpu.current_pstate, 0);
        assert!(cpu.online);
        assert_eq!(cpu.pstates.len(), 4);
    }

    #[test]
    fn test_cpu_p_state_frequencies() {
        let cpu = CpuPState::new(0, test_pstates());
        assert_eq!(cpu.current_frequency(), 800);
        assert_eq!(cpu.min_frequency(), 800);
        assert_eq!(cpu.max_frequency(), 3200);
    }

    #[test]
    fn test_cpu_p_state_set_pstate() {
        let mut cpu = CpuPState::new(0, test_pstates());
        cpu.set_pstate(2).unwrap();
        assert_eq!(cpu.current_pstate, 2);
        assert_eq!(cpu.current_frequency(), 2400);
    }

    #[test]
    fn test_cpu_p_state_invalid_pstate() {
        let mut cpu = CpuPState::new(0, test_pstates());
        let result = cpu.set_pstate(10);
        assert!(matches!(result, Err(PStateError::InvalidPState(10))));
    }

    #[test]
    fn test_cpu_p_state_find_by_frequency() {
        let cpu = CpuPState::new(0, test_pstates());
        assert_eq!(cpu.find_pstate_by_frequency(1600), Some(1));
        assert_eq!(cpu.find_pstate_by_frequency(1000), None);
    }

    #[test]
    fn test_cpu_p_state_find_closest() {
        let cpu = CpuPState::new(0, test_pstates());
        // With pstates [800, 1600, 2400, 3200]
        // 1000 is closer to 800 than 1600 (200 vs 600)
        assert_eq!(cpu.find_closest_pstate(1000), 0);
        // 2000 is equidistant between 1600 and 2400 (400 each), algorithm picks lower
        let closest = cpu.find_closest_pstate(2000);
        assert!(closest == 1 || closest == 2); // Either is valid
    }

    #[test]
    fn test_cpu_p_state_utilization() {
        let mut cpu = CpuPState::new(0, test_pstates());
        cpu.update_utilization(50);
        assert_eq!(cpu.utilization, 50);
        assert_eq!(cpu.average_utilization(), 50);

        cpu.update_utilization(70);
        cpu.update_utilization(60);
        let avg = cpu.average_utilization();
        assert!(avg > 50 && avg < 70);
    }

    #[test]
    fn test_cpu_p_state_residency_ratio() {
        let mut cpu = CpuPState::new(0, test_pstates());
        cpu.state_time_us[0] = 500;
        cpu.state_time_us[1] = 500;

        let ratio = cpu.state_residency_ratio(0);
        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_cpu_p_state_power_estimate() {
        let mut cpu = CpuPState::new(0, test_pstates());
        cpu.set_pstate(3).unwrap();
        cpu.update_utilization(100);
        let power = cpu.estimated_power_mw();
        assert!(power > 0);
    }

    #[test]
    fn test_cpu_p_state_frequency_limits() {
        let mut cpu = CpuPState::new(0, test_pstates());
        cpu.set_frequency_limits(1600, 2400);
        assert_eq!(cpu.min_pstate, 1);
        assert_eq!(cpu.max_pstate, 2);
    }

    #[test]
    fn test_p_state_manager_creation() {
        let manager = PStateManager::new(4);
        assert_eq!(manager.cpu_count(), 4);
        assert_eq!(manager.governor(), PStateGovernor::Ondemand);
    }

    #[test]
    fn test_p_state_manager_with_pstates() {
        let manager = PStateManager::with_pstates(2, test_pstates());
        assert_eq!(manager.cpu_count(), 2);
        assert_eq!(manager.cpu(0).unwrap().pstates.len(), 4);
    }

    #[test]
    fn test_p_state_manager_set_governor() {
        let mut manager = PStateManager::new(1);
        manager.set_governor(PStateGovernor::Performance);
        assert_eq!(manager.governor(), PStateGovernor::Performance);
    }

    #[test]
    fn test_p_state_manager_cpu_access() {
        let manager = PStateManager::new(2);
        assert!(manager.cpu(0).is_ok());
        assert!(manager.cpu(1).is_ok());
        assert!(matches!(manager.cpu(5), Err(PStateError::InvalidCpu(5))));
    }

    #[test]
    fn test_p_state_manager_update_utilization() {
        let mut manager = PStateManager::new(1);
        manager.update_utilization(0, 50).unwrap();
        assert_eq!(manager.cpu(0).unwrap().utilization, 50);
    }

    #[test]
    fn test_p_state_manager_governor_performance() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Performance);
        manager.update_utilization(0, 50).unwrap();
        let pstate = manager.run_governor(0).unwrap();
        assert_eq!(pstate, 3); // Max P-state
    }

    #[test]
    fn test_p_state_manager_governor_powersave() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Powersave);
        manager.update_utilization(0, 50).unwrap();
        let pstate = manager.run_governor(0).unwrap();
        assert_eq!(pstate, 0); // Min P-state
    }

    #[test]
    fn test_p_state_manager_governor_ondemand() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Ondemand);
        manager.set_thresholds(80, 20);

        // High utilization - jump to max
        manager.update_utilization(0, 90).unwrap();
        // Need to run governor multiple times to build up history
        for _ in 0..4 {
            manager.update_utilization(0, 90).unwrap();
        }
        let pstate = manager.run_governor(0).unwrap();
        // On first high-load detection, ondemand should jump to max
        assert!(pstate >= 3, "Expected max P-state (3), got {}", pstate);
    }

    #[test]
    fn test_p_state_manager_governor_conservative() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Conservative);
        manager.set_thresholds(80, 20);

        // High utilization - step up one at a time
        for _ in 0..3 {
            manager.update_utilization(0, 90).unwrap();
            manager.run_governor(0).unwrap();
        }
        let pstate = manager.cpu(0).unwrap().current_pstate;
        assert!(pstate > 0);
    }

    #[test]
    fn test_p_state_manager_set_frequency() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Userspace);

        manager.set_frequency(0, 1600).unwrap();
        assert_eq!(manager.cpu(0).unwrap().current_pstate, 1);
    }

    #[test]
    fn test_p_state_manager_frequency_limits() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_frequency_limits(0, 1600, 2400).unwrap();

        let cpu = manager.cpu(0).unwrap();
        assert_eq!(cpu.min_pstate, 1);
        assert_eq!(cpu.max_pstate, 2);
    }

    #[test]
    fn test_p_state_manager_turbo() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_turbo(0, false).unwrap();
        assert!(!manager.cpu(0).unwrap().turbo_enabled);
    }

    #[test]
    fn test_p_state_manager_cpu_offline() {
        let mut manager = PStateManager::new(1);
        manager.set_cpu_online(0, false).unwrap();

        let result = manager.run_governor(0);
        assert!(matches!(result, Err(PStateError::CpuOffline(0))));
    }

    #[test]
    fn test_p_state_manager_poll_event() {
        let mut manager = PStateManager::with_pstates(1, test_pstates());
        manager.set_governor(PStateGovernor::Performance);
        manager.update_utilization(0, 90).unwrap();
        manager.run_governor(0).unwrap();

        let event = manager.poll_event();
        assert!(matches!(event, Some(PowerEvent::PStateChange(_, _))));
    }

    #[test]
    fn test_p_state_manager_total_power() {
        let manager = PStateManager::with_pstates(2, test_pstates());
        let power = manager.total_power_mw();
        assert!(power > 0);
    }

    #[test]
    fn test_p_state_manager_sampling() {
        let mut manager = PStateManager::new(1);
        manager.set_sampling_rate(1000);
        manager.mark_sampled();

        // Should not be due immediately
        assert!(!manager.is_sample_due());

        // After waiting, should be due
        std::thread::sleep(Duration::from_millis(2));
        assert!(manager.is_sample_due());
    }

    #[test]
    fn test_p_state_manager_run_governor_all() {
        let mut manager = PStateManager::with_pstates(4, test_pstates());
        manager.set_governor(PStateGovernor::Performance);

        let results = manager.run_governor_all();
        assert_eq!(results.len(), 4);
        for (_, result) in results {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_p_state_manager_energy_perf_preference() {
        let mut manager = PStateManager::new(1);
        manager.set_energy_perf_preference(0, 200).unwrap();
        assert_eq!(manager.cpu(0).unwrap().energy_perf_preference, 200);
    }
}
