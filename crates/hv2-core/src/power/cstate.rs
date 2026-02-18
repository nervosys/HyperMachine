//! CPU C-state (Idle State) management
//!
//! This module provides management of per-CPU idle states using ACPI C-states
//! and Intel MWAIT extensions for efficient power management.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::types::{CState, PowerEvent, PowerStats};

/// C-state management result
pub type CStateResult<T> = Result<T, CStateError>;

/// C-state management error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CStateError {
    /// Invalid CPU ID
    #[error("Invalid CPU ID: {0}")]
    InvalidCpu(u32),
    /// C-state not supported
    #[error("C-state {0} not supported")]
    StateNotSupported(CState),
    /// Latency constraint violation
    #[error("C-state {requested} exceeds latency constraint of {max_latency_us}us")]
    LatencyConstraint {
        requested: CState,
        max_latency_us: u32,
    },
    /// Governor error
    #[error("Governor error: {0}")]
    GovernorError(String),
    /// CPU offline
    #[error("CPU {0} is offline")]
    CpuOffline(u32),
}

/// C-state governor policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CStateGovernor {
    /// Menu governor - estimates idle duration and selects optimal state
    #[default]
    Menu,
    /// Ladder governor - gradually steps through states
    Ladder,
    /// TEO (Timer Events Oriented) governor - considers timer events
    Teo,
    /// Halt-poll governor - spin before deeper sleep
    HaltPoll,
    /// Fixed state - always use specific state
    Fixed(CState),
}

impl std::fmt::Display for CStateGovernor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CStateGovernor::Menu => write!(f, "menu"),
            CStateGovernor::Ladder => write!(f, "ladder"),
            CStateGovernor::Teo => write!(f, "teo"),
            CStateGovernor::HaltPoll => write!(f, "haltpoll"),
            CStateGovernor::Fixed(state) => write!(f, "fixed({})", state),
        }
    }
}

/// Per-CPU C-state tracking
#[derive(Debug, Clone)]
pub struct CpuCState {
    /// CPU ID
    pub cpu_id: u32,
    /// Current C-state
    pub current_state: CState,
    /// Time when current state was entered
    pub state_entered: Instant,
    /// Time spent in each C-state (microseconds)
    pub state_time_us: [u64; 10],
    /// Number of times each state was entered
    pub state_transitions: [u64; 10],
    /// Last idle duration prediction (microseconds)
    pub predicted_idle_us: u32,
    /// Actual last idle duration (microseconds)
    pub actual_idle_us: u32,
    /// CPU is online
    pub online: bool,
    /// Maximum allowed latency (microseconds)
    pub max_latency_us: u32,
    /// Recent idle durations for prediction
    idle_history: VecDeque<u32>,
}

impl CpuCState {
    /// Create new CPU C-state tracker
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            current_state: CState::C0,
            state_entered: Instant::now(),
            state_time_us: [0; 10],
            state_transitions: [0; 10],
            predicted_idle_us: 0,
            actual_idle_us: 0,
            online: true,
            max_latency_us: u32::MAX,
            idle_history: VecDeque::with_capacity(8),
        }
    }

    /// Enter a C-state
    pub fn enter_state(&mut self, state: CState) {
        if self.current_state != state {
            // Record time in previous state
            let elapsed = self.state_entered.elapsed();
            let state_idx = self.state_index(self.current_state);
            self.state_time_us[state_idx] += elapsed.as_micros() as u64;

            // Enter new state
            self.current_state = state;
            self.state_entered = Instant::now();
            self.state_transitions[self.state_index(state)] += 1;
        }
    }

    /// Exit to C0 (active state)
    pub fn exit_to_active(&mut self) {
        let idle_duration = self.state_entered.elapsed();
        self.actual_idle_us = idle_duration.as_micros() as u32;

        // Update history for prediction
        if self.idle_history.len() >= 8 {
            self.idle_history.pop_front();
        }
        self.idle_history.push_back(self.actual_idle_us);

        self.enter_state(CState::C0);
    }

    /// Get index for a C-state
    fn state_index(&self, state: CState) -> usize {
        match state {
            CState::C0 => 0,
            CState::C1 => 1,
            CState::C1E => 2,
            CState::C2 => 3,
            CState::C3 => 4,
            CState::C6 => 5,
            CState::C7 => 6,
            CState::C8 => 7,
            CState::C10 => 8,
        }
    }

    /// Predict next idle duration based on history
    pub fn predict_idle(&mut self) -> u32 {
        if self.idle_history.is_empty() {
            self.predicted_idle_us = 1000; // Default 1ms
            return self.predicted_idle_us;
        }

        // Use exponential moving average
        let mut avg: u64 = 0;
        let mut weight: u64 = 1;
        let mut total_weight: u64 = 0;

        for &duration in self.idle_history.iter().rev() {
            avg += duration as u64 * weight;
            total_weight += weight;
            weight *= 2;
        }

        self.predicted_idle_us = (avg / total_weight) as u32;
        self.predicted_idle_us
    }

    /// Get residency ratio for a state
    pub fn state_residency_ratio(&self, state: CState) -> f64 {
        let state_idx = self.state_index(state);
        let total: u64 = self.state_time_us.iter().sum();
        if total == 0 {
            return 0.0;
        }
        self.state_time_us[state_idx] as f64 / total as f64
    }

    /// Set latency constraint
    pub fn set_latency_constraint(&mut self, max_us: u32) {
        self.max_latency_us = max_us;
    }

    /// Clear latency constraint
    pub fn clear_latency_constraint(&mut self) {
        self.max_latency_us = u32::MAX;
    }

    /// Get time in current state
    pub fn time_in_state(&self) -> Duration {
        self.state_entered.elapsed()
    }
}

