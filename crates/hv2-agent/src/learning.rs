//! Agent Learning System
//!
//! This module provides learning and adaptation capabilities for AI agents:
//! - Experience collection and storage
//! - Reward-based learning signals
//! - Policy updates from experience
//! - Skill acquisition and improvement
//! - Behavioral adaptation
//! - Learning rate scheduling

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// Learning error types
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LearningError {
    /// Experience not found
    #[error("Experience not found: {0}")]
    ExperienceNotFound(String),
    /// Skill not found
    #[error("Skill not found: {0}")]
    SkillNotFound(String),
    /// Policy not found
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    /// Invalid reward
    #[error("Invalid reward: {0}")]
    InvalidReward(String),
    /// Learning disabled
    #[error("Learning is disabled")]
    LearningDisabled,
    /// Capacity exceeded
    #[error("Capacity exceeded: {0}")]
    CapacityExceeded(usize),
    /// Convergence failure
    #[error("Convergence failed: {0}")]
    ConvergenceFailed(String),
}

/// Result type for learning operations
pub type LearningResult<T> = Result<T, LearningError>;

/// Reward signal for reinforcement learning
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reward {
    /// Reward value (can be negative for penalties)
    pub value: f64,
    /// Discount factor for future rewards
    pub discount: f64,
    /// Whether this is a terminal reward
    pub terminal: bool,
}

impl Default for Reward {
    fn default() -> Self {
        Self {
            value: 0.0,
            discount: 0.99,
            terminal: false,
        }
    }
}

impl Reward {
    /// Create a positive reward
    pub fn positive(value: f64) -> Self {
        Self {
            value: value.abs(),
            ..Default::default()
        }
    }

    /// Create a negative reward (penalty)
    pub fn negative(value: f64) -> Self {
        Self {
            value: -value.abs(),
            ..Default::default()
        }
    }

    /// Create a terminal reward
    pub fn terminal(value: f64) -> Self {
        Self {
            value,
            terminal: true,
            ..Default::default()
        }
    }

    /// Set discount factor
    pub fn with_discount(mut self, discount: f64) -> Self {
        self.discount = discount.clamp(0.0, 1.0);
        self
    }

    /// Discounted value at timestep t
    pub fn discounted(&self, timesteps: u32) -> f64 {
        self.value * self.discount.powi(timesteps as i32)
    }
}

/// State representation for learning
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LearningState {
    /// Discrete state
    Discrete(i64),
    /// Continuous state
    Continuous(Vec<f64>),
    /// Categorical state
    Categorical(String),
    /// Composite state
    Composite(HashMap<String, LearningState>),
}

