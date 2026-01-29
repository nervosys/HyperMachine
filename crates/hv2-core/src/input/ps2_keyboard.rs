//! PS/2 Keyboard with Scan Code Sets
//!
//! This module provides comprehensive PS/2 keyboard emulation with
//! support for scan code sets 1, 2, and 3.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// Scan code set
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ScanCodeSet {
    /// Scan code set 1 (XT compatible)
    Set1 = 1,
    /// Scan code set 2 (AT compatible, default)
    #[default]
    Set2 = 2,
    /// Scan code set 3 (PS/2)
    Set3 = 3,
}

impl ScanCodeSet {
    /// Create from value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(ScanCodeSet::Set1),
            2 => Some(ScanCodeSet::Set2),
            3 => Some(ScanCodeSet::Set3),
            _ => None,
        }
    }
}

/// Key codes (USB HID usage codes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyCode {
    // Letters
    A = 0x04,
    B = 0x05,
    C = 0x06,
    D = 0x07,
    E = 0x08,
    F = 0x09,
    G = 0x0A,
    H = 0x0B,
    I = 0x0C,
    J = 0x0D,
    K = 0x0E,
    L = 0x0F,
    M = 0x10,
    N = 0x11,
    O = 0x12,
    P = 0x13,
    Q = 0x14,
    R = 0x15,
    S = 0x16,
    T = 0x17,
    U = 0x18,
    V = 0x19,
    W = 0x1A,
    X = 0x1B,
    Y = 0x1C,
    Z = 0x1D,

    // Numbers
    Num1 = 0x1E,
    Num2 = 0x1F,
    Num3 = 0x20,
    Num4 = 0x21,
    Num5 = 0x22,
    Num6 = 0x23,
    Num7 = 0x24,
    Num8 = 0x25,
    Num9 = 0x26,
    Num0 = 0x27,

    // Special keys
    Enter = 0x28,
    Escape = 0x29,
    Backspace = 0x2A,
    Tab = 0x2B,
    Space = 0x2C,
    Minus = 0x2D,
    Equal = 0x2E,
    LeftBracket = 0x2F,
    RightBracket = 0x30,
    Backslash = 0x31,
    Semicolon = 0x33,
    Quote = 0x34,
    Grave = 0x35,
    Comma = 0x36,
    Period = 0x37,
    Slash = 0x38,
    CapsLock = 0x39,

    // Function keys
    F1 = 0x3A,
    F2 = 0x3B,
    F3 = 0x3C,
    F4 = 0x3D,
    F5 = 0x3E,
    F6 = 0x3F,
    F7 = 0x40,
    F8 = 0x41,
    F9 = 0x42,
    F10 = 0x43,
    F11 = 0x44,
    F12 = 0x45,

    // Control keys
    PrintScreen = 0x46,
    ScrollLock = 0x47,
    Pause = 0x48,
    Insert = 0x49,
    Home = 0x4A,
    PageUp = 0x4B,
    Delete = 0x4C,
    End = 0x4D,
    PageDown = 0x4E,
    Right = 0x4F,
    Left = 0x50,
    Down = 0x51,
    Up = 0x52,
    NumLock = 0x53,

    // Keypad
    KpDivide = 0x54,
    KpMultiply = 0x55,
    KpMinus = 0x56,
    KpPlus = 0x57,
    KpEnter = 0x58,
    Kp1 = 0x59,
    Kp2 = 0x5A,
    Kp3 = 0x5B,
    Kp4 = 0x5C,
    Kp5 = 0x5D,
    Kp6 = 0x5E,
    Kp7 = 0x5F,
    Kp8 = 0x60,
    Kp9 = 0x61,
    Kp0 = 0x62,
    KpPeriod = 0x63,

    // Modifiers
    LeftControl = 0xE0,
    LeftShift = 0xE1,
    LeftAlt = 0xE2,
    LeftGui = 0xE3,
    RightControl = 0xE4,
    RightShift = 0xE5,
    RightAlt = 0xE6,
    RightGui = 0xE7,
}

