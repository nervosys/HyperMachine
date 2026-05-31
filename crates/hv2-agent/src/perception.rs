//! Agent Perception System
//!
//! This module provides environment observation capabilities for AI agents:
//! - Sensor abstraction for different input types
//! - Observable definitions and subscriptions
//! - Perception filtering and aggregation
//! - World model construction
//! - Change detection and notifications
//! - Multi-modal perception fusion

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Perception error types
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PerceptionError {
    /// Sensor not found
    #[error("Sensor not found: {0}")]
    SensorNotFound(String),
    /// Sensor already registered
    #[error("Sensor already registered: {0}")]
    SensorAlreadyRegistered(String),
    /// Observable not found
    #[error("Observable not found: {0}")]
    ObservableNotFound(String),
    /// Sensor read failed
    #[error("Sensor read failed: {0}")]
    ReadFailed(String),
    /// Invalid sensor configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    /// Perception timeout
    #[error("Perception timeout after {0:?}")]
    Timeout(Duration),
    /// Sensor disabled
    #[error("Sensor disabled: {0}")]
    SensorDisabled(String),
}

/// Result type for perception operations
pub type PerceptionResult<T> = Result<T, PerceptionError>;

/// Sensor type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensorType {
    /// VM resource metrics (CPU, memory, disk)
    Resource,
    /// Network traffic and connectivity
    Network,
    /// System events and logs
    Event,
    /// Performance counters
    Performance,
    /// Security observations
    Security,
    /// User/external input
    Input,
    /// Time-based observations
    Temporal,
    /// Custom sensor type
    Custom,
}

impl fmt::Display for SensorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource => write!(f, "resource"),
            Self::Network => write!(f, "network"),
            Self::Event => write!(f, "event"),
            Self::Performance => write!(f, "performance"),
            Self::Security => write!(f, "security"),
            Self::Input => write!(f, "input"),
            Self::Temporal => write!(f, "temporal"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// Observation value types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObservationValue {
    /// Boolean observation
    Boolean(bool),
    /// Integer observation
    Integer(i64),
    /// Float observation
    Float(f64),
    /// String observation
    String(String),
    /// Binary data
    Binary(Vec<u8>),
    /// Structured data (JSON-like)
    Structured(HashMap<String, ObservationValue>),
    /// List of values
    List(Vec<ObservationValue>),
    /// No value (null)
    Null,
}

impl ObservationValue {
    /// Check if value is truthy
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Boolean(b) => *b,
            Self::Integer(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::Binary(b) => !b.is_empty(),
            Self::Structured(m) => !m.is_empty(),
            Self::List(l) => !l.is_empty(),
            Self::Null => false,
        }
    }

    /// Get as boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as integer
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Get as string
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for ObservationValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean(b) => write!(f, "{}", b),
            Self::Integer(i) => write!(f, "{}", i),
            Self::Float(v) => write!(f, "{}", v),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Binary(b) => write!(f, "<{} bytes>", b.len()),
            Self::Structured(m) => write!(f, "{{...{} fields}}", m.len()),
            Self::List(l) => write!(f, "[...{} items]", l.len()),
            Self::Null => write!(f, "null"),
        }
    }
}

/// Quality/confidence of an observation
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObservationQuality {
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
    /// Freshness (time since observation)
    pub age: Duration,
    /// Accuracy estimate
    pub accuracy: f64,
}

impl Default for ObservationQuality {
    fn default() -> Self {
        Self {
            confidence: 1.0,
            age: Duration::ZERO,
            accuracy: 1.0,
        }
    }
}

impl ObservationQuality {
    /// Create with custom confidence
    pub fn with_confidence(confidence: f64) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Overall quality score
    pub fn score(&self) -> f64 {
        let freshness = 1.0 / (1.0 + self.age.as_secs_f64());
        (self.confidence + self.accuracy + freshness) / 3.0
    }
}

/// A single observation from a sensor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Observable name
    pub name: String,
    /// Observed value
    pub value: ObservationValue,
    /// Observation quality
    pub quality: ObservationQuality,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Source sensor ID
    pub source: String,
    /// Tags for categorization
    pub tags: Vec<String>,
}

