//! PS/2 Mouse Protocol
//!
//! This module provides comprehensive PS/2 mouse emulation with
//! support for standard, scroll wheel, and 5-button protocols.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// Mouse protocol mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseProtocol {
    /// Standard 3-byte protocol (3 buttons)
    #[default]
    Standard,
    /// Intellimouse 4-byte protocol (3 buttons + scroll)
    Intellimouse,
    /// Intellimouse Explorer 4-byte protocol (5 buttons + scroll)
    Explorer,
}

impl MouseProtocol {
    /// Get packet size for this protocol
    pub fn packet_size(&self) -> usize {
        match self {
            MouseProtocol::Standard => 3,
            MouseProtocol::Intellimouse | MouseProtocol::Explorer => 4,
        }
    }

    /// Get device ID for this protocol
    pub fn device_id(&self) -> u8 {
        match self {
            MouseProtocol::Standard => 0x00,
            MouseProtocol::Intellimouse => 0x03,
            MouseProtocol::Explorer => 0x04,
        }
    }
}

/// Mouse buttons
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseButtons {
    /// Left button
    pub left: bool,
    /// Right button
    pub right: bool,
    /// Middle button
    pub middle: bool,
    /// Button 4 (side button 1, Explorer only)
    pub button4: bool,
    /// Button 5 (side button 2, Explorer only)
    pub button5: bool,
}

impl MouseButtons {
    /// Create from byte (standard protocol)
    pub fn from_byte(value: u8) -> Self {
        Self {
            left: value & 0x01 != 0,
            right: value & 0x02 != 0,
            middle: value & 0x04 != 0,
            button4: false,
            button5: false,
        }
    }

    /// Convert to byte (standard protocol)
    pub fn to_byte(&self) -> u8 {
        let mut value = 0u8;
        if self.left {
            value |= 0x01;
        }
        if self.right {
            value |= 0x02;
        }
        if self.middle {
            value |= 0x04;
        }
        value
    }

    /// Get extra button bits for Explorer protocol
    pub fn extra_bits(&self) -> u8 {
        let mut value = 0u8;
        if self.button4 {
            value |= 0x10;
        }
        if self.button5 {
            value |= 0x20;
        }
        value
    }

    /// Any button pressed
    pub fn any_pressed(&self) -> bool {
        self.left || self.right || self.middle || self.button4 || self.button5
    }
}

/// Mouse resolution (counts per mm)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum MouseResolution {
    /// 1 count/mm
    Res1 = 0,
    /// 2 counts/mm
    Res2 = 1,
    /// 4 counts/mm
    #[default]
    Res4 = 2,
    /// 8 counts/mm
    Res8 = 3,
}

impl MouseResolution {
    /// Create from value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(MouseResolution::Res1),
            1 => Some(MouseResolution::Res2),
            2 => Some(MouseResolution::Res4),
            3 => Some(MouseResolution::Res8),
            _ => None,
        }
    }

    /// Get counts per mm
    pub fn counts_per_mm(&self) -> u32 {
        match self {
            MouseResolution::Res1 => 1,
            MouseResolution::Res2 => 2,
            MouseResolution::Res4 => 4,
            MouseResolution::Res8 => 8,
        }
    }
}

/// Mouse sample rate (reports per second)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SampleRate {
    /// 10 samples/second
    Rate10 = 10,
    /// 20 samples/second
    Rate20 = 20,
    /// 40 samples/second
    Rate40 = 40,
    /// 60 samples/second
    Rate60 = 60,
    /// 80 samples/second
    Rate80 = 80,
    /// 100 samples/second (default)
    #[default]
    Rate100 = 100,
    /// 200 samples/second
    Rate200 = 200,
}

impl SampleRate {
    /// Create from value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            10 => Some(SampleRate::Rate10),
            20 => Some(SampleRate::Rate20),
            40 => Some(SampleRate::Rate40),
            60 => Some(SampleRate::Rate60),
            80 => Some(SampleRate::Rate80),
            100 => Some(SampleRate::Rate100),
            200 => Some(SampleRate::Rate200),
            _ => None,
        }
    }

    /// Get rate value
    pub fn value(&self) -> u8 {
        *self as u8
    }
}

/// Mouse scaling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScalingMode {
    /// Linear 1:1 scaling
    #[default]
    Linear,
    /// Non-linear 2:1 scaling
    NonLinear,
}