impl KeyCode {
    /// Get scan code set 1 make code
    pub fn set1_make(&self) -> &'static [u8] {
        match self {
            KeyCode::A => &[0x1E],
            KeyCode::B => &[0x30],
            KeyCode::C => &[0x2E],
            KeyCode::D => &[0x20],
            KeyCode::E => &[0x12],
            KeyCode::F => &[0x21],
            KeyCode::G => &[0x22],
            KeyCode::H => &[0x23],
            KeyCode::I => &[0x17],
            KeyCode::J => &[0x24],
            KeyCode::K => &[0x25],
            KeyCode::L => &[0x26],
            KeyCode::M => &[0x32],
            KeyCode::N => &[0x31],
            KeyCode::O => &[0x18],
            KeyCode::P => &[0x19],
            KeyCode::Q => &[0x10],
            KeyCode::R => &[0x13],
            KeyCode::S => &[0x1F],
            KeyCode::T => &[0x14],
            KeyCode::U => &[0x16],
            KeyCode::V => &[0x2F],
            KeyCode::W => &[0x11],
            KeyCode::X => &[0x2D],
            KeyCode::Y => &[0x15],
            KeyCode::Z => &[0x2C],
            KeyCode::Num1 => &[0x02],
            KeyCode::Num2 => &[0x03],
            KeyCode::Num3 => &[0x04],
            KeyCode::Num4 => &[0x05],
            KeyCode::Num5 => &[0x06],
            KeyCode::Num6 => &[0x07],
            KeyCode::Num7 => &[0x08],
            KeyCode::Num8 => &[0x09],
            KeyCode::Num9 => &[0x0A],
            KeyCode::Num0 => &[0x0B],
            KeyCode::Enter => &[0x1C],
            KeyCode::Escape => &[0x01],
            KeyCode::Backspace => &[0x0E],
            KeyCode::Tab => &[0x0F],
            KeyCode::Space => &[0x39],
            KeyCode::Minus => &[0x0C],
            KeyCode::Equal => &[0x0D],
            KeyCode::LeftBracket => &[0x1A],
            KeyCode::RightBracket => &[0x1B],
            KeyCode::Backslash => &[0x2B],
            KeyCode::Semicolon => &[0x27],
            KeyCode::Quote => &[0x28],
            KeyCode::Grave => &[0x29],
            KeyCode::Comma => &[0x33],
            KeyCode::Period => &[0x34],
            KeyCode::Slash => &[0x35],
            KeyCode::CapsLock => &[0x3A],
            KeyCode::F1 => &[0x3B],
            KeyCode::F2 => &[0x3C],
            KeyCode::F3 => &[0x3D],
            KeyCode::F4 => &[0x3E],
            KeyCode::F5 => &[0x3F],
            KeyCode::F6 => &[0x40],
            KeyCode::F7 => &[0x41],
            KeyCode::F8 => &[0x42],
            KeyCode::F9 => &[0x43],
            KeyCode::F10 => &[0x44],
            KeyCode::F11 => &[0x57],
            KeyCode::F12 => &[0x58],
            KeyCode::PrintScreen => &[0xE0, 0x2A, 0xE0, 0x37],
            KeyCode::ScrollLock => &[0x46],
            KeyCode::Pause => &[0xE1, 0x1D, 0x45, 0xE1, 0x9D, 0xC5],
            KeyCode::Insert => &[0xE0, 0x52],
            KeyCode::Home => &[0xE0, 0x47],
            KeyCode::PageUp => &[0xE0, 0x49],
            KeyCode::Delete => &[0xE0, 0x53],
            KeyCode::End => &[0xE0, 0x4F],
            KeyCode::PageDown => &[0xE0, 0x51],
            KeyCode::Right => &[0xE0, 0x4D],
            KeyCode::Left => &[0xE0, 0x4B],
            KeyCode::Down => &[0xE0, 0x50],
            KeyCode::Up => &[0xE0, 0x48],
            KeyCode::NumLock => &[0x45],
            KeyCode::KpDivide => &[0xE0, 0x35],
            KeyCode::KpMultiply => &[0x37],
            KeyCode::KpMinus => &[0x4A],
            KeyCode::KpPlus => &[0x4E],
            KeyCode::KpEnter => &[0xE0, 0x1C],
            KeyCode::Kp1 => &[0x4F],
            KeyCode::Kp2 => &[0x50],
            KeyCode::Kp3 => &[0x51],
            KeyCode::Kp4 => &[0x4B],
            KeyCode::Kp5 => &[0x4C],
            KeyCode::Kp6 => &[0x4D],
            KeyCode::Kp7 => &[0x47],
            KeyCode::Kp8 => &[0x48],
            KeyCode::Kp9 => &[0x49],
            KeyCode::Kp0 => &[0x52],
            KeyCode::KpPeriod => &[0x53],
            KeyCode::LeftControl => &[0x1D],
            KeyCode::LeftShift => &[0x2A],
            KeyCode::LeftAlt => &[0x38],
            KeyCode::LeftGui => &[0xE0, 0x5B],
            KeyCode::RightControl => &[0xE0, 0x1D],
            KeyCode::RightShift => &[0x36],
            KeyCode::RightAlt => &[0xE0, 0x38],
            KeyCode::RightGui => &[0xE0, 0x5C],
        }
    }

    /// Get scan code set 2 make code
    pub fn set2_make(&self) -> &'static [u8] {
        match self {
            KeyCode::A => &[0x1C],
            KeyCode::B => &[0x32],
            KeyCode::C => &[0x21],
            KeyCode::D => &[0x23],
            KeyCode::E => &[0x24],
            KeyCode::F => &[0x2B],
            KeyCode::G => &[0x34],
            KeyCode::H => &[0x33],
            KeyCode::I => &[0x43],
            KeyCode::J => &[0x3B],
            KeyCode::K => &[0x42],
            KeyCode::L => &[0x4B],
            KeyCode::M => &[0x3A],
            KeyCode::N => &[0x31],
            KeyCode::O => &[0x44],
            KeyCode::P => &[0x4D],
            KeyCode::Q => &[0x15],
            KeyCode::R => &[0x2D],
            KeyCode::S => &[0x1B],
            KeyCode::T => &[0x2C],
            KeyCode::U => &[0x3C],
            KeyCode::V => &[0x2A],
            KeyCode::W => &[0x1D],
            KeyCode::X => &[0x22],
            KeyCode::Y => &[0x35],
            KeyCode::Z => &[0x1A],
            KeyCode::Num1 => &[0x16],
            KeyCode::Num2 => &[0x1E],
            KeyCode::Num3 => &[0x26],
            KeyCode::Num4 => &[0x25],
            KeyCode::Num5 => &[0x2E],
            KeyCode::Num6 => &[0x36],
            KeyCode::Num7 => &[0x3D],
            KeyCode::Num8 => &[0x3E],
            KeyCode::Num9 => &[0x46],
            KeyCode::Num0 => &[0x45],
            KeyCode::Enter => &[0x5A],
            KeyCode::Escape => &[0x76],
            KeyCode::Backspace => &[0x66],
            KeyCode::Tab => &[0x0D],
            KeyCode::Space => &[0x29],
            KeyCode::Minus => &[0x4E],
            KeyCode::Equal => &[0x55],
            KeyCode::LeftBracket => &[0x54],
            KeyCode::RightBracket => &[0x5B],
            KeyCode::Backslash => &[0x5D],
            KeyCode::Semicolon => &[0x4C],
            KeyCode::Quote => &[0x52],
            KeyCode::Grave => &[0x0E],
            KeyCode::Comma => &[0x41],
            KeyCode::Period => &[0x49],
            KeyCode::Slash => &[0x4A],
            KeyCode::CapsLock => &[0x58],
            KeyCode::F1 => &[0x05],
            KeyCode::F2 => &[0x06],
            KeyCode::F3 => &[0x04],
            KeyCode::F4 => &[0x0C],
            KeyCode::F5 => &[0x03],
            KeyCode::F6 => &[0x0B],
            KeyCode::F7 => &[0x83],
            KeyCode::F8 => &[0x0A],
            KeyCode::F9 => &[0x01],
            KeyCode::F10 => &[0x09],
            KeyCode::F11 => &[0x78],
            KeyCode::F12 => &[0x07],
            KeyCode::PrintScreen => &[0xE0, 0x12, 0xE0, 0x7C],
            KeyCode::ScrollLock => &[0x7E],
            KeyCode::Pause => &[0xE1, 0x14, 0x77, 0xE1, 0xF0, 0x14, 0xF0, 0x77],
            KeyCode::Insert => &[0xE0, 0x70],
            KeyCode::Home => &[0xE0, 0x6C],
            KeyCode::PageUp => &[0xE0, 0x7D],
            KeyCode::Delete => &[0xE0, 0x71],
            KeyCode::End => &[0xE0, 0x69],
            KeyCode::PageDown => &[0xE0, 0x7A],
            KeyCode::Right => &[0xE0, 0x74],
            KeyCode::Left => &[0xE0, 0x6B],
            KeyCode::Down => &[0xE0, 0x72],
            KeyCode::Up => &[0xE0, 0x75],
            KeyCode::NumLock => &[0x77],
            KeyCode::KpDivide => &[0xE0, 0x4A],
            KeyCode::KpMultiply => &[0x7C],
            KeyCode::KpMinus => &[0x7B],
            KeyCode::KpPlus => &[0x79],
            KeyCode::KpEnter => &[0xE0, 0x5A],
            KeyCode::Kp1 => &[0x69],
            KeyCode::Kp2 => &[0x72],
            KeyCode::Kp3 => &[0x7A],
            KeyCode::Kp4 => &[0x6B],
            KeyCode::Kp5 => &[0x73],
            KeyCode::Kp6 => &[0x74],
            KeyCode::Kp7 => &[0x6C],
            KeyCode::Kp8 => &[0x75],
            KeyCode::Kp9 => &[0x7D],
            KeyCode::Kp0 => &[0x70],
            KeyCode::KpPeriod => &[0x71],
            KeyCode::LeftControl => &[0x14],
            KeyCode::LeftShift => &[0x12],
            KeyCode::LeftAlt => &[0x11],
            KeyCode::LeftGui => &[0xE0, 0x1F],
            KeyCode::RightControl => &[0xE0, 0x14],
            KeyCode::RightShift => &[0x59],
            KeyCode::RightAlt => &[0xE0, 0x11],
            KeyCode::RightGui => &[0xE0, 0x27],
        }
    }

    /// Get scan code set 3 make code
    pub fn set3_make(&self) -> &'static [u8] {
        match self {
            KeyCode::A => &[0x1C],
            KeyCode::B => &[0x32],
            KeyCode::C => &[0x21],
            KeyCode::D => &[0x23],
            KeyCode::E => &[0x24],
            KeyCode::F => &[0x2B],
            KeyCode::G => &[0x34],
            KeyCode::H => &[0x33],
            KeyCode::I => &[0x43],
            KeyCode::J => &[0x3B],
            KeyCode::K => &[0x42],
            KeyCode::L => &[0x4B],
            KeyCode::M => &[0x3A],
            KeyCode::N => &[0x31],
            KeyCode::O => &[0x44],
            KeyCode::P => &[0x4D],
            KeyCode::Q => &[0x15],
            KeyCode::R => &[0x2D],
            KeyCode::S => &[0x1B],
            KeyCode::T => &[0x2C],
            KeyCode::U => &[0x3C],
            KeyCode::V => &[0x2A],
            KeyCode::W => &[0x1D],
            KeyCode::X => &[0x22],
            KeyCode::Y => &[0x35],
            KeyCode::Z => &[0x1A],
            KeyCode::Num1 => &[0x16],
            KeyCode::Num2 => &[0x1E],
            KeyCode::Num3 => &[0x26],
            KeyCode::Num4 => &[0x25],
            KeyCode::Num5 => &[0x2E],
            KeyCode::Num6 => &[0x36],
            KeyCode::Num7 => &[0x3D],
            KeyCode::Num8 => &[0x3E],
            KeyCode::Num9 => &[0x46],
            KeyCode::Num0 => &[0x45],
            KeyCode::Enter => &[0x5A],
            KeyCode::Escape => &[0x08],
            KeyCode::Backspace => &[0x66],
            KeyCode::Tab => &[0x0D],
            KeyCode::Space => &[0x29],
            KeyCode::Minus => &[0x4E],
            KeyCode::Equal => &[0x55],
            KeyCode::LeftBracket => &[0x54],
            KeyCode::RightBracket => &[0x5B],
            KeyCode::Backslash => &[0x5C],
            KeyCode::Semicolon => &[0x4C],
            KeyCode::Quote => &[0x52],
            KeyCode::Grave => &[0x0E],
            KeyCode::Comma => &[0x41],
            KeyCode::Period => &[0x49],
            KeyCode::Slash => &[0x4A],
            KeyCode::CapsLock => &[0x14],
            KeyCode::F1 => &[0x07],
            KeyCode::F2 => &[0x0F],
            KeyCode::F3 => &[0x17],
            KeyCode::F4 => &[0x1F],
            KeyCode::F5 => &[0x27],
            KeyCode::F6 => &[0x2F],
            KeyCode::F7 => &[0x37],
            KeyCode::F8 => &[0x3F],
            KeyCode::F9 => &[0x47],
            KeyCode::F10 => &[0x4F],
            KeyCode::F11 => &[0x56],
            KeyCode::F12 => &[0x5E],
            KeyCode::PrintScreen => &[0x57],
            KeyCode::ScrollLock => &[0x5F],
            KeyCode::Pause => &[0x62],
            KeyCode::Insert => &[0x67],
            KeyCode::Home => &[0x6E],
            KeyCode::PageUp => &[0x6F],
            KeyCode::Delete => &[0x64],
            KeyCode::End => &[0x65],
            KeyCode::PageDown => &[0x6D],
            KeyCode::Right => &[0x6A],
            KeyCode::Left => &[0x61],
            KeyCode::Down => &[0x60],
            KeyCode::Up => &[0x63],
            KeyCode::NumLock => &[0x76],
            KeyCode::KpDivide => &[0x77],
            KeyCode::KpMultiply => &[0x7E],
            KeyCode::KpMinus => &[0x84],
            KeyCode::KpPlus => &[0x7C],
            KeyCode::KpEnter => &[0x79],
            KeyCode::Kp1 => &[0x69],
            KeyCode::Kp2 => &[0x72],
            KeyCode::Kp3 => &[0x7A],
            KeyCode::Kp4 => &[0x6B],
            KeyCode::Kp5 => &[0x73],
            KeyCode::Kp6 => &[0x74],
            KeyCode::Kp7 => &[0x6C],
            KeyCode::Kp8 => &[0x75],
            KeyCode::Kp9 => &[0x7D],
            KeyCode::Kp0 => &[0x70],
            KeyCode::KpPeriod => &[0x71],
            KeyCode::LeftControl => &[0x11],
            KeyCode::LeftShift => &[0x12],
            KeyCode::LeftAlt => &[0x19],
            KeyCode::LeftGui => &[0x8B],
            KeyCode::RightControl => &[0x58],
            KeyCode::RightShift => &[0x59],
            KeyCode::RightAlt => &[0x39],
            KeyCode::RightGui => &[0x8C],
        }
    }
}