impl LearningState {
    /// Get as discrete value
    pub fn as_discrete(&self) -> Option<i64> {
        match self {
            Self::Discrete(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as continuous vector
    pub fn as_continuous(&self) -> Option<&[f64]> {
        match self {
            Self::Continuous(v) => Some(v),
            _ => None,
        }
    }

    /// Get as categorical
    pub fn as_categorical(&self) -> Option<&str> {
        match self {
            Self::Categorical(s) => Some(s),
            _ => None,
        }
    }

    /// Dimensionality of state
    pub fn dimensions(&self) -> usize {
        match self {
            Self::Discrete(_) => 1,
            Self::Continuous(v) => v.len(),
            Self::Categorical(_) => 1,
            Self::Composite(m) => m.values().map(|v| v.dimensions()).sum(),
        }
    }
}

/// Action representation for learning
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActionValue {
    /// Discrete action (index)
    Discrete(i64),
    /// Continuous action
    Continuous(Vec<f64>),
    /// Named action
    Named(String),
    /// Parameterized action
    Parameterized { name: String, params: Vec<f64> },
}

impl ActionValue {
    /// Get as discrete value
    pub fn as_discrete(&self) -> Option<i64> {
        match self {
            Self::Discrete(v) => Some(*v),
            _ => None,
        }
    }

    /// Get action name
    pub fn name(&self) -> String {
        match self {
            Self::Discrete(v) => format!("action_{}", v),
            Self::Continuous(_) => "continuous".to_string(),
            Self::Named(n) => n.clone(),
            Self::Parameterized { name, .. } => name.clone(),
        }
    }
}

/// A single experience tuple (s, a, r, s')
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    /// Unique experience ID
    pub id: String,
    /// State before action
    pub state: LearningState,
    /// Action taken
    pub action: ActionValue,
    /// Reward received
    pub reward: Reward,
    /// Next state (None if terminal)
    pub next_state: Option<LearningState>,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Episode ID (for grouping)
    pub episode_id: Option<String>,
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl Experience {
    /// Create a new experience
    pub fn new(
        id: &str,
        state: LearningState,
        action: ActionValue,
        reward: Reward,
        next_state: Option<LearningState>,
    ) -> Self {
        Self {
            id: id.to_string(),
            state,
            action,
            reward,
            next_state,
            timestamp: SystemTime::now(),
            episode_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Set episode ID
    pub fn with_episode(mut self, episode_id: &str) -> Self {
        self.episode_id = Some(episode_id.to_string());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Check if terminal experience
    pub fn is_terminal(&self) -> bool {
        self.reward.terminal || self.next_state.is_none()
    }
}

/// Experience replay buffer
#[derive(Debug, Clone, Default)]
pub struct ExperienceBuffer {
    /// Stored experiences
    experiences: Vec<Experience>,
    /// Maximum capacity
    capacity: usize,
    /// Current write position (circular buffer)
    position: usize,
    /// Total experiences added
    total_added: u64,
}

impl ExperienceBuffer {
    /// Create a new buffer with capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            experiences: Vec::with_capacity(capacity),
            capacity,
            position: 0,
            total_added: 0,
        }
    }

    /// Add an experience
    pub fn add(&mut self, exp: Experience) {
        if self.experiences.len() < self.capacity {
            self.experiences.push(exp);
        } else {
            self.experiences[self.position] = exp;
        }
        self.position = (self.position + 1) % self.capacity;
        self.total_added += 1;
    }

    /// Sample random experiences
    pub fn sample(&self, batch_size: usize) -> Vec<&Experience> {
        use std::collections::HashSet;

        if self.experiences.is_empty() {
            return Vec::new();
        }

        let mut indices = HashSet::new();
        let max_idx = self.experiences.len();
        let actual_batch = batch_size.min(max_idx);

        // Simple deterministic sampling for reproducibility
        for i in 0..actual_batch {
            let idx = (i * 7 + 3) % max_idx; // Pseudo-random but deterministic
            indices.insert(idx);
        }

        indices.iter().map(|&i| &self.experiences[i]).collect()
    }

    /// Get all experiences
    pub fn all(&self) -> &[Experience] {
        &self.experiences
    }

    /// Get experiences for an episode
    pub fn episode(&self, episode_id: &str) -> Vec<&Experience> {
        self.experiences
            .iter()
            .filter(|e| e.episode_id.as_deref() == Some(episode_id))
            .collect()
    }

    /// Get recent experiences
    pub fn recent(&self, count: usize) -> Vec<&Experience> {
        let len = self.experiences.len();
        let start = len.saturating_sub(count);
        self.experiences[start..].iter().collect()
    }

    /// Buffer size
    pub fn len(&self) -> usize {
        self.experiences.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.experiences.is_empty()
    }

    /// Clear buffer
    pub fn clear(&mut self) {
        self.experiences.clear();
        self.position = 0;
    }

    /// Total experiences ever added
    pub fn total_added(&self) -> u64 {
        self.total_added
    }
}

/// Skill proficiency level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SkillLevel {
    /// No proficiency
    Novice,
    /// Basic proficiency
    Beginner,
    /// Intermediate proficiency
    Intermediate,
    /// High proficiency
    Advanced,
    /// Full proficiency
    Expert,
}

impl SkillLevel {
    /// Numeric value for the level
    pub fn value(&self) -> f64 {
        match self {
            Self::Novice => 0.0,
            Self::Beginner => 0.25,
            Self::Intermediate => 0.5,
            Self::Advanced => 0.75,
            Self::Expert => 1.0,
        }
    }

    /// From numeric proficiency (0.0 to 1.0)
    pub fn from_proficiency(p: f64) -> Self {
        if p < 0.2 {
            Self::Novice
        } else if p < 0.4 {
            Self::Beginner
        } else if p < 0.6 {
            Self::Intermediate
        } else if p < 0.8 {
            Self::Advanced
        } else {
            Self::Expert
        }
    }
}

impl fmt::Display for SkillLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Novice => write!(f, "novice"),
            Self::Beginner => write!(f, "beginner"),
            Self::Intermediate => write!(f, "intermediate"),
            Self::Advanced => write!(f, "advanced"),
            Self::Expert => write!(f, "expert"),
        }
    }
}

