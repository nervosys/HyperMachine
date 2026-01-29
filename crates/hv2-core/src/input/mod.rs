//! Input Devices Module
//!
//! This module provides comprehensive input device emulation for the hypervisor,
//! including PS/2 keyboard, PS/2 mouse, touchscreen, and game controller support.

pub mod gamepad;
pub mod ps2_keyboard;
pub mod ps2_mouse;
pub mod touchscreen;

pub use gamepad::{
    Axis, Button, ControllerEvent, ControllerState, ControllerStats, ControllerType,
    DeadzoneConfig, GameController, RumbleEffect,
};
pub use ps2_keyboard::{
    KeyCode, KeyboardStats, LedState, Ps2Command, Ps2Keyboard, ScanCodeSet, TypematicConfig,
};
pub use ps2_mouse::{
    MouseButtons, MouseCommand, MouseProtocol, MouseResolution, MouseStats, Ps2Mouse, SampleRate,
    ScalingMode,
};
pub use touchscreen::{
    Gesture, GestureType, TouchConfig, TouchEvent, TouchPoint, TouchState, TouchStats, Touchscreen,
};