/// Keyboard LED state
#[derive(Debug, Clone, Copy, Default)]
pub struct LedState {
    /// Scroll Lock LED
    pub scroll_lock: bool,
    /// Num Lock LED
    pub num_lock: bool,
    /// Caps Lock LED
    pub caps_lock: bool,
}

impl LedState {
    /// Create from byte value
    pub fn from_byte(value: u8) -> Self {
        Self {
            scroll_lock: value & 0x01 != 0,
            num_lock: value & 0x02 != 0,
            caps_lock: value & 0x04 != 0,
        }
    }

    /// Convert to byte value
    pub fn to_byte(&self) -> u8 {
        let mut value = 0u8;
        if self.scroll_lock {
            value |= 0x01;
        }
        if self.num_lock {
            value |= 0x02;
        }
        if self.caps_lock {
            value |= 0x04;
        }
        value
    }
}

/// Typematic rate and delay configuration
#[derive(Debug, Clone, Copy)]
pub struct TypematicConfig {
    /// Repeat rate (characters per second)
    pub rate: f32,
    /// Initial delay (milliseconds)
    pub delay_ms: u16,
}

impl Default for TypematicConfig {
    fn default() -> Self {
        Self {
            rate: 10.9,    // ~10.9 chars/sec default
            delay_ms: 500, // 500ms default delay
        }
    }
}