impl Observation {
    /// Create a new observation
    pub fn new(name: &str, value: ObservationValue, source: &str) -> Self {
        Self {
            name: name.to_string(),
            value,
            quality: ObservationQuality::default(),
            timestamp: SystemTime::now(),
            source: source.to_string(),
            tags: Vec::new(),
        }
    }

    /// Set quality
    pub fn with_quality(mut self, quality: ObservationQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Check if observation has tag
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Get age of observation
    pub fn age(&self) -> Duration {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO)
    }
}

/// Observable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observable {
    /// Observable name
    pub name: String,
    /// Description
    pub description: String,
    /// Expected value type
    pub value_type: String,
    /// Unit of measurement
    pub unit: Option<String>,
    /// Minimum polling interval
    pub min_interval: Duration,
    /// Tags
    pub tags: Vec<String>,
}

impl Observable {
    /// Create a new observable
    pub fn new(name: &str, description: &str, value_type: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            value_type: value_type.to_string(),
            unit: None,
            min_interval: Duration::from_millis(100),
            tags: Vec::new(),
        }
    }

    /// Set unit
    pub fn with_unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.to_string());
        self
    }

    /// Set minimum interval
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// Add tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }
}

/// Sensor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorConfig {
    /// Polling interval
    pub poll_interval: Duration,
    /// Whether sensor is enabled
    pub enabled: bool,
    /// Buffer size for observations
    pub buffer_size: usize,
    /// Custom settings
    pub settings: HashMap<String, String>,
}

impl Default for SensorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            enabled: true,
            buffer_size: 100,
            settings: HashMap::new(),
        }
    }
}

impl SensorConfig {
    /// Set poll interval
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set buffer size
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Add custom setting
    pub fn with_setting(mut self, key: &str, value: &str) -> Self {
        self.settings.insert(key.to_string(), value.to_string());
        self
    }

    /// Disable sensor
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Sensor definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorDefinition {
    /// Sensor ID
    pub id: String,
    /// Sensor name
    pub name: String,
    /// Sensor type
    pub sensor_type: SensorType,
    /// Description
    pub description: String,
    /// Observables provided by this sensor
    pub observables: Vec<Observable>,
    /// Configuration
    pub config: SensorConfig,
}

impl SensorDefinition {
    /// Create a new sensor definition
    pub fn new(id: &str, name: &str, sensor_type: SensorType) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            sensor_type,
            description: String::new(),
            observables: Vec::new(),
            config: SensorConfig::default(),
        }
    }

    /// Set description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Add an observable
    pub fn with_observable(mut self, obs: Observable) -> Self {
        self.observables.push(obs);
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: SensorConfig) -> Self {
        self.config = config;
        self
    }
}

/// Sensor reading handler
pub type SensorHandler = Box<dyn Fn(&str) -> PerceptionResult<Observation> + Send + Sync>;

/// A registered sensor
pub struct Sensor {
    /// Sensor definition
    pub definition: SensorDefinition,
    /// Handler function
    handler: SensorHandler,
    /// Last reading time
    last_read: Option<SystemTime>,
    /// Observation buffer
    buffer: Vec<Observation>,
}

impl fmt::Debug for Sensor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sensor")
            .field("definition", &self.definition)
            .field("last_read", &self.last_read)
            .field("buffer_len", &self.buffer.len())
            .finish()
    }
}

impl Sensor {
    /// Create a new sensor
    pub fn new<F>(definition: SensorDefinition, handler: F) -> Self
    where
        F: Fn(&str) -> PerceptionResult<Observation> + Send + Sync + 'static,
    {
        Self {
            definition,
            handler: Box::new(handler),
            last_read: None,
            buffer: Vec::new(),
        }
    }

    /// Read an observable
    pub fn read(&mut self, observable: &str) -> PerceptionResult<Observation> {
        if !self.definition.config.enabled {
            return Err(PerceptionError::SensorDisabled(self.definition.id.clone()));
        }

        let obs = (self.handler)(observable)?;
        self.last_read = Some(SystemTime::now());

        // Add to buffer
        self.buffer.push(obs.clone());
        if self.buffer.len() > self.definition.config.buffer_size {
            self.buffer.remove(0);
        }

        Ok(obs)
    }