/// A learnable skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Skill name
    pub name: String,
    /// Skill description
    pub description: String,
    /// Current proficiency (0.0 to 1.0)
    pub proficiency: f64,
    /// Practice count
    pub practice_count: u64,
    /// Success count
    pub success_count: u64,
    /// Last practiced
    pub last_practiced: Option<SystemTime>,
    /// Learning rate for this skill
    pub learning_rate: f64,
    /// Prerequisites
    pub prerequisites: Vec<String>,
}

impl Skill {
    /// Create a new skill
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            proficiency: 0.0,
            practice_count: 0,
            success_count: 0,
            last_practiced: None,
            learning_rate: 0.1,
            prerequisites: Vec::new(),
        }
    }

    /// Set learning rate
    pub fn with_learning_rate(mut self, rate: f64) -> Self {
        self.learning_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Add prerequisite
    pub fn with_prerequisite(mut self, prereq: &str) -> Self {
        self.prerequisites.push(prereq.to_string());
        self
    }

    /// Get skill level
    pub fn level(&self) -> SkillLevel {
        SkillLevel::from_proficiency(self.proficiency)
    }

    /// Practice the skill (update based on outcome)
    pub fn practice(&mut self, success: bool) {
        self.practice_count += 1;
        if success {
            self.success_count += 1;
        }

        // Update proficiency
        let target = if success { 1.0 } else { 0.0 };
        self.proficiency += self.learning_rate * (target - self.proficiency);
        self.proficiency = self.proficiency.clamp(0.0, 1.0);

        self.last_practiced = Some(SystemTime::now());
    }

    /// Success rate
    pub fn success_rate(&self) -> f64 {
        if self.practice_count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.practice_count as f64
        }
    }

    /// Apply skill decay over time
    pub fn apply_decay(&mut self, decay_rate: f64) {
        if let Some(last) = self.last_practiced {
            if let Ok(elapsed) = last.elapsed() {
                let days = elapsed.as_secs_f64() / 86400.0;
                let decay = (-decay_rate * days).exp();
                self.proficiency *= decay;
            }
        }
    }
}

/// Learning rate schedule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LearningRateSchedule {
    /// Constant learning rate
    Constant(f64),
    /// Linear decay
    LinearDecay {
        initial: f64,
        final_rate: f64,
        steps: u64,
    },
    /// Exponential decay
    ExponentialDecay { initial: f64, decay: f64 },
    /// Step decay
    StepDecay {
        initial: f64,
        factor: f64,
        step_size: u64,
    },
}

impl LearningRateSchedule {
    /// Get learning rate at step
    pub fn rate_at(&self, step: u64) -> f64 {
        match self {
            Self::Constant(r) => *r,
            Self::LinearDecay {
                initial,
                final_rate,
                steps,
            } => {
                if step >= *steps {
                    *final_rate
                } else {
                    initial - (initial - final_rate) * (step as f64 / *steps as f64)
                }
            }
            Self::ExponentialDecay { initial, decay } => initial * decay.powf(step as f64),
            Self::StepDecay {
                initial,
                factor,
                step_size,
            } => initial * factor.powi((step / step_size) as i32),
        }
    }
}

