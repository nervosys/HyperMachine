//! Touchscreen Device Emulation
//!
//! This module provides touchscreen device emulation with support for
//! single and multi-touch, pressure sensitivity, and gesture recognition.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Touch point state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TouchState {
    /// No touch
    #[default]
    Up,
    /// Touch down
    Down,
    /// Touch moved
    Move,
}

/// Touch point
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    /// Touch ID
    pub id: u32,
    /// X coordinate (0.0 - 1.0)
    pub x: f32,
    /// Y coordinate (0.0 - 1.0)
    pub y: f32,
    /// Pressure (0.0 - 1.0)
    pub pressure: f32,
    /// Touch state
    pub state: TouchState,
    /// Major axis (for elliptical touch)
    pub major: f32,
    /// Minor axis (for elliptical touch)
    pub minor: f32,
    /// Orientation (radians)
    pub orientation: f32,
}

impl Default for TouchPoint {
    fn default() -> Self {
        Self {
            id: 0,
            x: 0.0,
            y: 0.0,
            pressure: 1.0,
            state: TouchState::Up,
            major: 0.01,
            minor: 0.01,
            orientation: 0.0,
        }
    }
}

impl TouchPoint {
    /// Create new touch point
    pub fn new(id: u32, x: f32, y: f32) -> Self {
        Self {
            id,
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Set pressure
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = pressure.clamp(0.0, 1.0);
        self
    }

    /// Set size
    pub fn with_size(mut self, major: f32, minor: f32) -> Self {
        self.major = major.max(0.0);
        self.minor = minor.max(0.0);
        self
    }

    /// Distance to another point
    pub fn distance_to(&self, other: &TouchPoint) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Gesture type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    /// Single tap
    Tap,
    /// Double tap
    DoubleTap,
    /// Long press
    LongPress,
    /// Swipe left
    SwipeLeft,
    /// Swipe right
    SwipeRight,
    /// Swipe up
    SwipeUp,
    /// Swipe down
    SwipeDown,
    /// Pinch (zoom out)
    Pinch,
    /// Spread (zoom in)
    Spread,
    /// Rotate
    Rotate,
    /// Two-finger tap
    TwoFingerTap,
}

/// Gesture event
#[derive(Debug, Clone)]
pub struct Gesture {
    /// Gesture type
    pub gesture_type: GestureType,
    /// Center X
    pub x: f32,
    /// Center Y
    pub y: f32,
    /// Scale factor (for pinch/spread)
    pub scale: f32,
    /// Rotation angle (for rotate)
    pub rotation: f32,
    /// Velocity (for swipes)
    pub velocity: f32,
}

impl Gesture {
    /// Create new gesture
    pub fn new(gesture_type: GestureType, x: f32, y: f32) -> Self {
        Self {
            gesture_type,
            x,
            y,
            scale: 1.0,
            rotation: 0.0,
            velocity: 0.0,
        }
    }

    /// Create pinch gesture
    pub fn pinch(x: f32, y: f32, scale: f32) -> Self {
        Self {
            gesture_type: if scale < 1.0 {
                GestureType::Pinch
            } else {
                GestureType::Spread
            },
            x,
            y,
            scale,
            rotation: 0.0,
            velocity: 0.0,
        }
    }

    /// Create rotate gesture
    pub fn rotate(x: f32, y: f32, rotation: f32) -> Self {
        Self {
            gesture_type: GestureType::Rotate,
            x,
            y,
            scale: 1.0,
            rotation,
            velocity: 0.0,
        }
    }
}

/// Touch event for reporting
#[derive(Debug, Clone)]
pub struct TouchEvent {
    /// Touch points
    pub points: Vec<TouchPoint>,
    /// Timestamp
    pub timestamp: u64,
}

impl TouchEvent {
    /// Create new event
    pub fn new(points: Vec<TouchPoint>, timestamp: u64) -> Self {
        Self { points, timestamp }
    }