    /// Get buffered observations
    pub fn buffer(&self) -> &[Observation] {
        &self.buffer
    }

    /// Clear buffer
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Get recent observations
    pub fn recent(&self, max_age: Duration) -> Vec<&Observation> {
        let cutoff = SystemTime::now() - max_age;
        self.buffer
            .iter()
            .filter(|o| o.timestamp >= cutoff)
            .collect()
    }
}

/// Perception filter for subscriptions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerceptionFilter {
    /// Filter by sensor types
    pub sensor_types: Option<Vec<SensorType>>,
    /// Filter by observable names (prefix match)
    pub observable_prefix: Option<String>,
    /// Filter by tags
    pub tags: Option<Vec<String>>,
    /// Minimum quality score
    pub min_quality: Option<f64>,
}

impl PerceptionFilter {
    /// Create empty filter (matches all)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by sensor type
    pub fn with_sensor_type(mut self, sensor_type: SensorType) -> Self {
        self.sensor_types
            .get_or_insert_with(Vec::new)
            .push(sensor_type);
        self
    }

    /// Filter by observable prefix
    pub fn with_observable_prefix(mut self, prefix: &str) -> Self {
        self.observable_prefix = Some(prefix.to_string());
        self
    }

    /// Filter by tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.get_or_insert_with(Vec::new).push(tag.to_string());
        self
    }

    /// Set minimum quality
    pub fn with_min_quality(mut self, quality: f64) -> Self {
        self.min_quality = Some(quality);
        self
    }

    /// Check if observation matches filter
    pub fn matches(&self, obs: &Observation, sensor_type: SensorType) -> bool {
        // Check sensor type
        if let Some(ref types) = self.sensor_types {
            if !types.contains(&sensor_type) {
                return false;
            }
        }

        // Check observable prefix
        if let Some(ref prefix) = self.observable_prefix {
            if !obs.name.starts_with(prefix) {
                return false;
            }
        }

        // Check tags
        if let Some(ref tags) = self.tags {
            if !tags.iter().any(|t| obs.has_tag(t)) {
                return false;
            }
        }

        // Check quality
        if let Some(min_q) = self.min_quality {
            if obs.quality.score() < min_q {
                return false;
            }
        }

        true
    }
}

/// World model representing perceived state
#[derive(Debug, Clone, Default)]
pub struct WorldModel {
    /// Current observations by name
    observations: HashMap<String, Observation>,
    /// Observation history
    history: Vec<Observation>,
    /// Maximum history size
    max_history: usize,
    /// Last update time
    last_update: Option<SystemTime>,
}

impl WorldModel {
    /// Create a new world model
    pub fn new() -> Self {
        Self {
            observations: HashMap::new(),
            history: Vec::new(),
            max_history: 1000,
            last_update: None,
        }
    }

    /// Set maximum history size
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    /// Update with new observation
    pub fn update(&mut self, obs: Observation) {
        let name = obs.name.clone();

        // Add to history
        self.history.push(obs.clone());
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }

        // Update current state
        self.observations.insert(name, obs);
        self.last_update = Some(SystemTime::now());
    }

    /// Get current observation
    pub fn get(&self, name: &str) -> Option<&Observation> {
        self.observations.get(name)
    }

    /// Get current value
    pub fn get_value(&self, name: &str) -> Option<&ObservationValue> {
        self.observations.get(name).map(|o| &o.value)
    }

    /// List all current observations
    pub fn list(&self) -> Vec<&Observation> {
        self.observations.values().collect()
    }

    /// Get history for an observable
    pub fn history_for(&self, name: &str) -> Vec<&Observation> {
        self.history.iter().filter(|o| o.name == name).collect()
    }

    /// Get recent history
    pub fn recent_history(&self, max_age: Duration) -> Vec<&Observation> {
        let cutoff = SystemTime::now() - max_age;
        self.history
            .iter()
            .filter(|o| o.timestamp >= cutoff)
            .collect()
    }

    /// Detect changes between two time points
    pub fn detect_changes(
        &self,
        observable: &str,
        since: Duration,
    ) -> Vec<(&Observation, &Observation)> {
        let history: Vec<_> = self.history_for(observable);
        let cutoff = SystemTime::now() - since;

        let mut changes = Vec::new();
        let mut prev: Option<&Observation> = None;

        for obs in history {
            if obs.timestamp >= cutoff {
                if let Some(p) = prev {
                    // Detect value change
                    if p.value != obs.value {
                        changes.push((p, obs));
                    }
                }
            }
            prev = Some(obs);
        }

        changes
    }

    /// Get observation count
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// Clear all observations
    pub fn clear(&mut self) {
        self.observations.clear();
        self.history.clear();
        self.last_update = None;
    }
}

