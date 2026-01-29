//! Game Controller Support
//!
//! This module provides game controller/gamepad emulation with support
//! for buttons, analog sticks, triggers, and force feedback.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Controller type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControllerType {
    /// Generic controller
    #[default]
    Generic,
    /// Xbox-style controller
    Xbox,
    /// PlayStation-style controller
    PlayStation,
    /// Nintendo-style controller
    Nintendo,
}

impl ControllerType {
    /// Get controller name
    pub fn name(&self) -> &'static str {
        match self {
            ControllerType::Generic => "Generic Gamepad",
            ControllerType::Xbox => "Xbox Controller",
            ControllerType::PlayStation => "PlayStation Controller",
            ControllerType::Nintendo => "Nintendo Controller",
        }
    }

    /// Get button count
    pub fn button_count(&self) -> u8 {
        match self {
            ControllerType::Generic => 16,
            ControllerType::Xbox => 17,
            ControllerType::PlayStation => 18,
            ControllerType::Nintendo => 16,
        }
    }

    /// Get axis count
    pub fn axis_count(&self) -> u8 {
        match self {
            ControllerType::Generic => 6,
            ControllerType::Xbox => 6,
            ControllerType::PlayStation => 6,
            ControllerType::Nintendo => 6,
        }
    }
}

/// Standard button mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    /// A / Cross
    South = 0,
    /// B / Circle
    East = 1,
    /// X / Square
    West = 2,
    /// Y / Triangle
    North = 3,
    /// Left bumper / L1
    LeftBumper = 4,
    /// Right bumper / R1
    RightBumper = 5,
    /// Select / Back / Share
    Select = 6,
    /// Start / Options
    Start = 7,
    /// Left stick press / L3
    LeftStick = 8,
    /// Right stick press / R3
    RightStick = 9,
    /// D-pad up
    DpadUp = 10,
    /// D-pad down
    DpadDown = 11,
    /// D-pad left
    DpadLeft = 12,
    /// D-pad right
    DpadRight = 13,
    /// Guide / Home / PS
    Guide = 14,
    /// Misc button
    Misc = 15,
}

impl Button {
    /// Get button name
    pub fn name(&self) -> &'static str {
        match self {
            Button::South => "South (A/Cross)",
            Button::East => "East (B/Circle)",
            Button::West => "West (X/Square)",
            Button::North => "North (Y/Triangle)",
            Button::LeftBumper => "Left Bumper (LB/L1)",
            Button::RightBumper => "Right Bumper (RB/R1)",
            Button::Select => "Select/Back/Share",
            Button::Start => "Start/Options",
            Button::LeftStick => "Left Stick (L3)",
            Button::RightStick => "Right Stick (R3)",
            Button::DpadUp => "D-Pad Up",
            Button::DpadDown => "D-Pad Down",
            Button::DpadLeft => "D-Pad Left",
            Button::DpadRight => "D-Pad Right",
            Button::Guide => "Guide/Home/PS",
            Button::Misc => "Misc",
        }
    }

    /// Get button index
    pub fn index(&self) -> u8 {
        *self as u8
    }

    /// Create from index
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Button::South),
            1 => Some(Button::East),
            2 => Some(Button::West),
            3 => Some(Button::North),
            4 => Some(Button::LeftBumper),
            5 => Some(Button::RightBumper),
            6 => Some(Button::Select),
            7 => Some(Button::Start),
            8 => Some(Button::LeftStick),
            9 => Some(Button::RightStick),
            10 => Some(Button::DpadUp),
            11 => Some(Button::DpadDown),
            12 => Some(Button::DpadLeft),
            13 => Some(Button::DpadRight),
            14 => Some(Button::Guide),
            15 => Some(Button::Misc),
            _ => None,
        }
    }
}

/// Axis type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Axis {
    /// Left stick X (-1.0 to 1.0)
    LeftStickX = 0,
    /// Left stick Y (-1.0 to 1.0)
    LeftStickY = 1,
    /// Right stick X (-1.0 to 1.0)
    RightStickX = 2,
    /// Right stick Y (-1.0 to 1.0)
    RightStickY = 3,
    /// Left trigger (0.0 to 1.0)
    LeftTrigger = 4,
    /// Right trigger (0.0 to 1.0)
    RightTrigger = 5,
}