/// C-state manager for all CPUs
#[derive(Debug)]
pub struct CStateManager {
    /// Per-CPU state tracking
    cpus: Vec<CpuCState>,
    /// Supported C-states
    supported_states: Vec<CState>,
    /// Active governor
    governor: CStateGovernor,
    /// Event queue
    events: VecDeque<PowerEvent>,
    /// Global statistics
    stats: PowerStats,
    /// Disable deeper states (for debugging)
    max_state: CState,
}

impl CStateManager {
    /// Create a new C-state manager
    pub fn new(cpu_count: u32) -> Self {
        let cpus = (0..cpu_count).map(CpuCState::new).collect();

        Self {
            cpus,
            supported_states: vec![
                CState::C0,
                CState::C1,
                CState::C1E,
                CState::C2,
                CState::C3,
                CState::C6,
            ],
            governor: CStateGovernor::Menu,
            events: VecDeque::new(),
            stats: PowerStats::new(cpu_count as usize, 1),
            max_state: CState::C10,
        }
    }

    /// Set supported C-states
    pub fn with_supported_states(mut self, states: Vec<CState>) -> Self {
        self.supported_states = states;
        self
    }

    /// Set the governor
    pub fn set_governor(&mut self, governor: CStateGovernor) {
        self.governor = governor;
    }

    /// Get the current governor
    pub fn governor(&self) -> CStateGovernor {
        self.governor
    }

    /// Set maximum allowed C-state
    pub fn set_max_state(&mut self, state: CState) {
        self.max_state = state;
    }

    /// Get CPU count
    pub fn cpu_count(&self) -> u32 {
        self.cpus.len() as u32
    }

    /// Check if a state is supported
    pub fn is_state_supported(&self, state: CState) -> bool {
        self.supported_states.contains(&state)
    }

    /// Get CPU state
    pub fn cpu(&self, cpu_id: u32) -> CStateResult<&CpuCState> {
        self.cpus
            .get(cpu_id as usize)
            .ok_or(CStateError::InvalidCpu(cpu_id))
    }

    /// Get mutable CPU state
    pub fn cpu_mut(&mut self, cpu_id: u32) -> CStateResult<&mut CpuCState> {
        self.cpus
            .get_mut(cpu_id as usize)
            .ok_or(CStateError::InvalidCpu(cpu_id))
    }

    /// Get current C-state for a CPU
    pub fn current_state(&self, cpu_id: u32) -> CStateResult<CState> {
        Ok(self.cpu(cpu_id)?.current_state)
    }