impl TypematicConfig {
    /// Create from PS/2 command byte
    pub fn from_command(value: u8) -> Self {
        // Bits 0-4: repeat rate (0=30.0 cps, 31=2.0 cps)
        // Bits 5-6: delay (0=250ms, 1=500ms, 2=750ms, 3=1000ms)
        let rate_code = value & 0x1F;
        let delay_code = (value >> 5) & 0x03;

        // Rate formula: (8 + (rate_code & 0x07)) * 2^((rate_code >> 3) & 0x03) * 0.00417
        let a = (rate_code & 0x07) as f32 + 8.0;
        let b = 1 << ((rate_code >> 3) & 0x03);
        let period = a * b as f32 * 0.00417;
        let rate = 1.0 / period;

        let delay_ms = match delay_code {
            0 => 250,
            1 => 500,
            2 => 750,
            _ => 1000,
        };

        Self { rate, delay_ms }
    }

    /// Convert to PS/2 command byte
    pub fn to_command(&self) -> u8 {
        // Find closest rate code
        let mut best_code = 0u8;
        let mut best_diff = f32::MAX;

        for code in 0..32u8 {
            let test = Self::from_command(code);
            let diff = (test.rate - self.rate).abs();
            if diff < best_diff {
                best_diff = diff;
                best_code = code;
            }
        }

        // Find delay code
        let delay_code = match self.delay_ms {
            0..=375 => 0,
            376..=625 => 1,
            626..=875 => 2,
            _ => 3,
        };

        best_code | (delay_code << 5)
    }
}

/// PS/2 keyboard commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ps2Command {
    /// Set LEDs
    SetLeds = 0xED,
    /// Echo
    Echo = 0xEE,
    /// Get/Set scan code set
    ScanCodeSet = 0xF0,
    /// Identify keyboard
    Identify = 0xF2,
    /// Set typematic rate/delay
    SetTypematic = 0xF3,
    /// Enable scanning
    Enable = 0xF4,
    /// Disable scanning
    Disable = 0xF5,
    /// Set default parameters
    SetDefaults = 0xF6,
    /// Set all keys typematic (set 3)
    SetAllTypematic = 0xF7,
    /// Set all keys make/break (set 3)
    SetAllMakeBreak = 0xF8,
    /// Set all keys make only (set 3)
    SetAllMakeOnly = 0xF9,
    /// Set all keys typematic/make/break (set 3)
    SetAllTypematicMakeBreak = 0xFA,
    /// Set key typematic (set 3)
    SetKeyTypematic = 0xFB,
    /// Set key make/break (set 3)
    SetKeyMakeBreak = 0xFC,
    /// Set key make only (set 3)
    SetKeyMakeOnly = 0xFD,
    /// Resend
    Resend = 0xFE,
    /// Reset
    Reset = 0xFF,
}