    /// Get active touch count
    pub fn active_count(&self) -> usize {
        self.points
            .iter()
            .filter(|p| p.state != TouchState::Up)
            .count()
    }
}

/// Touchscreen configuration
#[derive(Debug, Clone)]
pub struct TouchConfig {
    /// Screen width
    pub width: u32,
    /// Screen height
    pub height: u32,
    /// Max touch points supported
    pub max_touches: u32,
    /// Pressure supported
    pub pressure_supported: bool,
    /// Multi-touch supported
    pub multitouch_supported: bool,
    /// Gesture recognition enabled
    pub gestures_enabled: bool,
    /// Long press duration
    pub long_press_duration: Duration,
    /// Double tap interval
    pub double_tap_interval: Duration,
    /// Swipe threshold (normalized)
    pub swipe_threshold: f32,
}

impl Default for TouchConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            max_touches: 10,
            pressure_supported: true,
            multitouch_supported: true,
            gestures_enabled: true,
            long_press_duration: Duration::from_millis(500),
            double_tap_interval: Duration::from_millis(300),
            swipe_threshold: 0.1,
        }
    }
}

impl TouchConfig {
    /// Create new config
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    /// Convert normalized coordinates to pixels
    pub fn to_pixels(&self, x: f32, y: f32) -> (u32, u32) {
        let px = (x * self.width as f32) as u32;
        let py = (y * self.height as f32) as u32;
        (px.min(self.width - 1), py.min(self.height - 1))
    }

    /// Convert pixel coordinates to normalized
    pub fn to_normalized(&self, x: u32, y: u32) -> (f32, f32) {
        let nx = x as f32 / self.width as f32;
        let ny = y as f32 / self.height as f32;
        (nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0))
    }
}

/// Touch tracking state
#[derive(Debug)]
struct TouchTrack {
    /// Start point
    start: TouchPoint,
    /// Current point
    current: TouchPoint,
    /// Start time
    start_time: Instant,
    /// Last tap time (for double-tap)
    last_tap: Option<Instant>,
}

/// Touchscreen statistics
#[derive(Debug, Default)]
pub struct TouchStats {
    /// Total touch events
    events: AtomicU64,
    /// Touch down events
    touch_downs: AtomicU64,
    /// Touch up events
    touch_ups: AtomicU64,
    /// Touch move events
    touch_moves: AtomicU64,
    /// Gestures detected
    gestures_detected: AtomicU64,
}