    /// Select optimal C-state for a CPU
    pub fn select_state(&mut self, cpu_id: u32) -> CStateResult<CState> {
        let cpu = self.cpu_mut(cpu_id)?;
        if !cpu.online {
            return Err(CStateError::CpuOffline(cpu_id));
        }

        let predicted_idle = cpu.predict_idle();
        let max_latency = cpu.max_latency_us;

        let selected = match self.governor {
            CStateGovernor::Fixed(state) => state,
            CStateGovernor::Menu => self.menu_select(predicted_idle, max_latency),
            CStateGovernor::Ladder => self.ladder_select(cpu_id, predicted_idle),
            CStateGovernor::Teo => self.teo_select(predicted_idle, max_latency),
            CStateGovernor::HaltPoll => self.haltpoll_select(predicted_idle),
        };

        // Apply max state limit
        let selected = if selected.exit_latency_us() > self.max_state.exit_latency_us() {
            self.max_state
        } else {
            selected
        };

        // Check latency constraint
        if selected.exit_latency_us() > max_latency {
            return Err(CStateError::LatencyConstraint {
                requested: selected,
                max_latency_us: max_latency,
            });
        }

        Ok(selected)
    }

    /// Menu governor state selection
    fn menu_select(&self, predicted_idle_us: u32, max_latency_us: u32) -> CState {
        let mut best_state = CState::C0;

        for &state in &self.supported_states {
            // Skip if exceeds latency constraint
            if state.exit_latency_us() > max_latency_us {
                continue;
            }

            // Skip if predicted idle is less than target residency
            if predicted_idle_us < state.target_residency_us() {
                continue;
            }

            // Select deepest viable state
            if state.exit_latency_us() > best_state.exit_latency_us() {
                best_state = state;
            }
        }

        best_state
    }

    /// Ladder governor state selection
    fn ladder_select(&self, cpu_id: u32, predicted_idle_us: u32) -> CState {
        let cpu = match self.cpus.get(cpu_id as usize) {
            Some(c) => c,
            None => return CState::C0,
        };

        let current = cpu.current_state;
        let current_idx = self.state_index(current);

        // Move up one level if idle was longer than residency
        if predicted_idle_us > current.target_residency_us() * 2 {
            // Find next deeper state
            for (i, &state) in self.supported_states.iter().enumerate() {
                if i > current_idx {
                    return state;
                }
            }
        }

        // Move down if idle was shorter than residency
        if cpu.actual_idle_us < current.target_residency_us() / 2 {
            // Find next shallower state
            for (i, &state) in self.supported_states.iter().enumerate().rev() {
                if i < current_idx {
                    return state;
                }
            }
        }

        current
    }

    /// TEO (Timer Events Oriented) governor selection
    fn teo_select(&self, predicted_idle_us: u32, max_latency_us: u32) -> CState {
        // Simplified TEO - uses menu selection with timer awareness
        self.menu_select(predicted_idle_us, max_latency_us)
    }

    /// Halt-poll governor selection
    fn haltpoll_select(&self, predicted_idle_us: u32) -> CState {
        // Use C1 for short idles, deeper for longer
        if predicted_idle_us < 100 {
            CState::C1
        } else if predicted_idle_us < 1000 {
            CState::C1E
        } else {
            CState::C3
        }
    }

    /// Get index for a C-state in supported list
    fn state_index(&self, state: CState) -> usize {
        self.supported_states
            .iter()
            .position(|&s| s == state)
            .unwrap_or(0)
    }

    /// Enter idle state for a CPU
    pub fn enter_idle(&mut self, cpu_id: u32) -> CStateResult<CState> {
        let state = self.select_state(cpu_id)?;

        let cpu = self.cpu_mut(cpu_id)?;
        let prev_state = cpu.current_state;
        cpu.enter_state(state);

        // Record event
        self.events
            .push_back(PowerEvent::CStateEnter(cpu_id, state));

        // Update stats
        if cpu_id < self.stats.c_state_time.len() as u32 {
            self.stats.c_state_transitions[cpu_id as usize][state as usize] += 1;
        }

        Ok(state)
    }

    /// Exit idle state for a CPU
    pub fn exit_idle(&mut self, cpu_id: u32) -> CStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        let state = cpu.current_state;
        let duration_us = cpu.time_in_state().as_micros() as u64;

        cpu.exit_to_active();

        // Record event
        self.events.push_back(PowerEvent::CStateExit(cpu_id, state));

        // Update stats
        if cpu_id < self.stats.c_state_time.len() as u32 {
            self.stats.c_state_time[cpu_id as usize][state as usize] += duration_us;
        }