impl Ps2Command {
    /// Create from byte
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0xED => Some(Ps2Command::SetLeds),
            0xEE => Some(Ps2Command::Echo),
            0xF0 => Some(Ps2Command::ScanCodeSet),
            0xF2 => Some(Ps2Command::Identify),
            0xF3 => Some(Ps2Command::SetTypematic),
            0xF4 => Some(Ps2Command::Enable),
            0xF5 => Some(Ps2Command::Disable),
            0xF6 => Some(Ps2Command::SetDefaults),
            0xF7 => Some(Ps2Command::SetAllTypematic),
            0xF8 => Some(Ps2Command::SetAllMakeBreak),
            0xF9 => Some(Ps2Command::SetAllMakeOnly),
            0xFA => Some(Ps2Command::SetAllTypematicMakeBreak),
            0xFB => Some(Ps2Command::SetKeyTypematic),
            0xFC => Some(Ps2Command::SetKeyMakeBreak),
            0xFD => Some(Ps2Command::SetKeyMakeOnly),
            0xFE => Some(Ps2Command::Resend),
            0xFF => Some(Ps2Command::Reset),
            _ => None,
        }
    }
}

/// PS/2 response codes
pub mod Response {
    /// Acknowledgement
    pub const ACK: u8 = 0xFA;
    /// Resend request
    pub const RESEND: u8 = 0xFE;
    /// Echo
    pub const ECHO: u8 = 0xEE;
    /// Self-test passed
    pub const BAT_OK: u8 = 0xAA;
    /// Self-test failed
    pub const BAT_FAIL: u8 = 0xFC;
    /// Keyboard ID byte 1
    pub const ID1: u8 = 0xAB;
    /// Keyboard ID byte 2 (MF2)
    pub const ID2_MF2: u8 = 0x83;
    /// Keyboard ID byte 2 (short)
    pub const ID2_SHORT: u8 = 0x41;
}

/// Command state machine
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CommandState {
    /// Ready for command
    #[default]
    Idle,
    /// Waiting for LED data
    WaitingLedData,
    /// Waiting for scan code set
    WaitingScanCodeSet,
    /// Waiting for typematic data
    WaitingTypematic,
    /// Waiting for key (set 3 commands)
    WaitingKey,
}

/// Keyboard statistics
#[derive(Debug, Default)]
pub struct KeyboardStats {
    /// Keys pressed
    keys_pressed: AtomicU64,
    /// Keys released
    keys_released: AtomicU64,
    /// Commands received
    commands_received: AtomicU64,
    /// Bytes output
    bytes_output: AtomicU64,
}