/// Perception system for managing sensors and observations
#[derive(Default)]
pub struct PerceptionSystem {
    /// Registered sensors
    sensors: HashMap<String, Sensor>,
    /// World model
    world_model: WorldModel,
    /// Active subscriptions
    subscriptions: HashMap<String, PerceptionFilter>,
}

impl fmt::Debug for PerceptionSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PerceptionSystem")
            .field("sensor_count", &self.sensors.len())
            .field("subscription_count", &self.subscriptions.len())
            .field("world_model", &self.world_model)
            .finish()
    }
}

impl PerceptionSystem {
    /// Create a new perception system
    pub fn new() -> Self {
        Self {
            sensors: HashMap::new(),
            world_model: WorldModel::new(),
            subscriptions: HashMap::new(),
        }
    }

    /// Register a sensor
    pub fn register_sensor(&mut self, sensor: Sensor) -> PerceptionResult<()> {
        let id = sensor.definition.id.clone();
        if self.sensors.contains_key(&id) {
            return Err(PerceptionError::SensorAlreadyRegistered(id));
        }
        self.sensors.insert(id, sensor);
        Ok(())
    }

    /// Unregister a sensor
    pub fn unregister_sensor(&mut self, id: &str) -> PerceptionResult<Sensor> {
        self.sensors
            .remove(id)
            .ok_or_else(|| PerceptionError::SensorNotFound(id.to_string()))
    }

    /// Get sensor by ID
    pub fn get_sensor(&self, id: &str) -> Option<&Sensor> {
        self.sensors.get(id)
    }

    /// Get mutable sensor
    pub fn get_sensor_mut(&mut self, id: &str) -> Option<&mut Sensor> {
        self.sensors.get_mut(id)
    }

    /// List all sensors
    pub fn list_sensors(&self) -> Vec<&SensorDefinition> {
        self.sensors.values().map(|s| &s.definition).collect()
    }

    /// List sensors by type
    pub fn list_sensors_by_type(&self, sensor_type: SensorType) -> Vec<&SensorDefinition> {
        self.sensors
            .values()
            .filter(|s| s.definition.sensor_type == sensor_type)
            .map(|s| &s.definition)
            .collect()
    }

    /// Read from a sensor
    pub fn read(&mut self, sensor_id: &str, observable: &str) -> PerceptionResult<Observation> {
        let sensor = self
            .sensors
            .get_mut(sensor_id)
            .ok_or_else(|| PerceptionError::SensorNotFound(sensor_id.to_string()))?;

        let obs = sensor.read(observable)?;

        // Update world model
        self.world_model.update(obs.clone());

        Ok(obs)
    }

    /// Subscribe to observations
    pub fn subscribe(&mut self, id: &str, filter: PerceptionFilter) {
        self.subscriptions.insert(id.to_string(), filter);
    }

    /// Unsubscribe
    pub fn unsubscribe(&mut self, id: &str) {
        self.subscriptions.remove(id);
    }

    /// Get world model
    pub fn world_model(&self) -> &WorldModel {
        &self.world_model
    }

    /// Get mutable world model
    pub fn world_model_mut(&mut self) -> &mut WorldModel {
        &mut self.world_model
    }