impl Default for LearningRateSchedule {
    fn default() -> Self {
        Self::Constant(0.001)
    }
}

/// Learning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningConfig {
    /// Whether learning is enabled
    pub enabled: bool,
    /// Learning rate schedule
    pub lr_schedule: LearningRateSchedule,
    /// Experience buffer capacity
    pub buffer_capacity: usize,
    /// Batch size for learning
    pub batch_size: usize,
    /// Discount factor (gamma)
    pub discount: f64,
    /// Update frequency
    pub update_frequency: u64,
    /// Skill decay rate per day
    pub skill_decay_rate: f64,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lr_schedule: LearningRateSchedule::default(),
            buffer_capacity: 10000,
            batch_size: 32,
            discount: 0.99,
            update_frequency: 4,
            skill_decay_rate: 0.01,
        }
    }
}

/// Learning statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearningStats {
    /// Total experiences collected
    pub total_experiences: u64,
    /// Total updates performed
    pub total_updates: u64,
    /// Total episodes completed
    pub total_episodes: u64,
    /// Average reward
    pub avg_reward: f64,
    /// Best reward seen
    pub best_reward: f64,
    /// Current learning rate
    pub current_lr: f64,
    /// Skills learned count
    pub skills_learned: usize,
}

impl LearningStats {
    /// Update with new reward
    pub fn record_reward(&mut self, reward: f64) {
        let n = self.total_experiences as f64;
        self.avg_reward = (self.avg_reward * n + reward) / (n + 1.0);
        if reward > self.best_reward {
            self.best_reward = reward;
        }
    }
}

/// Learning system for agents
#[derive(Debug)]
pub struct LearningSystem {
    /// Configuration
    config: LearningConfig,
    /// Experience buffer
    buffer: ExperienceBuffer,
    /// Skills
    skills: HashMap<String, Skill>,
    /// Statistics
    stats: LearningStats,
    /// Current step
    step: u64,
    /// Current episode
    episode: u64,
}

impl Default for LearningSystem {
    fn default() -> Self {
        Self::new(LearningConfig::default())
    }
}

impl LearningSystem {
    /// Create a new learning system
    pub fn new(config: LearningConfig) -> Self {
        let buffer = ExperienceBuffer::new(config.buffer_capacity);
        Self {
            config,
            buffer,
            skills: HashMap::new(),
            stats: LearningStats::default(),
            step: 0,
            episode: 0,
        }
    }

    /// Record an experience
    pub fn record_experience(&mut self, exp: Experience) -> LearningResult<()> {
        if !self.config.enabled {
            return Err(LearningError::LearningDisabled);
        }

        self.stats.record_reward(exp.reward.value);
        self.stats.total_experiences += 1;
        self.buffer.add(exp);
        self.step += 1;

        Ok(())
    }

    /// Start a new episode
    pub fn start_episode(&mut self) -> String {
        self.episode += 1;
        format!("episode-{}", self.episode)
    }

    /// End current episode
    pub fn end_episode(&mut self) {
        self.stats.total_episodes += 1;
    }

    /// Register a skill
    pub fn register_skill(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
        self.stats.skills_learned = self.skills.len();
    }

    /// Get a skill
    pub fn get_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Practice a skill
    pub fn practice_skill(&mut self, name: &str, success: bool) -> LearningResult<()> {
        let skill = self
            .skills
            .get_mut(name)
            .ok_or_else(|| LearningError::SkillNotFound(name.to_string()))?;
        skill.practice(success);
        Ok(())
    }