impl KeyboardStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record key press
    pub fn record_press(&self) {
        self.keys_pressed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record key release
    pub fn record_release(&self) {
        self.keys_released.fetch_add(1, Ordering::Relaxed);
    }

    /// Record command
    pub fn record_command(&self) {
        self.commands_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Record output bytes
    pub fn record_output(&self, count: u64) {
        self.bytes_output.fetch_add(count, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> KeyboardStatsSnapshot {
        KeyboardStatsSnapshot {
            keys_pressed: self.keys_pressed.load(Ordering::Relaxed),
            keys_released: self.keys_released.load(Ordering::Relaxed),
            commands_received: self.commands_received.load(Ordering::Relaxed),
            bytes_output: self.bytes_output.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct KeyboardStatsSnapshot {
    /// Keys pressed
    pub keys_pressed: u64,
    /// Keys released
    pub keys_released: u64,
    /// Commands received
    pub commands_received: u64,
    /// Bytes output
    pub bytes_output: u64,
}

/// PS/2 Keyboard device
pub struct Ps2Keyboard {
    /// Current scan code set
    scan_code_set: ScanCodeSet,
    /// LED state
    leds: LedState,
    /// Typematic configuration
    typematic: TypematicConfig,
    /// Scanning enabled
    enabled: bool,
    /// Output buffer
    output_buffer: VecDeque<u8>,
    /// Last output (for resend)
    last_output: Vec<u8>,
    /// Command state
    command_state: CommandState,
    /// Pending set 3 command
    pending_set3_command: Option<Ps2Command>,
    /// Statistics
    stats: KeyboardStats,
    /// Interrupt pending
    interrupt_pending: bool,
}

impl Default for Ps2Keyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Ps2Keyboard {
    /// Create new keyboard
    pub fn new() -> Self {
        Self {
            scan_code_set: ScanCodeSet::Set2,
            leds: LedState::default(),
            typematic: TypematicConfig::default(),
            enabled: true,
            output_buffer: VecDeque::with_capacity(64),
            last_output: Vec::new(),
            command_state: CommandState::Idle,
            pending_set3_command: None,
            stats: KeyboardStats::new(),
            interrupt_pending: false,
        }
    }

    /// Get current scan code set
    pub fn scan_code_set(&self) -> ScanCodeSet {
        self.scan_code_set
    }

    /// Get LED state
    pub fn leds(&self) -> LedState {
        self.leds
    }

    /// Get typematic config
    pub fn typematic(&self) -> TypematicConfig {
        self.typematic
    }

    /// Check if scanning is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get statistics
    pub fn stats(&self) -> &KeyboardStats {
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

    /// Write command/data to keyboard
    pub fn write(&mut self, value: u8) {
        self.stats.record_command();

        match self.command_state {
            CommandState::Idle => self.handle_command(value),
            CommandState::WaitingLedData => {
                self.leds = LedState::from_byte(value);
                self.send_ack();
                self.command_state = CommandState::Idle;
            }
            CommandState::WaitingScanCodeSet => {
                if value == 0 {
                    // Return current scan code set
                    self.send_ack();
                    self.send_byte(self.scan_code_set as u8);
                } else if let Some(set) = ScanCodeSet::from_u8(value) {
                    self.scan_code_set = set;
                    self.send_ack();
                } else {
                    self.send_byte(Response::RESEND);
                }
                self.command_state = CommandState::Idle;
            }
            CommandState::WaitingTypematic => {
                self.typematic = TypematicConfig::from_command(value);
                self.send_ack();
                self.command_state = CommandState::Idle;
            }
            CommandState::WaitingKey => {
                // Set 3 per-key commands - acknowledge but don't implement per-key tracking
                self.send_ack();
                self.command_state = CommandState::Idle;
            }
        }
    }

    /// Handle command byte
    fn handle_command(&mut self, value: u8) {
        if let Some(cmd) = Ps2Command::from_byte(value) {
            match cmd {
                Ps2Command::SetLeds => {
                    self.send_ack();
                    self.command_state = CommandState::WaitingLedData;
                }
                Ps2Command::Echo => {
                    self.send_byte(Response::ECHO);
                }
                Ps2Command::ScanCodeSet => {
                    self.send_ack();
                    self.command_state = CommandState::WaitingScanCodeSet;
                }
                Ps2Command::Identify => {
                    self.send_ack();
                    self.send_byte(Response::ID1);
                    self.send_byte(Response::ID2_MF2);
                }
                Ps2Command::SetTypematic => {
                    self.send_ack();
                    self.command_state = CommandState::WaitingTypematic;
                }
                Ps2Command::Enable => {
                    self.enabled = true;
                    self.send_ack();
                }
                Ps2Command::Disable => {
                    self.enabled = false;
                    self.send_ack();
                }
                Ps2Command::SetDefaults => {
                    self.scan_code_set = ScanCodeSet::Set2;
                    self.typematic = TypematicConfig::default();
                    self.enabled = true;
                    self.send_ack();
                }
                Ps2Command::SetAllTypematic
                | Ps2Command::SetAllMakeBreak
                | Ps2Command::SetAllMakeOnly
                | Ps2Command::SetAllTypematicMakeBreak => {
                    // Set 3 all-keys commands
                    self.send_ack();
                }
                Ps2Command::SetKeyTypematic
                | Ps2Command::SetKeyMakeBreak
                | Ps2Command::SetKeyMakeOnly => {
                    self.send_ack();
                    self.pending_set3_command = Some(cmd);
                    self.command_state = CommandState::WaitingKey;
                }
                Ps2Command::Resend => {
                    // Resend last output
                    for &byte in &self.last_output {
                        self.output_buffer.push_back(byte);
                    }
                    if !self.output_buffer.is_empty() {
                        self.interrupt_pending = true;
                    }
                }
                Ps2Command::Reset => {
                    self.reset();
                    self.send_ack();
                    self.send_byte(Response::BAT_OK);
                }
            }
        }
    }

    /// Reset keyboard to defaults
    pub fn reset(&mut self) {
        self.scan_code_set = ScanCodeSet::Set2;
        self.leds = LedState::default();
        self.typematic = TypematicConfig::default();
        self.enabled = true;
        self.output_buffer.clear();
        self.last_output.clear();
        self.command_state = CommandState::Idle;
        self.pending_set3_command = None;
    }

    /// Send acknowledgement
    fn send_ack(&mut self) {
        self.send_byte(Response::ACK);
    }

    /// Send byte to output buffer
    fn send_byte(&mut self, value: u8) {
        self.output_buffer.push_back(value);
        self.interrupt_pending = true;
        self.stats.record_output(1);
    }

    /// Key press event
    pub fn key_press(&mut self, key: KeyCode) {
        if !self.enabled {
            return;
        }

        self.stats.record_press();
        self.last_output.clear();

        let codes = match self.scan_code_set {
            ScanCodeSet::Set1 => key.set1_make(),
            ScanCodeSet::Set2 => key.set2_make(),
            ScanCodeSet::Set3 => key.set3_make(),
        };

        for &code in codes {
            self.output_buffer.push_back(code);
            self.last_output.push(code);
        }

        if !self.output_buffer.is_empty() {
            self.interrupt_pending = true;
            self.stats.record_output(codes.len() as u64);
        }
    }

    /// Key release event
    pub fn key_release(&mut self, key: KeyCode) {
        if !self.enabled {
            return;
        }

        self.stats.record_release();
        self.last_output.clear();

        match self.scan_code_set {
            ScanCodeSet::Set1 => {
                // Set 1: break code = make code | 0x80
                let codes = key.set1_make();
                for &code in codes {
                    let break_code = if code == 0xE0 || code == 0xE1 {
                        code
                    } else {
                        code | 0x80
                    };
                    self.output_buffer.push_back(break_code);
                    self.last_output.push(break_code);
                }
            }
            ScanCodeSet::Set2 => {
                // Set 2: break code = F0 + make code
                let codes = key.set2_make();
                let mut sent = false;
                for &code in codes {
                    if code == 0xE0 || code == 0xE1 {
                        self.output_buffer.push_back(code);
                        self.last_output.push(code);
                    } else {
                        if !sent {
                            self.output_buffer.push_back(0xF0);
                            self.last_output.push(0xF0);
                            sent = true;
                        }
                        self.output_buffer.push_back(code);
                        self.last_output.push(code);
                    }
                }
            }
            ScanCodeSet::Set3 => {
                // Set 3: break code = F0 + make code
                let codes = key.set3_make();
                self.output_buffer.push_back(0xF0);
                self.last_output.push(0xF0);
                for &code in codes {
                    self.output_buffer.push_back(code);
                    self.last_output.push(code);
                }
            }
        }

        if !self.output_buffer.is_empty() {
            self.interrupt_pending = true;
            self.stats.record_output(self.last_output.len() as u64);
        }
    }

    /// Type a string (press and release each key)
    pub fn type_string(&mut self, text: &str) {
        for c in text.chars() {
            if let Some(key) = char_to_keycode(c) {
                self.key_press(key);
                self.key_release(key);
            }
        }
    }
}

/// Convert character to key code
pub fn char_to_keycode(c: char) -> Option<KeyCode> {
    match c.to_ascii_lowercase() {
        'a' => Some(KeyCode::A),
        'b' => Some(KeyCode::B),
        'c' => Some(KeyCode::C),
        'd' => Some(KeyCode::D),
        'e' => Some(KeyCode::E),
        'f' => Some(KeyCode::F),
        'g' => Some(KeyCode::G),
        'h' => Some(KeyCode::H),
        'i' => Some(KeyCode::I),
        'j' => Some(KeyCode::J),
        'k' => Some(KeyCode::K),
        'l' => Some(KeyCode::L),
        'm' => Some(KeyCode::M),
        'n' => Some(KeyCode::N),
        'o' => Some(KeyCode::O),
        'p' => Some(KeyCode::P),
        'q' => Some(KeyCode::Q),
        'r' => Some(KeyCode::R),
        's' => Some(KeyCode::S),
        't' => Some(KeyCode::T),
        'u' => Some(KeyCode::U),
        'v' => Some(KeyCode::V),
        'w' => Some(KeyCode::W),
        'x' => Some(KeyCode::X),
        'y' => Some(KeyCode::Y),
        'z' => Some(KeyCode::Z),
        '1' => Some(KeyCode::Num1),
        '2' => Some(KeyCode::Num2),
        '3' => Some(KeyCode::Num3),
        '4' => Some(KeyCode::Num4),
        '5' => Some(KeyCode::Num5),
        '6' => Some(KeyCode::Num6),
        '7' => Some(KeyCode::Num7),
        '8' => Some(KeyCode::Num8),
        '9' => Some(KeyCode::Num9),
        '0' => Some(KeyCode::Num0),
        ' ' => Some(KeyCode::Space),
        '\n' => Some(KeyCode::Enter),
        '\t' => Some(KeyCode::Tab),
        '-' => Some(KeyCode::Minus),
        '=' => Some(KeyCode::Equal),
        '[' => Some(KeyCode::LeftBracket),
        ']' => Some(KeyCode::RightBracket),
        '\\' => Some(KeyCode::Backslash),
        ';' => Some(KeyCode::Semicolon),
        '\'' => Some(KeyCode::Quote),
        '`' => Some(KeyCode::Grave),
        ',' => Some(KeyCode::Comma),
        '.' => Some(KeyCode::Period),
        '/' => Some(KeyCode::Slash),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_code_set_from_u8() {
        assert_eq!(ScanCodeSet::from_u8(1), Some(ScanCodeSet::Set1));
        assert_eq!(ScanCodeSet::from_u8(2), Some(ScanCodeSet::Set2));
        assert_eq!(ScanCodeSet::from_u8(3), Some(ScanCodeSet::Set3));
        assert_eq!(ScanCodeSet::from_u8(4), None);
    }

    #[test]
    fn test_key_code_set1_make() {
        assert_eq!(KeyCode::A.set1_make(), &[0x1E]);
        assert_eq!(KeyCode::Enter.set1_make(), &[0x1C]);
        assert_eq!(KeyCode::Escape.set1_make(), &[0x01]);
    }

    #[test]
    fn test_key_code_set2_make() {
        assert_eq!(KeyCode::A.set2_make(), &[0x1C]);
        assert_eq!(KeyCode::Enter.set2_make(), &[0x5A]);
        assert_eq!(KeyCode::Escape.set2_make(), &[0x76]);
    }

    #[test]
    fn test_key_code_set3_make() {
        assert_eq!(KeyCode::A.set3_make(), &[0x1C]);
        assert_eq!(KeyCode::Enter.set3_make(), &[0x5A]);
        assert_eq!(KeyCode::Escape.set3_make(), &[0x08]);
    }

    #[test]
    fn test_led_state() {
        let leds = LedState::from_byte(0x07);
        assert!(leds.scroll_lock);
        assert!(leds.num_lock);
        assert!(leds.caps_lock);

        let leds = LedState::from_byte(0x02);
        assert!(!leds.scroll_lock);
        assert!(leds.num_lock);
        assert!(!leds.caps_lock);

        let leds = LedState {
            scroll_lock: true,
            num_lock: false,
            caps_lock: true,
        };
        assert_eq!(leds.to_byte(), 0x05);
    }

    #[test]
    fn test_typematic_config() {
        let config = TypematicConfig::default();
        assert!(config.rate > 10.0 && config.rate < 12.0);
        assert_eq!(config.delay_ms, 500);

        // Test from command
        let config = TypematicConfig::from_command(0x00);
        assert!(config.rate > 25.0); // Fastest rate
        assert_eq!(config.delay_ms, 250);

        let config = TypematicConfig::from_command(0x7F);
        assert!(config.rate < 3.0); // Slowest rate
        assert_eq!(config.delay_ms, 1000);
    }

    #[test]
    fn test_keyboard_creation() {
        let kbd = Ps2Keyboard::new();
        assert_eq!(kbd.scan_code_set(), ScanCodeSet::Set2);
        assert!(kbd.is_enabled());
        assert!(!kbd.has_data());
    }

    #[test]
    fn test_keyboard_key_press_set2() {
        let mut kbd = Ps2Keyboard::new();
        kbd.key_press(KeyCode::A);

        assert!(kbd.has_data());
        assert_eq!(kbd.read(), Some(0x1C));
        assert!(!kbd.has_data());
    }

    #[test]
    fn test_keyboard_key_release_set2() {
        let mut kbd = Ps2Keyboard::new();
        kbd.key_release(KeyCode::A);

        assert!(kbd.has_data());
        assert_eq!(kbd.read(), Some(0xF0)); // Break prefix
        assert_eq!(kbd.read(), Some(0x1C)); // Key code
        assert!(!kbd.has_data());
    }

    #[test]
    fn test_keyboard_key_press_set1() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xF0); // Scan code set command
        kbd.read(); // ACK
        kbd.write(0x01); // Set 1
        kbd.read(); // ACK

        kbd.key_press(KeyCode::A);
        assert_eq!(kbd.read(), Some(0x1E));
    }

    #[test]
    fn test_keyboard_key_release_set1() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xF0);
        kbd.read();
        kbd.write(0x01);
        kbd.read();

        kbd.key_release(KeyCode::A);
        assert_eq!(kbd.read(), Some(0x9E)); // 0x1E | 0x80
    }

    #[test]
    fn test_keyboard_echo() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xEE); // Echo
        assert_eq!(kbd.read(), Some(0xEE));
    }

    #[test]
    fn test_keyboard_identify() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xF2); // Identify
        assert_eq!(kbd.read(), Some(Response::ACK));
        assert_eq!(kbd.read(), Some(Response::ID1));
        assert_eq!(kbd.read(), Some(Response::ID2_MF2));
    }

    #[test]
    fn test_keyboard_set_leds() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xED); // Set LEDs
        assert_eq!(kbd.read(), Some(Response::ACK));
        kbd.write(0x07); // All LEDs on
        assert_eq!(kbd.read(), Some(Response::ACK));

        let leds = kbd.leds();
        assert!(leds.scroll_lock);
        assert!(leds.num_lock);
        assert!(leds.caps_lock);
    }

    #[test]
    fn test_keyboard_enable_disable() {
        let mut kbd = Ps2Keyboard::new();

        kbd.write(0xF5); // Disable
        kbd.read();
        assert!(!kbd.is_enabled());

        kbd.key_press(KeyCode::A);
        assert!(!kbd.has_data()); // No output when disabled

        kbd.write(0xF4); // Enable
        kbd.read();
        assert!(kbd.is_enabled());

        kbd.key_press(KeyCode::A);
        assert!(kbd.has_data());
    }

    #[test]
    fn test_keyboard_reset() {
        let mut kbd = Ps2Keyboard::new();

        // Change some settings
        kbd.write(0xF0);
        kbd.read();
        kbd.write(0x01);
        kbd.read();

        // Reset
        kbd.write(0xFF);
        assert_eq!(kbd.read(), Some(Response::ACK));
        assert_eq!(kbd.read(), Some(Response::BAT_OK));

        // Should be back to defaults
        assert_eq!(kbd.scan_code_set(), ScanCodeSet::Set2);
    }

    #[test]
    fn test_keyboard_set_defaults() {
        let mut kbd = Ps2Keyboard::new();

        // Change settings
        kbd.write(0xF0);
        kbd.read();
        kbd.write(0x03);
        kbd.read();
        kbd.write(0xF5);
        kbd.read();

        // Set defaults
        kbd.write(0xF6);
        assert_eq!(kbd.read(), Some(Response::ACK));

        assert_eq!(kbd.scan_code_set(), ScanCodeSet::Set2);
        assert!(kbd.is_enabled());
    }

    #[test]
    fn test_keyboard_get_scan_code_set() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xF0); // Scan code set command
        kbd.read(); // ACK
        kbd.write(0x00); // Get current
        assert_eq!(kbd.read(), Some(Response::ACK));
        assert_eq!(kbd.read(), Some(2)); // Default is set 2
    }

    #[test]
    fn test_keyboard_extended_key_set2() {
        let mut kbd = Ps2Keyboard::new();
        kbd.key_press(KeyCode::Right);

        assert_eq!(kbd.read(), Some(0xE0));
        assert_eq!(kbd.read(), Some(0x74));
    }

    #[test]
    fn test_keyboard_stats() {
        let mut kbd = Ps2Keyboard::new();
        kbd.key_press(KeyCode::A);
        kbd.key_release(KeyCode::A);
        kbd.write(0xEE);
        kbd.read();

        let stats = kbd.stats().snapshot();
        assert_eq!(stats.keys_pressed, 1);
        assert_eq!(stats.keys_released, 1);
        assert!(stats.commands_received > 0);
    }

    #[test]
    fn test_keyboard_interrupt() {
        let mut kbd = Ps2Keyboard::new();
        assert!(!kbd.has_interrupt());

        kbd.key_press(KeyCode::A);
        assert!(kbd.has_interrupt());

        kbd.read();
        assert!(!kbd.has_interrupt());
    }

    #[test]
    fn test_keyboard_type_string() {
        let mut kbd = Ps2Keyboard::new();
        kbd.type_string("ab");

        // Should have press/release for 'a' and 'b'
        assert!(kbd.output_len() >= 4);
    }

    #[test]
    fn test_char_to_keycode() {
        assert_eq!(char_to_keycode('a'), Some(KeyCode::A));
        assert_eq!(char_to_keycode('A'), Some(KeyCode::A));
        assert_eq!(char_to_keycode('1'), Some(KeyCode::Num1));
        assert_eq!(char_to_keycode(' '), Some(KeyCode::Space));
        assert_eq!(char_to_keycode('\n'), Some(KeyCode::Enter));
        assert_eq!(char_to_keycode('!'), None); // Shifted character
    }

    #[test]
    fn test_keyboard_set_typematic() {
        let mut kbd = Ps2Keyboard::new();
        kbd.write(0xF3); // Set typematic
        assert_eq!(kbd.read(), Some(Response::ACK));
        kbd.write(0x00); // Fastest rate, shortest delay
        assert_eq!(kbd.read(), Some(Response::ACK));

        let config = kbd.typematic();
        assert!(config.rate > 25.0);
        assert_eq!(config.delay_ms, 250);
    }
}