    /// Get matching observations for a subscription
    pub fn get_matching(&self, subscription_id: &str) -> Vec<&Observation> {
        let filter = match self.subscriptions.get(subscription_id) {
            Some(f) => f,
            None => return Vec::new(),
        };

        self.world_model
            .list()
            .into_iter()
            .filter(|obs| {
                // Find sensor type for this observation
                if let Some(sensor) = self
                    .sensors
                    .values()
                    .find(|s| s.definition.id == obs.source)
                {
                    filter.matches(obs, sensor.definition.sensor_type)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Enable a sensor
    pub fn enable_sensor(&mut self, id: &str) -> PerceptionResult<()> {
        let sensor = self
            .sensors
            .get_mut(id)
            .ok_or_else(|| PerceptionError::SensorNotFound(id.to_string()))?;
        sensor.definition.config.enabled = true;
        Ok(())
    }

    /// Disable a sensor
    pub fn disable_sensor(&mut self, id: &str) -> PerceptionResult<()> {
        let sensor = self
            .sensors
            .get_mut(id)
            .ok_or_else(|| PerceptionError::SensorNotFound(id.to_string()))?;
        sensor.definition.config.enabled = false;
        Ok(())
    }

    /// Get sensor count
    pub fn sensor_count(&self) -> usize {
        self.sensors.len()
    }
}

/// Thread-safe shared perception system
#[derive(Debug, Clone, Default)]
pub struct SharedPerception {
    inner: Arc<RwLock<PerceptionSystem>>,
}

impl SharedPerception {
    /// Create a new shared perception system
    pub fn new(system: PerceptionSystem) -> Self {
        Self {
            inner: Arc::new(RwLock::new(system)),
        }
    }

    /// Register a sensor
    pub fn register_sensor(&self, sensor: Sensor) -> PerceptionResult<()> {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .register_sensor(sensor)
    }

    /// Read from a sensor
    pub fn read(&self, sensor_id: &str, observable: &str) -> PerceptionResult<Observation> {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .read(sensor_id, observable)
    }

    /// Subscribe to observations
    pub fn subscribe(&self, id: &str, filter: PerceptionFilter) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .subscribe(id, filter);
    }

    /// Unsubscribe
    pub fn unsubscribe(&self, id: &str) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .unsubscribe(id);
    }

    /// Get current value
    pub fn get_value(&self, name: &str) -> Option<ObservationValue> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .world_model()
            .get_value(name)
            .cloned()
    }

    /// Get sensor count
    pub fn sensor_count(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .sensor_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_type_display() {
        assert_eq!(SensorType::Resource.to_string(), "resource");
        assert_eq!(SensorType::Network.to_string(), "network");
        assert_eq!(SensorType::Security.to_string(), "security");
    }

    #[test]
    fn test_observation_value_truthy() {
        assert!(ObservationValue::Boolean(true).is_truthy());
        assert!(!ObservationValue::Boolean(false).is_truthy());
        assert!(ObservationValue::Integer(42).is_truthy());
        assert!(!ObservationValue::Integer(0).is_truthy());
        assert!(ObservationValue::String("hello".to_string()).is_truthy());
        assert!(!ObservationValue::String(String::new()).is_truthy());
        assert!(!ObservationValue::Null.is_truthy());
    }

    #[test]
    fn test_observation_value_accessors() {
        let b = ObservationValue::Boolean(true);
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_int(), None);

        let i = ObservationValue::Integer(42);
        assert_eq!(i.as_int(), Some(42));
        assert_eq!(i.as_float(), Some(42.0));

        let f = ObservationValue::Float(std::f64::consts::PI);
        assert_eq!(f.as_float(), Some(std::f64::consts::PI));

        let s = ObservationValue::String("test".to_string());
        assert_eq!(s.as_str(), Some("test"));
    }

    #[test]
    fn test_observation_value_display() {
        assert_eq!(ObservationValue::Boolean(true).to_string(), "true");
        assert_eq!(ObservationValue::Integer(42).to_string(), "42");
        assert_eq!(ObservationValue::Null.to_string(), "null");
    }

    #[test]
    fn test_observation_quality() {
        let q = ObservationQuality::default();
        assert_eq!(q.confidence, 1.0);
        assert_eq!(q.accuracy, 1.0);

        let q2 = ObservationQuality::with_confidence(0.8);
        assert_eq!(q2.confidence, 0.8);
    }

    #[test]
    fn test_observation_quality_score() {
        let q = ObservationQuality::default();
        let score = q.score();
        assert!(score > 0.0 && score <= 1.0);
    }

    #[test]
    fn test_observation_creation() {
        let obs = Observation::new("cpu.usage", ObservationValue::Float(75.5), "cpu-sensor");

        assert_eq!(obs.name, "cpu.usage");
        assert_eq!(obs.source, "cpu-sensor");
        assert!(matches!(obs.value, ObservationValue::Float(f) if (f - 75.5).abs() < 0.001));
    }

    #[test]
    fn test_observation_tags() {
        let obs = Observation::new("test", ObservationValue::Null, "sensor")
            .with_tag("important")
            .with_tag("vm-1");

        assert!(obs.has_tag("important"));
        assert!(obs.has_tag("vm-1"));
        assert!(!obs.has_tag("other"));
    }

    #[test]
    fn test_observable_creation() {
        let obs = Observable::new("cpu.usage", "CPU utilization percentage", "float")
            .with_unit("percent")
            .with_min_interval(Duration::from_millis(500))
            .with_tag("performance");

        assert_eq!(obs.name, "cpu.usage");
        assert_eq!(obs.unit, Some("percent".to_string()));
        assert_eq!(obs.min_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_sensor_config() {
        let config = SensorConfig::default()
            .with_poll_interval(Duration::from_secs(5))
            .with_buffer_size(50)
            .with_setting("debug", "true");

        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.buffer_size, 50);
        assert_eq!(config.settings.get("debug"), Some(&"true".to_string()));
    }

    #[test]
    fn test_sensor_config_disabled() {
        let config = SensorConfig::default().disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_sensor_definition() {
        let def = SensorDefinition::new("cpu-sensor", "CPU Sensor", SensorType::Resource)
            .with_description("Monitors CPU metrics")
            .with_observable(Observable::new("cpu.usage", "Usage", "float"));

        assert_eq!(def.id, "cpu-sensor");
        assert_eq!(def.sensor_type, SensorType::Resource);
        assert_eq!(def.observables.len(), 1);
    }

    #[test]
    fn test_sensor_read() {
        let def = SensorDefinition::new("test-sensor", "Test", SensorType::Custom);
        let mut sensor = Sensor::new(def, |name| {
            Ok(Observation::new(
                name,
                ObservationValue::Integer(42),
                "test-sensor",
            ))
        });

        let obs = sensor.read("test.value").unwrap();
        assert_eq!(obs.name, "test.value");
        assert_eq!(obs.value.as_int(), Some(42));
        assert_eq!(sensor.buffer().len(), 1);
    }

    #[test]
    fn test_sensor_disabled() {
        let def = SensorDefinition::new("test", "Test", SensorType::Custom)
            .with_config(SensorConfig::default().disabled());
        let mut sensor = Sensor::new(def, |_| {
            Ok(Observation::new("x", ObservationValue::Null, "x"))
        });

        let result = sensor.read("test");
        assert!(matches!(result, Err(PerceptionError::SensorDisabled(_))));
    }

    #[test]
    fn test_perception_filter_empty() {
        let filter = PerceptionFilter::new();
        let obs = Observation::new("test", ObservationValue::Null, "sensor");
        assert!(filter.matches(&obs, SensorType::Custom));
    }

    #[test]
    fn test_perception_filter_sensor_type() {
        let filter = PerceptionFilter::new().with_sensor_type(SensorType::Resource);

        let obs = Observation::new("test", ObservationValue::Null, "sensor");
        assert!(filter.matches(&obs, SensorType::Resource));
        assert!(!filter.matches(&obs, SensorType::Network));
    }

    #[test]
    fn test_perception_filter_observable_prefix() {
        let filter = PerceptionFilter::new().with_observable_prefix("cpu.");

        let obs1 = Observation::new("cpu.usage", ObservationValue::Null, "sensor");
        let obs2 = Observation::new("memory.usage", ObservationValue::Null, "sensor");

        assert!(filter.matches(&obs1, SensorType::Custom));
        assert!(!filter.matches(&obs2, SensorType::Custom));
    }

    #[test]
    fn test_perception_filter_tags() {
        let filter = PerceptionFilter::new().with_tag("important");

        let obs1 = Observation::new("test", ObservationValue::Null, "sensor").with_tag("important");
        let obs2 = Observation::new("test", ObservationValue::Null, "sensor");

        assert!(filter.matches(&obs1, SensorType::Custom));
        assert!(!filter.matches(&obs2, SensorType::Custom));
    }

    #[test]
    fn test_world_model_update() {
        let mut wm = WorldModel::new();
        let obs = Observation::new("test", ObservationValue::Integer(42), "sensor");

        wm.update(obs);

        assert_eq!(wm.len(), 1);
        assert!(wm.get("test").is_some());
        assert_eq!(wm.get_value("test").and_then(|v| v.as_int()), Some(42));
    }

    #[test]
    fn test_world_model_history() {
        let mut wm = WorldModel::new().with_max_history(10);

        for i in 0..5 {
            wm.update(Observation::new(
                "counter",
                ObservationValue::Integer(i),
                "sensor",
            ));
        }

        let history = wm.history_for("counter");
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn test_perception_system_register() {
        let mut system = PerceptionSystem::new();

        let def = SensorDefinition::new("test", "Test", SensorType::Custom);
        let sensor = Sensor::new(def, |_| {
            Ok(Observation::new("x", ObservationValue::Null, "x"))
        });

        system.register_sensor(sensor).unwrap();
        assert_eq!(system.sensor_count(), 1);
    }

    #[test]
    fn test_perception_system_duplicate() {
        let mut system = PerceptionSystem::new();

        let def1 = SensorDefinition::new("test", "Test 1", SensorType::Custom);
        let sensor1 = Sensor::new(def1, |_| {
            Ok(Observation::new("x", ObservationValue::Null, "x"))
        });

        let def2 = SensorDefinition::new("test", "Test 2", SensorType::Custom);
        let sensor2 = Sensor::new(def2, |_| {
            Ok(Observation::new("x", ObservationValue::Null, "x"))
        });

        system.register_sensor(sensor1).unwrap();
        assert!(system.register_sensor(sensor2).is_err());
    }

    #[test]
    fn test_perception_system_read() {
        let mut system = PerceptionSystem::new();

        let def = SensorDefinition::new("cpu", "CPU", SensorType::Resource);
        let sensor = Sensor::new(def, |name| {
            Ok(Observation::new(name, ObservationValue::Float(50.0), "cpu"))
        });

        system.register_sensor(sensor).unwrap();

        let obs = system.read("cpu", "cpu.usage").unwrap();
        assert_eq!(obs.name, "cpu.usage");

        // Check world model updated
        assert!(system.world_model().get("cpu.usage").is_some());
    }

    #[test]
    fn test_perception_system_subscriptions() {
        let mut system = PerceptionSystem::new();

        let filter = PerceptionFilter::new().with_sensor_type(SensorType::Resource);
        system.subscribe("sub-1", filter);

        system.unsubscribe("sub-1");
    }

    #[test]
    fn test_perception_system_enable_disable() {
        let mut system = PerceptionSystem::new();

        let def = SensorDefinition::new("test", "Test", SensorType::Custom);
        let sensor = Sensor::new(def, |_| {
            Ok(Observation::new("x", ObservationValue::Null, "x"))
        });

        system.register_sensor(sensor).unwrap();

        system.disable_sensor("test").unwrap();
        assert!(!system.get_sensor("test").unwrap().definition.config.enabled);

        system.enable_sensor("test").unwrap();
        assert!(system.get_sensor("test").unwrap().definition.config.enabled);
    }

    #[test]
    fn test_shared_perception() {
        let system = PerceptionSystem::new();
        let shared = SharedPerception::new(system);

        let def = SensorDefinition::new("test", "Test", SensorType::Custom);
        let sensor = Sensor::new(def, |name| {
            Ok(Observation::new(
                name,
                ObservationValue::Integer(100),
                "test",
            ))
        });

        shared.register_sensor(sensor).unwrap();
        assert_eq!(shared.sensor_count(), 1);

        shared.read("test", "value").unwrap();
        assert_eq!(
            shared.get_value("value").and_then(|v| v.as_int()),
            Some(100)
        );
    }

    #[test]
    fn test_perception_error_display() {
        let err = PerceptionError::SensorNotFound("test".to_string());
        assert!(err.to_string().contains("not found"));

        let err = PerceptionError::Timeout(Duration::from_secs(5));
        assert!(err.to_string().contains("timeout"));
    }
}
