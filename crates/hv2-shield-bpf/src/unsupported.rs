//! The same surface on a host that cannot do it: every entry point says so.

use crate::{HookCount, HookRule};

const WHY: &str = "BPF LSM shields need Linux on x86_64 or aarch64";

/// Whether this host can load and attach a BPF LSM program. It cannot.
pub fn probe() -> Result<(), String> {
    Err(WHY.into())
}

/// The hooks this kernel has. None: there is no such kernel here.
pub fn available_hooks() -> Result<Vec<&'static str>, String> {
    Err(WHY.into())
}

/// The engine. It cannot be made on this host.
#[derive(Debug)]
pub struct HookEngine {
    _never: std::convert::Infallible,
}

impl HookEngine {
    /// Always an error here.
    pub fn new() -> Result<Self, String> {
        Err(WHY.into())
    }

    /// Never reached: there is no engine.
    pub fn exempt(&self, _tgid: u32) -> Result<(), String> {
        match self._never {}
    }

    /// Never reached: there is no engine.
    pub fn unexempt(&self, _tgid: u32) -> Result<(), String> {
        match self._never {}
    }

    /// Never reached: there is no engine.
    pub fn attached_hooks(&self) -> Vec<&'static str> {
        match self._never {}
    }

    /// Never reached: there is no engine.
    pub fn set(&mut self, _rules: &[HookRule]) -> Result<(), String> {
        match self._never {}
    }

    /// Never reached: there is no engine.
    pub fn counts(&self) -> Result<Vec<HookCount>, String> {
        match self._never {}
    }
}