impl Axis {
    /// Get axis name
    pub fn name(&self) -> &'static str {
        match self {
            Axis::LeftStickX => "Left Stick X",
            Axis::LeftStickY => "Left Stick Y",
            Axis::RightStickX => "Right Stick X",
            Axis::RightStickY => "Right Stick Y",
            Axis::LeftTrigger => "Left Trigger",
            Axis::RightTrigger => "Right Trigger",
        }
    }

    /// Get axis index
    pub fn index(&self) -> u8 {
        *self as u8
    }

    /// Create from index
    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Axis::LeftStickX),
            1 => Some(Axis::LeftStickY),
            2 => Some(Axis::RightStickX),
            3 => Some(Axis::RightStickY),
            4 => Some(Axis::LeftTrigger),
            5 => Some(Axis::RightTrigger),
            _ => None,
        }
    }

    /// Check if this is a trigger
    pub fn is_trigger(&self) -> bool {
        matches!(self, Axis::LeftTrigger | Axis::RightTrigger)
    }

    /// Get default value (0 for sticks, 0 for triggers)
    pub fn default_value(&self) -> f32 {
        0.0
    }
}

/// Rumble/vibration effect
#[derive(Debug, Clone, Copy, Default)]
pub struct RumbleEffect {
    /// Strong motor intensity (0.0 to 1.0)
    pub strong: f32,
    /// Weak motor intensity (0.0 to 1.0)
    pub weak: f32,
    /// Duration
    pub duration: Duration,
}

impl RumbleEffect {
    /// Create new effect
    pub fn new(strong: f32, weak: f32, duration: Duration) -> Self {
        Self {
            strong: strong.clamp(0.0, 1.0),
            weak: weak.clamp(0.0, 1.0),
            duration,
        }
    }

    /// Create strong rumble
    pub fn strong(intensity: f32, duration: Duration) -> Self {
        Self::new(intensity, 0.0, duration)
    }

    /// Create weak rumble
    pub fn weak(intensity: f32, duration: Duration) -> Self {
        Self::new(0.0, intensity, duration)
    }

    /// Check if effect is active
    pub fn is_active(&self) -> bool {
        (self.strong > 0.0 || self.weak > 0.0) && !self.duration.is_zero()
    }
}

/// Controller state snapshot
#[derive(Debug, Clone)]
pub struct ControllerState {
    /// Button states (bit field)
    pub buttons: u32,
    /// Axis values
    pub axes: [f32; 6],
    /// Timestamp
    pub timestamp: u64,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            buttons: 0,
            axes: [0.0; 6],
            timestamp: 0,
        }
    }
}

impl ControllerState {
    /// Check if button is pressed
    pub fn is_pressed(&self, button: Button) -> bool {
        self.buttons & (1 << button.index()) != 0
    }

    /// Get axis value
    pub fn get_axis(&self, axis: Axis) -> f32 {
        self.axes[axis.index() as usize]
    }

    /// Get left stick as vector
    pub fn left_stick(&self) -> (f32, f32) {
        (
            self.axes[Axis::LeftStickX.index() as usize],
            self.axes[Axis::LeftStickY.index() as usize],
        )
    }

    /// Get right stick as vector
    pub fn right_stick(&self) -> (f32, f32) {
        (
            self.axes[Axis::RightStickX.index() as usize],
            self.axes[Axis::RightStickY.index() as usize],
        )
    }

    /// Get triggers
    pub fn triggers(&self) -> (f32, f32) {
        (
            self.axes[Axis::LeftTrigger.index() as usize],
            self.axes[Axis::RightTrigger.index() as usize],
        )
    }
}

/// Controller event
#[derive(Debug, Clone)]
pub enum ControllerEvent {
    /// Button pressed
    ButtonPressed(Button),
    /// Button released
    ButtonReleased(Button),
    /// Axis changed
    AxisChanged(Axis, f32),
    /// Connected
    Connected,
    /// Disconnected
    Disconnected,
}

/// Controller statistics
#[derive(Debug, Default)]
pub struct ControllerStats {
    /// Button presses
    button_presses: AtomicU64,
    /// Button releases
    button_releases: AtomicU64,
    /// Axis updates
    axis_updates: AtomicU64,
    /// Rumble effects played
    rumble_effects: AtomicU64,
    /// Total input events
    total_events: AtomicU64,
}