    /// List all skills
    pub fn list_skills(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Get current learning rate
    pub fn current_learning_rate(&self) -> f64 {
        self.config.lr_schedule.rate_at(self.step)
    }

    /// Get sample batch for learning
    pub fn sample_batch(&self) -> Vec<&Experience> {
        self.buffer.sample(self.config.batch_size)
    }

    /// Should update (based on frequency)
    pub fn should_update(&self) -> bool {
        self.config.enabled && self.step.is_multiple_of(self.config.update_frequency)
    }

    /// Perform update (placeholder for actual learning)
    pub fn update(&mut self) -> LearningResult<()> {
        if !self.config.enabled {
            return Err(LearningError::LearningDisabled);
        }

        self.stats.total_updates += 1;
        self.stats.current_lr = self.current_learning_rate();

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> &LearningStats {
        &self.stats
    }

    /// Get configuration
    pub fn config(&self) -> &LearningConfig {
        &self.config
    }

    /// Get experience buffer
    pub fn buffer(&self) -> &ExperienceBuffer {
        &self.buffer
    }

    /// Enable learning
    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    /// Disable learning
    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    /// Check if learning is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Apply decay to all skills
    pub fn apply_skill_decay(&mut self) {
        for skill in self.skills.values_mut() {
            skill.apply_decay(self.config.skill_decay_rate);
        }
    }

    /// Current step
    pub fn current_step(&self) -> u64 {
        self.step
    }

    /// Current episode
    pub fn current_episode(&self) -> u64 {
        self.episode
    }
}

/// Thread-safe shared learning system
#[derive(Debug, Clone, Default)]
pub struct SharedLearning {
    inner: Arc<RwLock<LearningSystem>>,
}

impl SharedLearning {
    /// Create a new shared learning system
    pub fn new(system: LearningSystem) -> Self {
        Self {
            inner: Arc::new(RwLock::new(system)),
        }
    }

    /// Record an experience
    pub fn record_experience(&self, exp: Experience) -> LearningResult<()> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).record_experience(exp)
    }