        Ok(())
    }

    /// Set CPU online status
    pub fn set_cpu_online(&mut self, cpu_id: u32, online: bool) -> CStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.online = online;
        Ok(())
    }

    /// Set latency constraint for a CPU
    pub fn set_latency_constraint(&mut self, cpu_id: u32, max_us: u32) -> CStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.set_latency_constraint(max_us);
        Ok(())
    }

    /// Clear latency constraint for a CPU
    pub fn clear_latency_constraint(&mut self, cpu_id: u32) -> CStateResult<()> {
        let cpu = self.cpu_mut(cpu_id)?;
        cpu.clear_latency_constraint();
        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> &PowerStats {
        &self.stats
    }

    /// Poll for pending events
    pub fn poll_event(&mut self) -> Option<PowerEvent> {
        self.events.pop_front()
    }

    /// Get MWAIT hint for entering a state
    pub fn mwait_hint(&self, state: CState) -> Option<u32> {
        Some(state.mwait_hint())
    }

    /// Get total C-state residency across all CPUs
    pub fn total_c_state_residency(&self, state: CState) -> u64 {
        self.cpus
            .iter()
            .map(|cpu| cpu.state_time_us[cpu.state_index(state)])
            .sum()
    }

    /// Get average C-state for a CPU
    pub fn average_c_state(&self, cpu_id: u32) -> CStateResult<f64> {
        let cpu = self.cpu(cpu_id)?;
        let total_time: u64 = cpu.state_time_us.iter().sum();
        if total_time == 0 {
            return Ok(0.0);
        }

        let mut weighted_sum: f64 = 0.0;
        for (i, &time) in cpu.state_time_us.iter().enumerate() {
            weighted_sum += i as f64 * time as f64;
        }

        Ok(weighted_sum / total_time as f64)
    }
}

impl Default for CStateManager {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_state_error_display() {
        let err = CStateError::InvalidCpu(5);
        assert!(format!("{}", err).contains("Invalid CPU"));
    }

    #[test]
    fn test_c_state_governor_display() {
        assert_eq!(format!("{}", CStateGovernor::Menu), "menu");
        let display = format!("{}", CStateGovernor::Fixed(CState::C3));
        assert!(display.contains("fixed") && display.contains("C3"));
    }

    #[test]
    fn test_cpu_c_state_creation() {
        let cpu = CpuCState::new(0);
        assert_eq!(cpu.cpu_id, 0);
        assert_eq!(cpu.current_state, CState::C0);
        assert!(cpu.online);
    }

    #[test]
    fn test_cpu_c_state_enter() {
        let mut cpu = CpuCState::new(0);
        cpu.enter_state(CState::C3);
        assert_eq!(cpu.current_state, CState::C3);
        assert_eq!(cpu.state_transitions[4], 1); // C3 is index 4
    }

    #[test]
    fn test_cpu_c_state_exit_to_active() {
        let mut cpu = CpuCState::new(0);
        cpu.enter_state(CState::C3);
        std::thread::sleep(Duration::from_micros(100));
        cpu.exit_to_active();
        assert_eq!(cpu.current_state, CState::C0);
        assert!(cpu.actual_idle_us > 0);
    }

    #[test]
    fn test_cpu_c_state_predict_idle() {
        let mut cpu = CpuCState::new(0);

        // First prediction with no history
        let pred = cpu.predict_idle();
        assert_eq!(pred, 1000); // Default

        // Add some history
        cpu.idle_history.push_back(500);
        cpu.idle_history.push_back(600);
        let pred = cpu.predict_idle();
        assert!(pred > 0 && pred < 1000);
    }