impl ControllerStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record button press
    pub fn record_press(&self) {
        self.button_presses.fetch_add(1, Ordering::Relaxed);
        self.total_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record button release
    pub fn record_release(&self) {
        self.button_releases.fetch_add(1, Ordering::Relaxed);
        self.total_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record axis update
    pub fn record_axis(&self) {
        self.axis_updates.fetch_add(1, Ordering::Relaxed);
        self.total_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record rumble effect
    pub fn record_rumble(&self) {
        self.rumble_effects.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> ControllerStatsSnapshot {
        ControllerStatsSnapshot {
            button_presses: self.button_presses.load(Ordering::Relaxed),
            button_releases: self.button_releases.load(Ordering::Relaxed),
            axis_updates: self.axis_updates.load(Ordering::Relaxed),
            rumble_effects: self.rumble_effects.load(Ordering::Relaxed),
            total_events: self.total_events.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct ControllerStatsSnapshot {
    /// Button presses
    pub button_presses: u64,
    /// Button releases
    pub button_releases: u64,
    /// Axis updates
    pub axis_updates: u64,
    /// Rumble effects
    pub rumble_effects: u64,
    /// Total events
    pub total_events: u64,
}

/// Deadzone configuration
#[derive(Debug, Clone, Copy)]
pub struct DeadzoneConfig {
    /// Inner deadzone (values below this are treated as 0)
    pub inner: f32,
    /// Outer deadzone (values above this are treated as 1)
    pub outer: f32,
}

impl Default for DeadzoneConfig {
    fn default() -> Self {
        Self {
            inner: 0.1,
            outer: 0.9,
        }
    }
}

impl DeadzoneConfig {
    /// Apply deadzone to value
    pub fn apply(&self, value: f32) -> f32 {
        let abs_value = value.abs();

        if abs_value < self.inner {
            return 0.0;
        }

        if abs_value > self.outer {
            return value.signum();
        }

        // Scale the value
        let range = self.outer - self.inner;
        if range > 0.0 {
            let scaled = (abs_value - self.inner) / range;
            scaled * value.signum()
        } else {
            value
        }
    }
}

/// Game controller device
pub struct GameController {
    /// Controller type
    controller_type: ControllerType,
    /// Controller index
    index: u8,
    /// Connected state
    connected: bool,
    /// Button states
    buttons: u32,
    /// Axis values
    axes: [f32; 6],
    /// Deadzone config
    deadzone: DeadzoneConfig,
    /// Pending events
    events: VecDeque<ControllerEvent>,
    /// Active rumble effect
    rumble: Option<(RumbleEffect, Instant)>,
    /// Statistics
    stats: ControllerStats,
    /// Timestamp counter
    timestamp: u64,
}

impl Default for GameController {
    fn default() -> Self {
        Self::new(0, ControllerType::Generic)
    }
}

impl GameController {
    /// Create new controller
    pub fn new(index: u8, controller_type: ControllerType) -> Self {
        Self {
            controller_type,
            index,
            connected: true,
            buttons: 0,
            axes: [0.0; 6],
            deadzone: DeadzoneConfig::default(),
            events: VecDeque::with_capacity(32),
            rumble: None,
            stats: ControllerStats::new(),
            timestamp: 0,
        }
    }

    /// Get controller type
    pub fn controller_type(&self) -> ControllerType {
        self.controller_type
    }

    /// Get controller index
    pub fn index(&self) -> u8 {
        self.index
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Set connected state
    pub fn set_connected(&mut self, connected: bool) {
        if self.connected != connected {
            self.connected = connected;
            self.events.push_back(if connected {
                ControllerEvent::Connected
            } else {
                ControllerEvent::Disconnected
            });
        }
    }

    /// Get deadzone config
    pub fn deadzone(&self) -> &DeadzoneConfig {
        &self.deadzone
    }

    /// Set deadzone config
    pub fn set_deadzone(&mut self, deadzone: DeadzoneConfig) {
        self.deadzone = deadzone;
    }

    /// Get statistics
    pub fn stats(&self) -> &ControllerStats {
        &self.stats
    }

    /// Check if button is pressed
    pub fn is_pressed(&self, button: Button) -> bool {
        self.buttons & (1 << button.index()) != 0
    }

    /// Get axis value (with deadzone applied)
    pub fn get_axis(&self, axis: Axis) -> f32 {
        let raw = self.axes[axis.index() as usize];
        if axis.is_trigger() {
            raw // Triggers don't use deadzone
        } else {
            self.deadzone.apply(raw)
        }
    }

    /// Get raw axis value (without deadzone)
    pub fn get_axis_raw(&self, axis: Axis) -> f32 {
        self.axes[axis.index() as usize]
    }

    /// Get current state
    pub fn state(&self) -> ControllerState {
        ControllerState {
            buttons: self.buttons,
            axes: self.axes,
            timestamp: self.timestamp,
        }
    }

    /// Press button
    pub fn press_button(&mut self, button: Button) {
        let mask = 1 << button.index();
        if self.buttons & mask == 0 {
            self.buttons |= mask;
            self.events
                .push_back(ControllerEvent::ButtonPressed(button));
            self.stats.record_press();
            self.timestamp += 1;
        }
    }

    /// Release button
    pub fn release_button(&mut self, button: Button) {
        let mask = 1 << button.index();
        if self.buttons & mask != 0 {
            self.buttons &= !mask;
            self.events
                .push_back(ControllerEvent::ButtonReleased(button));
            self.stats.record_release();
            self.timestamp += 1;
        }
    }

    /// Set button state
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.press_button(button);
        } else {
            self.release_button(button);
        }
    }

    /// Set axis value
    pub fn set_axis(&mut self, axis: Axis, value: f32) {
        let clamped = if axis.is_trigger() {
            value.clamp(0.0, 1.0)
        } else {
            value.clamp(-1.0, 1.0)
        };

        let idx = axis.index() as usize;
        if (self.axes[idx] - clamped).abs() > 0.001 {
            self.axes[idx] = clamped;
            self.events
                .push_back(ControllerEvent::AxisChanged(axis, clamped));
            self.stats.record_axis();
            self.timestamp += 1;
        }
    }

    /// Set left stick
    pub fn set_left_stick(&mut self, x: f32, y: f32) {
        self.set_axis(Axis::LeftStickX, x);
        self.set_axis(Axis::LeftStickY, y);
    }

    /// Set right stick
    pub fn set_right_stick(&mut self, x: f32, y: f32) {
        self.set_axis(Axis::RightStickX, x);
        self.set_axis(Axis::RightStickY, y);
    }

    /// Set left trigger
    pub fn set_left_trigger(&mut self, value: f32) {
        self.set_axis(Axis::LeftTrigger, value);
    }

    /// Set right trigger
    pub fn set_right_trigger(&mut self, value: f32) {
        self.set_axis(Axis::RightTrigger, value);
    }

    /// Start rumble effect
    pub fn rumble(&mut self, effect: RumbleEffect) {
        if effect.is_active() {
            self.rumble = Some((effect, Instant::now()));
            self.stats.record_rumble();
        }
    }

    /// Stop rumble
    pub fn stop_rumble(&mut self) {
        self.rumble = None;
    }

    /// Get current rumble state
    pub fn get_rumble(&self) -> Option<(f32, f32)> {
        self.rumble.as_ref().and_then(|(effect, start)| {
            if start.elapsed() < effect.duration {
                Some((effect.strong, effect.weak))
            } else {
                None
            }
        })
    }

    /// Update rumble (call periodically)
    pub fn update_rumble(&mut self) {
        if let Some((effect, start)) = &self.rumble {
            if start.elapsed() >= effect.duration {
                self.rumble = None;
            }
        }
    }

    /// Poll next event
    pub fn poll_event(&mut self) -> Option<ControllerEvent> {
        self.events.pop_front()
    }

    /// Check if events pending
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// Reset controller state
    pub fn reset(&mut self) {
        self.buttons = 0;
        self.axes = [0.0; 6];
        self.events.clear();
        self.rumble = None;
        self.timestamp = 0;
    }

    /// Simulate button tap (press and release)
    pub fn tap_button(&mut self, button: Button) {
        self.press_button(button);
        self.release_button(button);
    }

    /// Simulate D-pad direction
    pub fn set_dpad(&mut self, up: bool, down: bool, left: bool, right: bool) {
        self.set_button(Button::DpadUp, up);
        self.set_button(Button::DpadDown, down);
        self.set_button(Button::DpadLeft, left);
        self.set_button(Button::DpadRight, right);
    }

    /// Get pressed buttons as list
    pub fn pressed_buttons(&self) -> Vec<Button> {
        let mut buttons = Vec::new();
        for i in 0..16 {
            if let Some(button) = Button::from_index(i) {
                if self.is_pressed(button) {
                    buttons.push(button);
                }
            }
        }
        buttons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_type() {
        assert_eq!(ControllerType::Xbox.button_count(), 17);
        assert_eq!(ControllerType::Generic.axis_count(), 6);
        assert_eq!(ControllerType::PlayStation.name(), "PlayStation Controller");
    }

    #[test]
    fn test_button() {
        assert_eq!(Button::South.index(), 0);
        assert_eq!(Button::from_index(3), Some(Button::North));
        assert_eq!(Button::from_index(100), None);
        assert_eq!(Button::LeftBumper.name(), "Left Bumper (LB/L1)");
    }

    #[test]
    fn test_axis() {
        assert_eq!(Axis::LeftStickX.index(), 0);
        assert!(Axis::LeftTrigger.is_trigger());
        assert!(!Axis::LeftStickX.is_trigger());
        assert_eq!(Axis::from_index(5), Some(Axis::RightTrigger));
    }

    #[test]
    fn test_deadzone() {
        let dz = DeadzoneConfig {
            inner: 0.2,
            outer: 0.8,
        };

        assert_eq!(dz.apply(0.1), 0.0); // Below inner
        assert_eq!(dz.apply(0.9), 1.0); // Above outer
        assert_eq!(dz.apply(-0.9), -1.0); // Negative above outer

        let mid = dz.apply(0.5);
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn test_controller_creation() {
        let ctrl = GameController::new(0, ControllerType::Xbox);
        assert!(ctrl.is_connected());
        assert_eq!(ctrl.controller_type(), ControllerType::Xbox);
        assert_eq!(ctrl.index(), 0);
    }

    #[test]
    fn test_button_press_release() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::South);
        assert!(ctrl.is_pressed(Button::South));

        ctrl.release_button(Button::South);
        assert!(!ctrl.is_pressed(Button::South));
    }

    #[test]
    fn test_button_events() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::North);
        let event = ctrl.poll_event().unwrap();
        assert!(matches!(
            event,
            ControllerEvent::ButtonPressed(Button::North)
        ));

        ctrl.release_button(Button::North);
        let event = ctrl.poll_event().unwrap();
        assert!(matches!(
            event,
            ControllerEvent::ButtonReleased(Button::North)
        ));
    }

    #[test]
    fn test_axis_values() {
        let mut ctrl = GameController::default();

        ctrl.set_axis(Axis::LeftStickX, 0.75);
        assert!((ctrl.get_axis_raw(Axis::LeftStickX) - 0.75).abs() < 0.001);

        // With deadzone
        ctrl.set_axis(Axis::LeftStickX, 0.05);
        assert_eq!(ctrl.get_axis(Axis::LeftStickX), 0.0);
    }

    #[test]
    fn test_trigger_clamping() {
        let mut ctrl = GameController::default();

        ctrl.set_axis(Axis::LeftTrigger, 1.5);
        assert_eq!(ctrl.get_axis_raw(Axis::LeftTrigger), 1.0);

        ctrl.set_axis(Axis::LeftTrigger, -0.5);
        assert_eq!(ctrl.get_axis_raw(Axis::LeftTrigger), 0.0);
    }

    #[test]
    fn test_stick_values() {
        let mut ctrl = GameController::default();

        ctrl.set_left_stick(0.5, -0.5);
        let (x, y) = ctrl.state().left_stick();
        assert!((x - 0.5).abs() < 0.001);
        assert!((y - (-0.5)).abs() < 0.001);

        ctrl.set_right_stick(-0.3, 0.7);
        let (x, y) = ctrl.state().right_stick();
        assert!((x - (-0.3)).abs() < 0.001);
        assert!((y - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_controller_state() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::South);
        ctrl.press_button(Button::East);
        ctrl.set_left_trigger(0.8);

        let state = ctrl.state();
        assert!(state.is_pressed(Button::South));
        assert!(state.is_pressed(Button::East));
        assert!(!state.is_pressed(Button::West));
        assert!((state.get_axis(Axis::LeftTrigger) - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_dpad() {
        let mut ctrl = GameController::default();

        ctrl.set_dpad(true, false, true, false);

        assert!(ctrl.is_pressed(Button::DpadUp));
        assert!(!ctrl.is_pressed(Button::DpadDown));
        assert!(ctrl.is_pressed(Button::DpadLeft));
        assert!(!ctrl.is_pressed(Button::DpadRight));
    }

    #[test]
    fn test_rumble_effect() {
        let effect = RumbleEffect::new(1.0, 0.5, Duration::from_millis(100));
        assert!(effect.is_active());
        assert_eq!(effect.strong, 1.0);
        assert_eq!(effect.weak, 0.5);

        let strong = RumbleEffect::strong(0.8, Duration::from_millis(50));
        assert_eq!(strong.strong, 0.8);
        assert_eq!(strong.weak, 0.0);
    }

    #[test]
    fn test_controller_rumble() {
        let mut ctrl = GameController::default();

        let effect = RumbleEffect::new(1.0, 0.5, Duration::from_secs(1));
        ctrl.rumble(effect);

        let rumble = ctrl.get_rumble();
        assert!(rumble.is_some());
        let (strong, weak) = rumble.unwrap();
        assert_eq!(strong, 1.0);
        assert_eq!(weak, 0.5);

        ctrl.stop_rumble();
        assert!(ctrl.get_rumble().is_none());
    }

    #[test]
    fn test_connected_state() {
        let mut ctrl = GameController::default();

        ctrl.set_connected(false);
        assert!(!ctrl.is_connected());

        let event = ctrl.poll_event().unwrap();
        assert!(matches!(event, ControllerEvent::Disconnected));

        ctrl.set_connected(true);
        let event = ctrl.poll_event().unwrap();
        assert!(matches!(event, ControllerEvent::Connected));
    }

    #[test]
    fn test_tap_button() {
        let mut ctrl = GameController::default();

        ctrl.tap_button(Button::Start);

        let event1 = ctrl.poll_event().unwrap();
        assert!(matches!(event1, ControllerEvent::ButtonPressed(_)));

        let event2 = ctrl.poll_event().unwrap();
        assert!(matches!(event2, ControllerEvent::ButtonReleased(_)));
    }

    #[test]
    fn test_pressed_buttons() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::South);
        ctrl.press_button(Button::West);
        ctrl.press_button(Button::LeftBumper);

        let pressed = ctrl.pressed_buttons();
        assert_eq!(pressed.len(), 3);
        assert!(pressed.contains(&Button::South));
        assert!(pressed.contains(&Button::West));
        assert!(pressed.contains(&Button::LeftBumper));
    }

    #[test]
    fn test_controller_reset() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::South);
        ctrl.set_left_stick(1.0, 1.0);

        ctrl.reset();

        assert!(!ctrl.is_pressed(Button::South));
        assert_eq!(ctrl.get_axis_raw(Axis::LeftStickX), 0.0);
        assert!(!ctrl.has_events());
    }

    #[test]
    fn test_controller_stats() {
        let mut ctrl = GameController::default();

        ctrl.press_button(Button::South);
        ctrl.release_button(Button::South);
        ctrl.set_axis(Axis::LeftStickX, 0.5);

        let stats = ctrl.stats().snapshot();
        assert_eq!(stats.button_presses, 1);
        assert_eq!(stats.button_releases, 1);
        assert_eq!(stats.axis_updates, 1);
        assert_eq!(stats.total_events, 3);
    }

    #[test]
    fn test_axis_no_duplicate_events() {
        let mut ctrl = GameController::default();

        ctrl.set_axis(Axis::LeftStickX, 0.5);
        ctrl.poll_event();

        // Same value should not generate event
        ctrl.set_axis(Axis::LeftStickX, 0.5);
        assert!(ctrl.poll_event().is_none());

        // Different value should generate event
        ctrl.set_axis(Axis::LeftStickX, 0.6);
        assert!(ctrl.poll_event().is_some());
    }

    #[test]
    fn test_state_triggers() {
        let mut ctrl = GameController::default();

        ctrl.set_left_trigger(0.4);
        ctrl.set_right_trigger(0.9);

        let state = ctrl.state();
        let (lt, rt) = state.triggers();
        assert!((lt - 0.4).abs() < 0.001);
        assert!((rt - 0.9).abs() < 0.001);
    }
}