impl ScalingMode {
    /// Apply scaling to movement
    pub fn apply(&self, value: i16) -> i16 {
        match self {
            ScalingMode::Linear => value,
            ScalingMode::NonLinear => {
                // Non-linear scaling table
                match value.abs() {
                    0 => 0,
                    1 => value.signum(),
                    2 => value.signum(),
                    3 => value.signum() * 3,
                    4 => value.signum() * 6,
                    5 => value.signum() * 9,
                    _ => value * 2,
                }
            }
        }
    }
}

/// PS/2 mouse commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MouseCommand {
    /// Set scaling 1:1
    SetScaling1to1 = 0xE6,
    /// Set scaling 2:1
    SetScaling2to1 = 0xE7,
    /// Set resolution
    SetResolution = 0xE8,
    /// Get status
    StatusRequest = 0xE9,
    /// Set stream mode
    SetStreamMode = 0xEA,
    /// Read data
    ReadData = 0xEB,
    /// Reset wrap mode
    ResetWrapMode = 0xEC,
    /// Set wrap mode
    SetWrapMode = 0xEE,
    /// Set remote mode
    SetRemoteMode = 0xF0,
    /// Get device ID
    GetDeviceId = 0xF2,
    /// Set sample rate
    SetSampleRate = 0xF3,
    /// Enable data reporting
    EnableReporting = 0xF4,
    /// Disable data reporting
    DisableReporting = 0xF5,
    /// Set defaults
    SetDefaults = 0xF6,
    /// Resend
    Resend = 0xFE,
    /// Reset
    Reset = 0xFF,
}

impl MouseCommand {
    /// Create from byte
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0xE6 => Some(MouseCommand::SetScaling1to1),
            0xE7 => Some(MouseCommand::SetScaling2to1),
            0xE8 => Some(MouseCommand::SetResolution),
            0xE9 => Some(MouseCommand::StatusRequest),
            0xEA => Some(MouseCommand::SetStreamMode),
            0xEB => Some(MouseCommand::ReadData),
            0xEC => Some(MouseCommand::ResetWrapMode),
            0xEE => Some(MouseCommand::SetWrapMode),
            0xF0 => Some(MouseCommand::SetRemoteMode),
            0xF2 => Some(MouseCommand::GetDeviceId),
            0xF3 => Some(MouseCommand::SetSampleRate),
            0xF4 => Some(MouseCommand::EnableReporting),
            0xF5 => Some(MouseCommand::DisableReporting),
            0xF6 => Some(MouseCommand::SetDefaults),
            0xFE => Some(MouseCommand::Resend),
            0xFF => Some(MouseCommand::Reset),
            _ => None,
        }
    }
}

/// Response codes
pub mod Response {
    /// Acknowledgement
    pub const ACK: u8 = 0xFA;
    /// Resend
    pub const RESEND: u8 = 0xFE;
    /// Self-test passed
    pub const BAT_OK: u8 = 0xAA;
    /// Error
    pub const ERROR: u8 = 0xFC;
}

/// Command state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CommandState {
    #[default]
    Idle,
    WaitingResolution,
    WaitingSampleRate,
}

/// Mouse statistics
#[derive(Debug, Default)]
pub struct MouseStats {
    /// Packets sent
    packets_sent: AtomicU64,
    /// Button events
    button_events: AtomicU64,
    /// Total movement (pixels)
    total_movement: AtomicU64,
    /// Scroll events
    scroll_events: AtomicU64,
    /// Commands received
    commands_received: AtomicU64,
}