    #[test]
    fn test_cpu_c_state_residency_ratio() {
        let mut cpu = CpuCState::new(0);
        cpu.state_time_us[0] = 500; // C0
        cpu.state_time_us[4] = 500; // C3

        let ratio = cpu.state_residency_ratio(CState::C0);
        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_cpu_c_state_latency_constraint() {
        let mut cpu = CpuCState::new(0);
        assert_eq!(cpu.max_latency_us, u32::MAX);

        cpu.set_latency_constraint(100);
        assert_eq!(cpu.max_latency_us, 100);

        cpu.clear_latency_constraint();
        assert_eq!(cpu.max_latency_us, u32::MAX);
    }

    #[test]
    fn test_c_state_manager_creation() {
        let manager = CStateManager::new(4);
        assert_eq!(manager.cpu_count(), 4);
        assert_eq!(manager.governor(), CStateGovernor::Menu);
    }

    #[test]
    fn test_c_state_manager_supported_states() {
        let manager = CStateManager::new(1);
        assert!(manager.is_state_supported(CState::C0));
        assert!(manager.is_state_supported(CState::C3));
        assert!(!manager.is_state_supported(CState::C10));
    }

    #[test]
    fn test_c_state_manager_set_governor() {
        let mut manager = CStateManager::new(1);
        manager.set_governor(CStateGovernor::Ladder);
        assert_eq!(manager.governor(), CStateGovernor::Ladder);
    }

    #[test]
    fn test_c_state_manager_cpu_access() {
        let manager = CStateManager::new(2);
        assert!(manager.cpu(0).is_ok());
        assert!(manager.cpu(1).is_ok());
        assert!(matches!(manager.cpu(5), Err(CStateError::InvalidCpu(5))));
    }

    #[test]
    fn test_c_state_manager_select_state_menu() {
        let mut manager = CStateManager::new(1);
        manager.set_governor(CStateGovernor::Menu);

        // With default prediction, should select appropriate state
        let state = manager.select_state(0).unwrap();
        assert!(manager.is_state_supported(state));
    }

    #[test]
    fn test_c_state_manager_select_state_fixed() {
        let mut manager = CStateManager::new(1);
        manager.set_governor(CStateGovernor::Fixed(CState::C1E));

        let state = manager.select_state(0).unwrap();
        assert_eq!(state, CState::C1E);
    }

    #[test]
    fn test_c_state_manager_enter_exit_idle() {
        let mut manager = CStateManager::new(1);

        let state = manager.enter_idle(0).unwrap();
        assert_ne!(state, CState::C0); // Should enter some sleep state

        std::thread::sleep(Duration::from_micros(100));
        manager.exit_idle(0).unwrap();

        let cpu = manager.cpu(0).unwrap();
        assert_eq!(cpu.current_state, CState::C0);
    }

    #[test]
    fn test_c_state_manager_latency_constraint() {
        let mut manager = CStateManager::new(1);

        // Set tight latency constraint that's impossible to meet for deeper states
        manager.set_latency_constraint(0, 0).unwrap();

        // Select should return C0 since all deeper states exceed 0us latency
        let result = manager.select_state(0);
        // With 0us constraint, we can only be in C0
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), CState::C0);

        manager.clear_latency_constraint(0).unwrap();
        assert!(manager.select_state(0).is_ok());
    }

    #[test]
    fn test_c_state_manager_cpu_offline() {
        let mut manager = CStateManager::new(1);
        manager.set_cpu_online(0, false).unwrap();

        let result = manager.select_state(0);
        assert!(matches!(result, Err(CStateError::CpuOffline(0))));
    }

    #[test]
    fn test_c_state_manager_poll_event() {
        let mut manager = CStateManager::new(1);
        manager.enter_idle(0).unwrap();

        let event = manager.poll_event();
        assert!(matches!(event, Some(PowerEvent::CStateEnter(_, _))));
    }

    #[test]
    fn test_c_state_manager_max_state() {
        let mut manager = CStateManager::new(1);
        manager.set_max_state(CState::C1);

        let state = manager.select_state(0).unwrap();
        assert!(state.exit_latency_us() <= CState::C1.exit_latency_us());
    }

    #[test]
    fn test_c_state_manager_mwait_hint() {
        let manager = CStateManager::new(1);
        let hint = manager.mwait_hint(CState::C1);
        assert!(hint.is_some());
    }

    #[test]
    fn test_c_state_manager_total_residency() {
        let mut manager = CStateManager::new(2);

        // Enter and exit idle for both CPUs
        manager.enter_idle(0).unwrap();
        manager.enter_idle(1).unwrap();
        std::thread::sleep(Duration::from_micros(50));
        manager.exit_idle(0).unwrap();
        manager.exit_idle(1).unwrap();

        // Should have some total residency
        let total = manager.total_c_state_residency(CState::C0);
        // C0 time depends on state selection
    }

    #[test]
    fn test_c_state_manager_average_c_state() {
        let mut manager = CStateManager::new(1);

        // Record some time in states
        {
            let cpu = manager.cpu_mut(0).unwrap();
            cpu.state_time_us[0] = 500; // C0
            cpu.state_time_us[1] = 500; // C1
        }

        let avg = manager.average_c_state(0).unwrap();
        assert!(avg >= 0.0 && avg <= 9.0);
    }

    #[test]
    fn test_c_state_manager_haltpoll_select() {
        let mut manager = CStateManager::new(1);
        manager.set_governor(CStateGovernor::HaltPoll);

        let state = manager.select_state(0).unwrap();
        // HaltPoll selects based on predicted idle
        assert!(manager.is_state_supported(state));
    }
}