    /// Start a new episode
    pub fn start_episode(&self) -> String {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).start_episode()
    }

    /// End current episode
    pub fn end_episode(&self) {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).end_episode();
    }

    /// Register a skill
    pub fn register_skill(&self, skill: Skill) {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).register_skill(skill);
    }

    /// Practice a skill
    pub fn practice_skill(&self, name: &str, success: bool) -> LearningResult<()> {
        self.inner.write().unwrap_or_else(|e| e.into_inner()).practice_skill(name, success)
    }

    /// Get statistics
    pub fn stats(&self) -> LearningStats {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).stats().clone()
    }

    /// Check if learning is enabled
    pub fn is_enabled(&self) -> bool {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).is_enabled()
    }

    /// Get current step
    pub fn current_step(&self) -> u64 {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).current_step()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reward_creation() {
        let r = Reward::positive(1.0);
        assert_eq!(r.value, 1.0);
        assert!(!r.terminal);

        let r = Reward::negative(0.5);
        assert_eq!(r.value, -0.5);

        let r = Reward::terminal(10.0);
        assert!(r.terminal);
    }

    #[test]
    fn test_reward_discounted() {
        let r = Reward::positive(1.0).with_discount(0.9);
        assert!((r.discounted(0) - 1.0).abs() < 0.001);
        assert!((r.discounted(1) - 0.9).abs() < 0.001);
        assert!((r.discounted(2) - 0.81).abs() < 0.001);
    }

    #[test]
    fn test_state_value_discrete() {
        let s = LearningState::Discrete(42);
        assert_eq!(s.as_discrete(), Some(42));
        assert_eq!(s.dimensions(), 1);
    }

    #[test]
    fn test_state_value_continuous() {
        let s = LearningState::Continuous(vec![1.0, 2.0, 3.0]);
        assert_eq!(s.as_continuous(), Some(&[1.0, 2.0, 3.0][..]));
        assert_eq!(s.dimensions(), 3);
    }

    #[test]
    fn test_state_value_categorical() {
        let s = LearningState::Categorical("state_a".to_string());
        assert_eq!(s.as_categorical(), Some("state_a"));
    }

    #[test]
    fn test_action_value_discrete() {
        let a = ActionValue::Discrete(5);
        assert_eq!(a.as_discrete(), Some(5));
        assert_eq!(a.name(), "action_5");
    }

    #[test]
    fn test_action_value_named() {
        let a = ActionValue::Named("move_forward".to_string());
        assert_eq!(a.name(), "move_forward");
    }

    #[test]
    fn test_experience_creation() {
        let exp = Experience::new(
            "exp-1",
            LearningState::Discrete(0),
            ActionValue::Discrete(1),
            Reward::positive(1.0),
            Some(LearningState::Discrete(1)),
        );

        assert_eq!(exp.id, "exp-1");
        assert!(!exp.is_terminal());
    }

    #[test]
    fn test_experience_terminal() {
        let exp = Experience::new(
            "exp-1",
            LearningState::Discrete(0),
            ActionValue::Discrete(1),
            Reward::terminal(10.0),
            None,
        );

        assert!(exp.is_terminal());
    }

    #[test]
    fn test_experience_with_episode() {
        let exp = Experience::new(
            "exp-1",
            LearningState::Discrete(0),
            ActionValue::Discrete(1),
            Reward::default(),
            None,
        )
        .with_episode("ep-1")
        .with_metadata("source", "test");

        assert_eq!(exp.episode_id, Some("ep-1".to_string()));
        assert_eq!(exp.metadata.get("source"), Some(&"test".to_string()));
    }

    #[test]
    fn test_experience_buffer() {
        let mut buffer = ExperienceBuffer::new(10);

        for i in 0..5 {
            buffer.add(Experience::new(
                &format!("exp-{}", i),
                LearningState::Discrete(i),
                ActionValue::Discrete(0),
                Reward::default(),
                None,
            ));
        }

        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.total_added(), 5);
    }

    #[test]
    fn test_experience_buffer_circular() {
        let mut buffer = ExperienceBuffer::new(3);

        for i in 0..5 {
            buffer.add(Experience::new(
                &format!("exp-{}", i),
                LearningState::Discrete(i),
                ActionValue::Discrete(0),
                Reward::default(),
                None,
            ));
        }

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.total_added(), 5);
    }

    #[test]
    fn test_experience_buffer_sample() {
        let mut buffer = ExperienceBuffer::new(100);

        for i in 0..50 {
            buffer.add(Experience::new(
                &format!("exp-{}", i),
                LearningState::Discrete(i),
                ActionValue::Discrete(0),
                Reward::default(),
                None,
            ));
        }

        let sample = buffer.sample(10);
        assert!(!sample.is_empty());
    }

    #[test]
    fn test_skill_level() {
        assert_eq!(SkillLevel::from_proficiency(0.0), SkillLevel::Novice);
        assert_eq!(SkillLevel::from_proficiency(0.3), SkillLevel::Beginner);
        assert_eq!(SkillLevel::from_proficiency(0.5), SkillLevel::Intermediate);
        assert_eq!(SkillLevel::from_proficiency(0.75), SkillLevel::Advanced);
        assert_eq!(SkillLevel::from_proficiency(1.0), SkillLevel::Expert);
    }

    #[test]
    fn test_skill_level_value() {
        assert_eq!(SkillLevel::Novice.value(), 0.0);
        assert_eq!(SkillLevel::Expert.value(), 1.0);
    }

    #[test]
    fn test_skill_creation() {
        let skill = Skill::new("coding", "Programming ability")
            .with_learning_rate(0.05)
            .with_prerequisite("typing");

        assert_eq!(skill.name, "coding");
        assert_eq!(skill.learning_rate, 0.05);
        assert_eq!(skill.prerequisites, vec!["typing"]);
    }

    #[test]
    fn test_skill_practice() {
        let mut skill = Skill::new("test", "Test skill").with_learning_rate(0.1);

        skill.practice(true);
        assert!(skill.proficiency > 0.0);
        assert_eq!(skill.practice_count, 1);
        assert_eq!(skill.success_count, 1);

        skill.practice(false);
        assert_eq!(skill.practice_count, 2);
        assert_eq!(skill.success_count, 1);
    }

    #[test]
    fn test_skill_success_rate() {
        let mut skill = Skill::new("test", "Test skill");

        skill.practice(true);
        skill.practice(true);
        skill.practice(false);

        assert!((skill.success_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_learning_rate_constant() {
        let schedule = LearningRateSchedule::Constant(0.01);
        assert_eq!(schedule.rate_at(0), 0.01);
        assert_eq!(schedule.rate_at(1000), 0.01);
    }

    #[test]
    fn test_learning_rate_linear_decay() {
        let schedule = LearningRateSchedule::LinearDecay {
            initial: 1.0,
            final_rate: 0.1,
            steps: 100,
        };

        assert_eq!(schedule.rate_at(0), 1.0);
        assert!((schedule.rate_at(50) - 0.55).abs() < 0.01);
        assert_eq!(schedule.rate_at(100), 0.1);
        assert_eq!(schedule.rate_at(200), 0.1);
    }

    #[test]
    fn test_learning_rate_exponential_decay() {
        let schedule = LearningRateSchedule::ExponentialDecay {
            initial: 1.0,
            decay: 0.9,
        };

        assert_eq!(schedule.rate_at(0), 1.0);
        assert!((schedule.rate_at(1) - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_learning_config_default() {
        let config = LearningConfig::default();
        assert!(config.enabled);
        assert_eq!(config.batch_size, 32);
        assert_eq!(config.discount, 0.99);
    }

    #[test]
    fn test_learning_stats_record_reward() {
        let mut stats = LearningStats::default();

        stats.record_reward(1.0);
        stats.total_experiences = 1;
        assert_eq!(stats.best_reward, 1.0);

        stats.record_reward(2.0);
        stats.total_experiences = 2;
        assert_eq!(stats.best_reward, 2.0);
    }

    #[test]
    fn test_learning_system_creation() {
        let system = LearningSystem::default();
        assert!(system.is_enabled());
        assert_eq!(system.current_step(), 0);
    }

    #[test]
    fn test_learning_system_record_experience() {
        let mut system = LearningSystem::default();

        let exp = Experience::new(
            "exp-1",
            LearningState::Discrete(0),
            ActionValue::Discrete(1),
            Reward::positive(1.0),
            Some(LearningState::Discrete(1)),
        );

        system.record_experience(exp).unwrap();
        assert_eq!(system.stats().total_experiences, 1);
        assert_eq!(system.current_step(), 1);
    }

    #[test]
    fn test_learning_system_episodes() {
        let mut system = LearningSystem::default();

        let ep1 = system.start_episode();
        assert!(ep1.starts_with("episode-"));

        system.end_episode();
        assert_eq!(system.stats().total_episodes, 1);
    }

    #[test]
    fn test_learning_system_skills() {
        let mut system = LearningSystem::default();

        system.register_skill(Skill::new("test", "Test skill"));
        assert!(system.get_skill("test").is_some());
        assert_eq!(system.list_skills().len(), 1);

        system.practice_skill("test", true).unwrap();
        assert!(system.get_skill("test").unwrap().proficiency > 0.0);
    }

    #[test]
    fn test_learning_system_enable_disable() {
        let mut system = LearningSystem::default();

        system.disable();
        assert!(!system.is_enabled());

        let result = system.record_experience(Experience::new(
            "exp-1",
            LearningState::Discrete(0),
            ActionValue::Discrete(0),
            Reward::default(),
            None,
        ));
        assert!(matches!(result, Err(LearningError::LearningDisabled)));

        system.enable();
        assert!(system.is_enabled());
    }

    #[test]
    fn test_shared_learning() {
        let system = LearningSystem::default();
        let shared = SharedLearning::new(system);

        assert!(shared.is_enabled());

        let ep = shared.start_episode();
        assert!(!ep.is_empty());

        shared.register_skill(Skill::new("test", "Test"));
        shared.practice_skill("test", true).unwrap();

        let stats = shared.stats();
        assert_eq!(stats.skills_learned, 1);
    }

    #[test]
    fn test_learning_error_display() {
        let err = LearningError::SkillNotFound("test".to_string());
        assert!(err.to_string().contains("not found"));

        let err = LearningError::LearningDisabled;
        assert!(err.to_string().contains("disabled"));
    }
}