impl MouseStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record packet
    pub fn record_packet(&self) {
        self.packets_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record button event
    pub fn record_button(&self) {
        self.button_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record movement
    pub fn record_movement(&self, dx: i16, dy: i16) {
        let distance = ((dx as i32).abs() + (dy as i32).abs()) as u64;
        self.total_movement.fetch_add(distance, Ordering::Relaxed);
    }

    /// Record scroll
    pub fn record_scroll(&self) {
        self.scroll_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Record command
    pub fn record_command(&self) {
        self.commands_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> MouseStatsSnapshot {
        MouseStatsSnapshot {
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            button_events: self.button_events.load(Ordering::Relaxed),
            total_movement: self.total_movement.load(Ordering::Relaxed),
            scroll_events: self.scroll_events.load(Ordering::Relaxed),
            commands_received: self.commands_received.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct MouseStatsSnapshot {
    /// Packets sent
    pub packets_sent: u64,
    /// Button events
    pub button_events: u64,
    /// Total movement
    pub total_movement: u64,
    /// Scroll events
    pub scroll_events: u64,
    /// Commands received
    pub commands_received: u64,
}

/// PS/2 Mouse device
pub struct Ps2Mouse {
    /// Current protocol
    protocol: MouseProtocol,
    /// Current button state
    buttons: MouseButtons,
    /// Resolution
    resolution: MouseResolution,
    /// Sample rate
    sample_rate: SampleRate,
    /// Scaling mode
    scaling: ScalingMode,
    /// Stream mode enabled
    stream_mode: bool,
    /// Data reporting enabled
    reporting_enabled: bool,
    /// Wrap mode enabled
    wrap_mode: bool,
    /// Output buffer
    output_buffer: VecDeque<u8>,
    /// Last output for resend
    last_output: Vec<u8>,
    /// Command state
    command_state: CommandState,
    /// Sample rate sequence for protocol detection
    rate_sequence: Vec<u8>,
    /// Statistics
    stats: MouseStats,
    /// Interrupt pending
    interrupt_pending: bool,
}

impl Default for Ps2Mouse {
    fn default() -> Self {
        Self::new()
    }
}

impl Ps2Mouse {
    /// Create new mouse
    pub fn new() -> Self {
        Self {
            protocol: MouseProtocol::Standard,
            buttons: MouseButtons::default(),
            resolution: MouseResolution::default(),
            sample_rate: SampleRate::default(),
            scaling: ScalingMode::default(),
            stream_mode: true,
            reporting_enabled: true,
            wrap_mode: false,
            output_buffer: VecDeque::with_capacity(64),
            last_output: Vec::new(),
            command_state: CommandState::Idle,
            rate_sequence: Vec::new(),
            stats: MouseStats::new(),
            interrupt_pending: false,
        }
    }

    /// Get current protocol
    pub fn protocol(&self) -> MouseProtocol {
        self.protocol
    }

    /// Get resolution
    pub fn resolution(&self) -> MouseResolution {
        self.resolution
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// Get scaling mode
    pub fn scaling(&self) -> ScalingMode {
        self.scaling
    }

    /// Check if reporting is enabled
    pub fn is_reporting_enabled(&self) -> bool {
        self.reporting_enabled
    }

    /// Get statistics
    pub fn stats(&self) -> &MouseStats {
        &self.stats
    }

    /// Check for pending interrupt
    pub fn has_interrupt(&self) -> bool {
        self.interrupt_pending
    }

    /// Clear interrupt
    pub fn clear_interrupt(&mut self) {
        self.interrupt_pending = false;
    }

    /// Read output byte
    pub fn read(&mut self) -> Option<u8> {
        let byte = self.output_buffer.pop_front();
        if self.output_buffer.is_empty() {
            self.interrupt_pending = false;
        }
        byte
    }

    /// Check if output buffer has data
    pub fn has_data(&self) -> bool {
        !self.output_buffer.is_empty()
    }

    /// Get output buffer length
    pub fn output_len(&self) -> usize {
        self.output_buffer.len()
    }

    /// Write command/data to mouse
    pub fn write(&mut self, value: u8) {
        self.stats.record_command();

        // Wrap mode echoes everything except reset and reset wrap
        if self.wrap_mode && value != 0xFF && value != 0xEC {
            self.send_byte(value);
            return;
        }

        match self.command_state {
            CommandState::Idle => self.handle_command(value),
            CommandState::WaitingResolution => {
                if let Some(res) = MouseResolution::from_u8(value) {
                    self.resolution = res;
                    self.send_ack();
                } else {
                    self.send_byte(Response::RESEND);
                }
                self.command_state = CommandState::Idle;
            }
            CommandState::WaitingSampleRate => {
                if let Some(rate) = SampleRate::from_u8(value) {
                    self.sample_rate = rate;
                    self.rate_sequence.push(value);
                    self.check_protocol_upgrade();
                    self.send_ack();
                } else {
                    self.send_byte(Response::RESEND);
                }
                self.command_state = CommandState::Idle;
            }
        }
    }

    /// Handle command
    fn handle_command(&mut self, value: u8) {
        if let Some(cmd) = MouseCommand::from_byte(value) {
            match cmd {
                MouseCommand::SetScaling1to1 => {
                    self.scaling = ScalingMode::Linear;
                    self.send_ack();
                }
                MouseCommand::SetScaling2to1 => {
                    self.scaling = ScalingMode::NonLinear;
                    self.send_ack();
                }
                MouseCommand::SetResolution => {
                    self.send_ack();
                    self.command_state = CommandState::WaitingResolution;
                }
                MouseCommand::StatusRequest => {
                    self.send_ack();
                    self.send_status();
                }
                MouseCommand::SetStreamMode => {
                    self.stream_mode = true;
                    self.send_ack();
                }
                MouseCommand::ReadData => {
                    self.send_ack();
                    self.send_packet(0, 0, 0);
                }
                MouseCommand::ResetWrapMode => {
                    self.wrap_mode = false;
                    self.send_ack();
                }
                MouseCommand::SetWrapMode => {
                    self.wrap_mode = true;
                    self.send_ack();
                }
                MouseCommand::SetRemoteMode => {
                    self.stream_mode = false;
                    self.send_ack();
                }
                MouseCommand::GetDeviceId => {
                    self.send_ack();
                    self.send_byte(self.protocol.device_id());
                }
                MouseCommand::SetSampleRate => {
                    self.send_ack();
                    self.command_state = CommandState::WaitingSampleRate;
                }
                MouseCommand::EnableReporting => {
                    self.reporting_enabled = true;
                    self.send_ack();
                }
                MouseCommand::DisableReporting => {
                    self.reporting_enabled = false;
                    self.send_ack();
                }
                MouseCommand::SetDefaults => {
                    self.set_defaults();
                    self.send_ack();
                }
                MouseCommand::Resend => {
                    for &byte in &self.last_output {
                        self.output_buffer.push_back(byte);
                    }
                    if !self.output_buffer.is_empty() {
                        self.interrupt_pending = true;
                    }
                }
                MouseCommand::Reset => {
                    self.reset();
                    self.send_ack();
                    self.send_byte(Response::BAT_OK);
                    self.send_byte(self.protocol.device_id());
                }
            }
        }
    }

    /// Check for protocol upgrade sequence
    fn check_protocol_upgrade(&mut self) {
        let len = self.rate_sequence.len();

        // Intellimouse sequence: 200, 100, 80
        if len >= 3 {
            let last3 = &self.rate_sequence[len - 3..];
            if last3 == [200, 100, 80] && self.protocol == MouseProtocol::Standard {
                self.protocol = MouseProtocol::Intellimouse;
                self.rate_sequence.clear();
                return;
            }
        }

        // Explorer sequence: 200, 200, 80
        if len >= 3 {
            let last3 = &self.rate_sequence[len - 3..];
            if last3 == [200, 200, 80] && self.protocol == MouseProtocol::Intellimouse {
                self.protocol = MouseProtocol::Explorer;
                self.rate_sequence.clear();
                return;
            }
        }

        // Trim sequence if too long
        if len > 10 {
            self.rate_sequence.drain(0..len - 5);
        }
    }

    /// Set defaults
    fn set_defaults(&mut self) {
        self.resolution = MouseResolution::default();
        self.sample_rate = SampleRate::default();
        self.scaling = ScalingMode::default();
        self.stream_mode = true;
        self.reporting_enabled = false;
    }

    /// Reset mouse
    pub fn reset(&mut self) {
        self.protocol = MouseProtocol::Standard;
        self.buttons = MouseButtons::default();
        self.set_defaults();
        self.output_buffer.clear();
        self.last_output.clear();
        self.command_state = CommandState::Idle;
        self.rate_sequence.clear();
        self.wrap_mode = false;
    }

    /// Send acknowledgement
    fn send_ack(&mut self) {
        self.send_byte(Response::ACK);
    }

    /// Send byte to output buffer
    fn send_byte(&mut self, value: u8) {
        self.output_buffer.push_back(value);
        self.interrupt_pending = true;
    }

    /// Send status bytes
    fn send_status(&mut self) {
        let byte1 = self.buttons.to_byte()
            | if self.scaling == ScalingMode::NonLinear {
                0x10
            } else {
                0
            }
            | if self.reporting_enabled { 0x20 } else { 0 }
            | if self.stream_mode { 0x00 } else { 0x40 };

        self.send_byte(byte1);
        self.send_byte(self.resolution as u8);
        self.send_byte(self.sample_rate.value());
    }

    /// Send movement packet
    fn send_packet(&mut self, dx: i16, dy: i16, scroll: i8) {
        self.last_output.clear();
        self.stats.record_packet();

        // Apply scaling
        let dx = self.scaling.apply(dx);
        let dy = self.scaling.apply(dy);

        // Clamp to -256..255
        let dx = dx.clamp(-256, 255);
        let dy = dy.clamp(-256, 255);

        // Byte 1: buttons, signs, overflow
        let mut byte1 = self.buttons.to_byte();
        if dx < 0 {
            byte1 |= 0x10;
        }
        if dy < 0 {
            byte1 |= 0x20;
        }
        if dx < -256 || dx > 255 {
            byte1 |= 0x40;
        }
        if dy < -256 || dy > 255 {
            byte1 |= 0x80;
        }

        // Byte 2: X movement
        let byte2 = (dx & 0xFF) as u8;

        // Byte 3: Y movement
        let byte3 = (dy & 0xFF) as u8;

        self.output_buffer.push_back(byte1);
        self.last_output.push(byte1);
        self.output_buffer.push_back(byte2);
        self.last_output.push(byte2);
        self.output_buffer.push_back(byte3);
        self.last_output.push(byte3);

        // Byte 4 for extended protocols
        match self.protocol {
            MouseProtocol::Standard => {}
            MouseProtocol::Intellimouse => {
                let byte4 = (scroll as i8) as u8;
                self.output_buffer.push_back(byte4);
                self.last_output.push(byte4);
            }
            MouseProtocol::Explorer => {
                let mut byte4 = (scroll.clamp(-8, 7) & 0x0F) as u8;
                byte4 |= self.buttons.extra_bits();
                self.output_buffer.push_back(byte4);
                self.last_output.push(byte4);
            }
        }

        self.interrupt_pending = true;
    }

    /// Move mouse
    pub fn move_mouse(&mut self, dx: i16, dy: i16) {
        if !self.reporting_enabled || !self.stream_mode {
            return;
        }

        self.stats.record_movement(dx, dy);
        self.send_packet(dx, dy, 0);
    }

    /// Move mouse with scroll
    pub fn move_with_scroll(&mut self, dx: i16, dy: i16, scroll: i8) {
        if !self.reporting_enabled || !self.stream_mode {
            return;
        }

        self.stats.record_movement(dx, dy);
        if scroll != 0 {
            self.stats.record_scroll();
        }
        self.send_packet(dx, dy, scroll);
    }

    /// Scroll wheel
    pub fn scroll(&mut self, amount: i8) {
        if !self.reporting_enabled || !self.stream_mode {
            return;
        }

        self.stats.record_scroll();
        self.send_packet(0, 0, amount);
    }

    /// Button press
    pub fn button_press(&mut self, button: u8) {
        self.stats.record_button();

        match button {
            0 => self.buttons.left = true,
            1 => self.buttons.right = true,
            2 => self.buttons.middle = true,
            3 => self.buttons.button4 = true,
            4 => self.buttons.button5 = true,
            _ => return,
        }

        if self.reporting_enabled && self.stream_mode {
            self.send_packet(0, 0, 0);
        }
    }

    /// Button release
    pub fn button_release(&mut self, button: u8) {
        self.stats.record_button();

        match button {
            0 => self.buttons.left = false,
            1 => self.buttons.right = false,
            2 => self.buttons.middle = false,
            3 => self.buttons.button4 = false,
            4 => self.buttons.button5 = false,
            _ => return,
        }

        if self.reporting_enabled && self.stream_mode {
            self.send_packet(0, 0, 0);
        }
    }

    /// Set buttons directly
    pub fn set_buttons(&mut self, buttons: MouseButtons) {
        let changed = buttons.to_byte() != self.buttons.to_byte()
            || buttons.button4 != self.buttons.button4
            || buttons.button5 != self.buttons.button5;

        self.buttons = buttons;

        if changed && self.reporting_enabled && self.stream_mode {
            self.stats.record_button();
            self.send_packet(0, 0, 0);
        }
    }

    /// Get current button state
    pub fn buttons(&self) -> MouseButtons {
        self.buttons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_protocol() {
        assert_eq!(MouseProtocol::Standard.packet_size(), 3);
        assert_eq!(MouseProtocol::Intellimouse.packet_size(), 4);
        assert_eq!(MouseProtocol::Explorer.packet_size(), 4);

        assert_eq!(MouseProtocol::Standard.device_id(), 0x00);
        assert_eq!(MouseProtocol::Intellimouse.device_id(), 0x03);
        assert_eq!(MouseProtocol::Explorer.device_id(), 0x04);
    }

    #[test]
    fn test_mouse_buttons() {
        let buttons = MouseButtons::from_byte(0x07);
        assert!(buttons.left);
        assert!(buttons.right);
        assert!(buttons.middle);

        let buttons = MouseButtons {
            left: true,
            right: false,
            middle: true,
            button4: false,
            button5: false,
        };
        assert_eq!(buttons.to_byte(), 0x05);
    }

    #[test]
    fn test_mouse_resolution() {
        assert_eq!(MouseResolution::Res1.counts_per_mm(), 1);
        assert_eq!(MouseResolution::Res4.counts_per_mm(), 4);
        assert_eq!(MouseResolution::Res8.counts_per_mm(), 8);
    }

    #[test]
    fn test_sample_rate() {
        assert_eq!(SampleRate::Rate100.value(), 100);
        assert_eq!(SampleRate::from_u8(200), Some(SampleRate::Rate200));
        assert_eq!(SampleRate::from_u8(50), None);
    }

    #[test]
    fn test_scaling_mode() {
        assert_eq!(ScalingMode::Linear.apply(5), 5);
        assert_eq!(ScalingMode::NonLinear.apply(1), 1);
        assert_eq!(ScalingMode::NonLinear.apply(5), 9);
        assert_eq!(ScalingMode::NonLinear.apply(-5), -9);
    }

    #[test]
    fn test_mouse_creation() {
        let mouse = Ps2Mouse::new();
        assert_eq!(mouse.protocol(), MouseProtocol::Standard);
        assert!(mouse.is_reporting_enabled());
        assert!(!mouse.has_data());
    }

    #[test]
    fn test_mouse_movement() {
        let mut mouse = Ps2Mouse::new();
        mouse.move_mouse(10, -5);

        assert!(mouse.has_data());
        assert_eq!(mouse.output_len(), 3); // Standard protocol

        // Byte 1: buttons and signs
        let byte1 = mouse.read().unwrap();
        assert_eq!(byte1 & 0x10, 0); // X positive
        assert_eq!(byte1 & 0x20, 0x20); // Y negative

        // Byte 2: X movement
        assert_eq!(mouse.read().unwrap(), 10);

        // Byte 3: Y movement (two's complement for -5)
        assert_eq!(mouse.read().unwrap() as i8, -5);
    }

    #[test]
    fn test_mouse_buttons_press() {
        let mut mouse = Ps2Mouse::new();

        mouse.button_press(0); // Left
        assert!(mouse.has_data());

        let byte1 = mouse.read().unwrap();
        assert_eq!(byte1 & 0x01, 0x01); // Left button

        mouse.output_buffer.clear();
        mouse.button_press(1); // Right

        let byte1 = mouse.read().unwrap();
        assert_eq!(byte1 & 0x03, 0x03); // Left + Right
    }

    #[test]
    fn test_mouse_reset() {
        let mut mouse = Ps2Mouse::new();
        mouse.write(0xFF);

        assert_eq!(mouse.read(), Some(Response::ACK));
        assert_eq!(mouse.read(), Some(Response::BAT_OK));
        assert_eq!(mouse.read(), Some(0x00)); // Standard device ID
    }

    #[test]
    fn test_mouse_get_device_id() {
        let mut mouse = Ps2Mouse::new();
        mouse.write(0xF2);

        assert_eq!(mouse.read(), Some(Response::ACK));
        assert_eq!(mouse.read(), Some(0x00)); // Standard
    }

    #[test]
    fn test_mouse_protocol_upgrade_intellimouse() {
        let mut mouse = Ps2Mouse::new();

        // Send magic sequence: 200, 100, 80
        mouse.write(0xF3);
        mouse.read();
        mouse.write(200);
        mouse.read();

        mouse.write(0xF3);
        mouse.read();
        mouse.write(100);
        mouse.read();

        mouse.write(0xF3);
        mouse.read();
        mouse.write(80);
        mouse.read();

        assert_eq!(mouse.protocol(), MouseProtocol::Intellimouse);
    }

    #[test]
    fn test_mouse_intellimouse_scroll() {
        let mut mouse = Ps2Mouse::new();

        // Upgrade to Intellimouse
        for rate in [200, 100, 80] {
            mouse.write(0xF3);
            mouse.read();
            mouse.write(rate);
            mouse.read();
        }

        mouse.scroll(3);
        assert_eq!(mouse.output_len(), 4);

        mouse.read(); // byte1
        mouse.read(); // byte2
        mouse.read(); // byte3
        assert_eq!(mouse.read().unwrap(), 3); // scroll
    }

    #[test]
    fn test_mouse_status_request() {
        let mut mouse = Ps2Mouse::new();
        mouse.write(0xE9);

        assert_eq!(mouse.read(), Some(Response::ACK));

        let status = mouse.read().unwrap();
        assert_eq!(status & 0x20, 0x20); // Reporting enabled

        let _resolution = mouse.read().unwrap();
        let sample_rate = mouse.read().unwrap();
        assert_eq!(sample_rate, 100); // Default
    }

    #[test]
    fn test_mouse_set_resolution() {
        let mut mouse = Ps2Mouse::new();

        mouse.write(0xE8); // Set resolution
        assert_eq!(mouse.read(), Some(Response::ACK));

        mouse.write(3); // 8 counts/mm
        assert_eq!(mouse.read(), Some(Response::ACK));

        assert_eq!(mouse.resolution(), MouseResolution::Res8);
    }

    #[test]
    fn test_mouse_scaling() {
        let mut mouse = Ps2Mouse::new();

        mouse.write(0xE7); // 2:1 scaling
        mouse.read();

        assert_eq!(mouse.scaling(), ScalingMode::NonLinear);

        mouse.write(0xE6); // 1:1 scaling
        mouse.read();

        assert_eq!(mouse.scaling(), ScalingMode::Linear);
    }

    #[test]
    fn test_mouse_enable_disable() {
        let mut mouse = Ps2Mouse::new();

        mouse.write(0xF5); // Disable
        mouse.read();
        assert!(!mouse.is_reporting_enabled());

        mouse.move_mouse(10, 10);
        assert!(!mouse.has_data());

        mouse.write(0xF4); // Enable
        mouse.read();
        assert!(mouse.is_reporting_enabled());

        mouse.move_mouse(10, 10);
        assert!(mouse.has_data());
    }

    #[test]
    fn test_mouse_stats() {
        let mut mouse = Ps2Mouse::new();
        mouse.move_mouse(5, 5);
        mouse.button_press(0);
        mouse.scroll(1);

        let stats = mouse.stats().snapshot();
        assert!(stats.packets_sent > 0);
        assert!(stats.button_events > 0);
        assert!(stats.total_movement > 0);
    }

    #[test]
    fn test_mouse_interrupt() {
        let mut mouse = Ps2Mouse::new();
        assert!(!mouse.has_interrupt());

        mouse.move_mouse(1, 1);
        assert!(mouse.has_interrupt());

        while mouse.read().is_some() {}
        assert!(!mouse.has_interrupt());
    }

    #[test]
    fn test_mouse_set_defaults() {
        let mut mouse = Ps2Mouse::new();

        // Change settings
        mouse.write(0xE7); // 2:1 scaling
        mouse.read();

        // Set defaults
        mouse.write(0xF6);
        mouse.read();

        assert_eq!(mouse.scaling(), ScalingMode::Linear);
        assert!(!mouse.is_reporting_enabled()); // Disabled by set defaults
    }

    #[test]
    fn test_mouse_explorer_buttons() {
        let mut mouse = Ps2Mouse::new();

        // Upgrade to Intellimouse first
        for rate in [200, 100, 80] {
            mouse.write(0xF3);
            mouse.read();
            mouse.write(rate);
            mouse.read();
        }

        // Upgrade to Explorer
        for rate in [200, 200, 80] {
            mouse.write(0xF3);
            mouse.read();
            mouse.write(rate);
            mouse.read();
        }

        assert_eq!(mouse.protocol(), MouseProtocol::Explorer);

        mouse.button_press(3); // Button 4
        let _b1 = mouse.read().unwrap();
        let _b2 = mouse.read().unwrap();
        let _b3 = mouse.read().unwrap();
        let b4 = mouse.read().unwrap();
        assert_eq!(b4 & 0x10, 0x10); // Button 4
    }
}