impl TouchStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record event
    pub fn record_event(&self) {
        self.events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record touch down
    pub fn record_touch_down(&self) {
        self.touch_downs.fetch_add(1, Ordering::Relaxed);
    }

    /// Record touch up
    pub fn record_touch_up(&self) {
        self.touch_ups.fetch_add(1, Ordering::Relaxed);
    }

    /// Record touch move
    pub fn record_touch_move(&self) {
        self.touch_moves.fetch_add(1, Ordering::Relaxed);
    }

    /// Record gesture
    pub fn record_gesture(&self) {
        self.gestures_detected.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> TouchStatsSnapshot {
        TouchStatsSnapshot {
            events: self.events.load(Ordering::Relaxed),
            touch_downs: self.touch_downs.load(Ordering::Relaxed),
            touch_ups: self.touch_ups.load(Ordering::Relaxed),
            touch_moves: self.touch_moves.load(Ordering::Relaxed),
            gestures_detected: self.gestures_detected.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct TouchStatsSnapshot {
    /// Total events
    pub events: u64,
    /// Touch down events
    pub touch_downs: u64,
    /// Touch up events
    pub touch_ups: u64,
    /// Touch move events
    pub touch_moves: u64,
    /// Gestures detected
    pub gestures_detected: u64,
}

/// Touchscreen device
pub struct Touchscreen {
    /// Configuration
    config: TouchConfig,
    /// Active touch points
    active_touches: HashMap<u32, TouchTrack>,
    /// Pending events
    pending_events: Vec<TouchEvent>,
    /// Pending gestures
    pending_gestures: Vec<Gesture>,
    /// Next touch ID
    next_id: u32,
    /// Timestamp counter
    timestamp: u64,
    /// Statistics
    stats: TouchStats,
    /// Initial distance for pinch
    initial_distance: Option<f32>,
    /// Initial angle for rotate
    initial_angle: Option<f32>,
}

impl Default for Touchscreen {
    fn default() -> Self {
        Self::new(TouchConfig::default())
    }
}

impl Touchscreen {
    /// Create new touchscreen
    pub fn new(config: TouchConfig) -> Self {
        Self {
            config,
            active_touches: HashMap::new(),
            pending_events: Vec::new(),
            pending_gestures: Vec::new(),
            next_id: 1,
            timestamp: 0,
            stats: TouchStats::new(),
            initial_distance: None,
            initial_angle: None,
        }
    }

    /// Get configuration
    pub fn config(&self) -> &TouchConfig {
        &self.config
    }

    /// Get active touch count
    pub fn active_touch_count(&self) -> usize {
        self.active_touches.len()
    }

    /// Get statistics
    pub fn stats(&self) -> &TouchStats {
        &self.stats
    }

    /// Check for pending events
    pub fn has_pending_events(&self) -> bool {
        !self.pending_events.is_empty()
    }

    /// Get next pending event
    pub fn next_event(&mut self) -> Option<TouchEvent> {
        self.pending_events.pop()
    }

    /// Check for pending gestures
    pub fn has_pending_gestures(&self) -> bool {
        !self.pending_gestures.is_empty()
    }

    /// Get next pending gesture
    pub fn next_gesture(&mut self) -> Option<Gesture> {
        self.pending_gestures.pop()
    }

    /// Touch down at normalized coordinates
    pub fn touch_down(&mut self, x: f32, y: f32) -> u32 {
        self.touch_down_with_pressure(x, y, 1.0)
    }

    /// Touch down with pressure
    pub fn touch_down_with_pressure(&mut self, x: f32, y: f32, pressure: f32) -> u32 {
        if self.active_touches.len() >= self.config.max_touches as usize {
            return 0;
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }

        let point = TouchPoint::new(id, x, y).with_pressure(pressure);
        let mut point_down = point;
        point_down.state = TouchState::Down;

        let track = TouchTrack {
            start: point,
            current: point_down,
            start_time: Instant::now(),
            last_tap: None,
        };

        self.active_touches.insert(id, track);
        self.stats.record_touch_down();
        self.emit_event();

        id
    }

    /// Touch move
    pub fn touch_move(&mut self, id: u32, x: f32, y: f32) {
        self.touch_move_with_pressure(id, x, y, 1.0);
    }

    /// Touch move with pressure
    pub fn touch_move_with_pressure(&mut self, id: u32, x: f32, y: f32, pressure: f32) {
        if let Some(track) = self.active_touches.get_mut(&id) {
            track.current = TouchPoint::new(id, x, y).with_pressure(pressure);
            track.current.state = TouchState::Move;
            self.stats.record_touch_move();
            self.emit_event();
            self.detect_multi_touch_gestures();
        }
    }

    /// Touch up
    pub fn touch_up(&mut self, id: u32) {
        if let Some(track) = self.active_touches.remove(&id) {
            self.stats.record_touch_up();

            // Detect gestures
            if self.config.gestures_enabled {
                self.detect_tap_gesture(&track);
                self.detect_swipe_gesture(&track);
            }

            // Emit up event
            let mut point = track.current;
            point.state = TouchState::Up;

            let event = TouchEvent::new(vec![point], self.timestamp);
            self.pending_events.push(event);
            self.timestamp += 1;
            self.stats.record_event();

            // Reset multi-touch gesture tracking
            if self.active_touches.is_empty() {
                self.initial_distance = None;
                self.initial_angle = None;
            }
        }
    }

    /// Emit current touch state as event
    fn emit_event(&mut self) {
        let points: Vec<TouchPoint> = self.active_touches.values().map(|t| t.current).collect();

        if !points.is_empty() {
            let event = TouchEvent::new(points, self.timestamp);
            self.pending_events.push(event);
            self.timestamp += 1;
            self.stats.record_event();
        }
    }

    /// Detect tap gesture
    fn detect_tap_gesture(&mut self, track: &TouchTrack) {
        let duration = track.start_time.elapsed();
        let distance = track.start.distance_to(&track.current);

        // Check if it's a tap (short duration, minimal movement)
        if duration < self.config.long_press_duration && distance < 0.02 {
            let gesture = Gesture::new(GestureType::Tap, track.current.x, track.current.y);
            self.pending_gestures.push(gesture);
            self.stats.record_gesture();
        } else if duration >= self.config.long_press_duration && distance < 0.02 {
            let gesture = Gesture::new(GestureType::LongPress, track.current.x, track.current.y);
            self.pending_gestures.push(gesture);
            self.stats.record_gesture();
        }
    }

    /// Detect swipe gesture
    fn detect_swipe_gesture(&mut self, track: &TouchTrack) {
        let dx = track.current.x - track.start.x;
        let dy = track.current.y - track.start.y;
        let distance = (dx * dx + dy * dy).sqrt();

        if distance >= self.config.swipe_threshold {
            let gesture_type = if dx.abs() > dy.abs() {
                if dx > 0.0 {
                    GestureType::SwipeRight
                } else {
                    GestureType::SwipeLeft
                }
            } else if dy > 0.0 {
                GestureType::SwipeDown
            } else {
                GestureType::SwipeUp
            };

            let duration = track.start_time.elapsed().as_secs_f32();
            let velocity = if duration > 0.0 {
                distance / duration
            } else {
                0.0
            };

            let mut gesture = Gesture::new(gesture_type, track.current.x, track.current.y);
            gesture.velocity = velocity;
            self.pending_gestures.push(gesture);
            self.stats.record_gesture();
        }
    }

    /// Detect multi-touch gestures (pinch/rotate)
    fn detect_multi_touch_gestures(&mut self) {
        if !self.config.gestures_enabled || self.active_touches.len() != 2 {
            return;
        }

        let points: Vec<&TouchTrack> = self.active_touches.values().collect();
        let p1 = &points[0].current;
        let p2 = &points[1].current;

        let current_distance = p1.distance_to(p2);
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let current_angle = dy.atan2(dx);

        let center_x = (p1.x + p2.x) / 2.0;
        let center_y = (p1.y + p2.y) / 2.0;

        // Initialize or detect pinch
        if let Some(initial) = self.initial_distance {
            if initial > 0.0 {
                let scale = current_distance / initial;
                if (scale - 1.0).abs() > 0.1 {
                    let gesture = Gesture::pinch(center_x, center_y, scale);
                    self.pending_gestures.push(gesture);
                    self.stats.record_gesture();
                }
            }
        } else {
            self.initial_distance = Some(current_distance);
        }

        // Initialize or detect rotation
        if let Some(initial) = self.initial_angle {
            let rotation = current_angle - initial;
            if rotation.abs() > 0.1 {
                let gesture = Gesture::rotate(center_x, center_y, rotation);
                self.pending_gestures.push(gesture);
                self.stats.record_gesture();
            }
        } else {
            self.initial_angle = Some(current_angle);
        }
    }

    /// Simulate tap at position
    pub fn tap(&mut self, x: f32, y: f32) {
        let id = self.touch_down(x, y);
        self.touch_up(id);
    }

    /// Simulate double tap
    pub fn double_tap(&mut self, x: f32, y: f32) {
        self.tap(x, y);
        self.tap(x, y);

        // Generate double tap gesture
        if self.config.gestures_enabled {
            let gesture = Gesture::new(GestureType::DoubleTap, x, y);
            self.pending_gestures.push(gesture);
            self.stats.record_gesture();
        }
    }

    /// Simulate swipe
    pub fn swipe(&mut self, start_x: f32, start_y: f32, end_x: f32, end_y: f32, steps: u32) {
        let id = self.touch_down(start_x, start_y);

        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = start_x + (end_x - start_x) * t;
            let y = start_y + (end_y - start_y) * t;
            self.touch_move(id, x, y);
        }

        self.touch_up(id);
    }

    /// Simulate pinch
    pub fn pinch(&mut self, center_x: f32, center_y: f32, start_distance: f32, end_distance: f32) {
        let id1 = self.touch_down(center_x - start_distance / 2.0, center_y);
        let id2 = self.touch_down(center_x + start_distance / 2.0, center_y);

        // Move to end distance
        let steps = 10;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let dist = start_distance + (end_distance - start_distance) * t;
            self.touch_move(id1, center_x - dist / 2.0, center_y);
            self.touch_move(id2, center_x + dist / 2.0, center_y);
        }

        self.touch_up(id1);
        self.touch_up(id2);
    }

    /// Reset touchscreen
    pub fn reset(&mut self) {
        self.active_touches.clear();
        self.pending_events.clear();
        self.pending_gestures.clear();
        self.initial_distance = None;
        self.initial_angle = None;
    }

    /// Get all active touch points
    pub fn get_active_points(&self) -> Vec<TouchPoint> {
        self.active_touches.values().map(|t| t.current).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_point() {
        let point = TouchPoint::new(1, 0.5, 0.5);
        assert_eq!(point.id, 1);
        assert_eq!(point.x, 0.5);
        assert_eq!(point.y, 0.5);
        assert_eq!(point.pressure, 1.0);
    }

    #[test]
    fn test_touch_point_clamping() {
        let point = TouchPoint::new(1, 1.5, -0.5);
        assert_eq!(point.x, 1.0);
        assert_eq!(point.y, 0.0);
    }

    #[test]
    fn test_touch_point_distance() {
        let p1 = TouchPoint::new(1, 0.0, 0.0);
        let p2 = TouchPoint::new(2, 0.3, 0.4);
        assert!((p1.distance_to(&p2) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_touch_config() {
        let config = TouchConfig::new(1920, 1080);
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);

        let (px, py) = config.to_pixels(0.5, 0.5);
        assert_eq!(px, 960);
        assert_eq!(py, 540);

        let (nx, ny) = config.to_normalized(960, 540);
        assert!((nx - 0.5).abs() < 0.001);
        assert!((ny - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_touchscreen_creation() {
        let ts = Touchscreen::default();
        assert_eq!(ts.active_touch_count(), 0);
        assert!(!ts.has_pending_events());
    }

    #[test]
    fn test_touch_down_up() {
        let mut ts = Touchscreen::default();

        let id = ts.touch_down(0.5, 0.5);
        assert!(id > 0);
        assert_eq!(ts.active_touch_count(), 1);

        ts.touch_up(id);
        assert_eq!(ts.active_touch_count(), 0);
    }

    #[test]
    fn test_touch_events() {
        let mut ts = Touchscreen::default();

        let id = ts.touch_down(0.5, 0.5);
        assert!(ts.has_pending_events());

        let event = ts.next_event().unwrap();
        assert_eq!(event.active_count(), 1);
        assert_eq!(event.points[0].state, TouchState::Down);

        ts.touch_move(id, 0.6, 0.5);
        let event = ts.next_event().unwrap();
        assert_eq!(event.points[0].state, TouchState::Move);

        ts.touch_up(id);
        let event = ts.next_event().unwrap();
        assert_eq!(event.points[0].state, TouchState::Up);
    }

    #[test]
    fn test_multi_touch() {
        let mut ts = Touchscreen::default();

        let id1 = ts.touch_down(0.3, 0.5);
        let id2 = ts.touch_down(0.7, 0.5);

        assert_eq!(ts.active_touch_count(), 2);

        let points = ts.get_active_points();
        assert_eq!(points.len(), 2);

        ts.touch_up(id1);
        ts.touch_up(id2);

        assert_eq!(ts.active_touch_count(), 0);
    }

    #[test]
    fn test_tap_gesture() {
        let mut ts = Touchscreen::default();

        ts.tap(0.5, 0.5);

        assert!(ts.has_pending_gestures());
        let gesture = ts.next_gesture().unwrap();
        assert_eq!(gesture.gesture_type, GestureType::Tap);
    }

    #[test]
    fn test_double_tap_gesture() {
        let mut ts = Touchscreen::default();

        ts.double_tap(0.5, 0.5);

        // Should have tap gestures and double tap
        let mut has_double_tap = false;
        while let Some(gesture) = ts.next_gesture() {
            if gesture.gesture_type == GestureType::DoubleTap {
                has_double_tap = true;
            }
        }
        assert!(has_double_tap);
    }

    #[test]
    fn test_swipe_gesture() {
        let mut ts = Touchscreen::default();

        ts.swipe(0.1, 0.5, 0.9, 0.5, 10);

        let mut has_swipe = false;
        while let Some(gesture) = ts.next_gesture() {
            if gesture.gesture_type == GestureType::SwipeRight {
                has_swipe = true;
                assert!(gesture.velocity > 0.0);
            }
        }
        assert!(has_swipe);
    }

    #[test]
    fn test_swipe_directions() {
        let mut ts = Touchscreen::default();

        // Swipe left
        ts.swipe(0.9, 0.5, 0.1, 0.5, 10);
        let mut found = false;
        while let Some(g) = ts.next_gesture() {
            if g.gesture_type == GestureType::SwipeLeft {
                found = true;
            }
        }
        assert!(found);

        // Swipe up
        ts.swipe(0.5, 0.9, 0.5, 0.1, 10);
        let mut found = false;
        while let Some(g) = ts.next_gesture() {
            if g.gesture_type == GestureType::SwipeUp {
                found = true;
            }
        }
        assert!(found);

        // Swipe down
        ts.swipe(0.5, 0.1, 0.5, 0.9, 10);
        let mut found = false;
        while let Some(g) = ts.next_gesture() {
            if g.gesture_type == GestureType::SwipeDown {
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn test_max_touches() {
        let config = TouchConfig {
            max_touches: 2,
            ..Default::default()
        };
        let mut ts = Touchscreen::new(config);

        ts.touch_down(0.1, 0.5);
        ts.touch_down(0.5, 0.5);
        let id3 = ts.touch_down(0.9, 0.5);

        assert_eq!(id3, 0); // Should fail
        assert_eq!(ts.active_touch_count(), 2);
    }

    #[test]
    fn test_touch_with_pressure() {
        let mut ts = Touchscreen::default();

        let id = ts.touch_down_with_pressure(0.5, 0.5, 0.7);
        let points = ts.get_active_points();

        assert_eq!(points[0].pressure, 0.7);
        ts.touch_up(id);
    }

    #[test]
    fn test_touch_stats() {
        let mut ts = Touchscreen::default();

        let id = ts.touch_down(0.5, 0.5);
        ts.touch_move(id, 0.6, 0.5);
        ts.touch_up(id);

        let stats = ts.stats().snapshot();
        assert!(stats.touch_downs > 0);
        assert!(stats.touch_moves > 0);
        assert!(stats.touch_ups > 0);
    }

    #[test]
    fn test_touchscreen_reset() {
        let mut ts = Touchscreen::default();

        ts.touch_down(0.5, 0.5);
        ts.touch_down(0.6, 0.6);

        ts.reset();

        assert_eq!(ts.active_touch_count(), 0);
        assert!(!ts.has_pending_events());
        assert!(!ts.has_pending_gestures());
    }

    #[test]
    fn test_gesture_creation() {
        let gesture = Gesture::pinch(0.5, 0.5, 0.5);
        assert_eq!(gesture.gesture_type, GestureType::Pinch);
        assert_eq!(gesture.scale, 0.5);

        let gesture = Gesture::rotate(0.5, 0.5, 1.57);
        assert_eq!(gesture.gesture_type, GestureType::Rotate);
        assert!((gesture.rotation - 1.57).abs() < 0.01);
    }

    #[test]
    fn test_touch_id_wraparound() {
        let mut ts = Touchscreen::default();
        ts.next_id = u32::MAX;

        let id1 = ts.touch_down(0.5, 0.5);
        ts.touch_up(id1);

        let id2 = ts.touch_down(0.5, 0.5);
        assert!(id2 > 0);
        ts.touch_up(id2);
    }

    #[test]
    fn test_pinch_simulation() {
        let mut ts = Touchscreen::default();

        ts.pinch(0.5, 0.5, 0.4, 0.2);

        // Should detect pinch gesture
        let mut has_pinch = false;
        while let Some(gesture) = ts.next_gesture() {
            if gesture.gesture_type == GestureType::Pinch {
                has_pinch = true;
                assert!(gesture.scale < 1.0);
            }
        }
        assert!(has_pinch);
    }
}
