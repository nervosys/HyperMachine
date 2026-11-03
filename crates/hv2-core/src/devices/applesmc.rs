//! The System Management Controller of an Apple machine, as a guest sees it
//! on ports 0x300-0x31F.
//!
//! This implements the register protocol only: a command port and a data
//! port, with keys the guest reads by four-character name. It is the same
//! protocol QEMU's `applesmc` speaks.
//!
//! It does **not** contain the 64-byte "OSK" string the guest's boot code
//! reads from keys `OSK0` and `OSK1`. Nothing in this repository carries it.
//! The operator supplies it ([`AppleSmc::with_osk`]); without one those keys
//! are absent and a read of them fails, as on a machine that has none.
//!
//! Status: built, not released. No macOS guest has been booted with this, so
//! whether the keys below are enough for any macOS version is NOT VERIFIED.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::device::{Device, DeviceType};
use crate::Result;

/// The first port of the controller.
pub const APPLE_SMC_BASE: u16 = 0x300;
/// The last port of the 32-port window.
pub const APPLE_SMC_LAST: u16 = 0x31F;

const DATA_PORT: u64 = 0x00;
const COMMAND_PORT: u64 = 0x04;
const ERROR_PORT: u64 = 0x1E;

const CMD_READ: u8 = 0x10;
const CMD_WRITE: u8 = 0x11;
const CMD_KEY_BY_INDEX: u8 = 0x12;
const CMD_KEY_TYPE: u8 = 0x13;

/// Status: a command was accepted and the controller wants its key.
const STATUS_ACCEPTED: u8 = 0x0C;
/// Status: bytes of a key are still being taken.
const STATUS_KEY: u8 = 0x04;
/// Status: data is ready to read (or to write).
const STATUS_DATA: u8 = 0x05;
/// Status: idle.
const STATUS_IDLE: u8 = 0x00;

/// The length of the operator-supplied key, split over `OSK0` and `OSK1`.
pub const OSK_LEN: usize = 64;

#[derive(Debug, Clone)]
struct Key {
    type_name: [u8; 4],
    data: Vec<u8>,
    writable: bool,
}

/// The SMC register protocol over a table of keys.
#[derive(Debug)]
struct Inner {
    keys: BTreeMap<[u8; 4], Key>,
    command: u8,
    status: u8,
    /// Bytes of the key name (or index) received so far, then the length.
    received: usize,
    key_name: [u8; 4],
    length: usize,
    /// Position within the data being read or written.
    position: usize,
    /// What a read hands back, prepared when the key is complete.
    output: Vec<u8>,
    error: u8,
}

impl Inner {
    /// A controller with the keys that are not secret, and no OSK.
    #[must_use]
    pub fn new() -> Self {
        let mut smc = Self {
            keys: BTreeMap::new(),
            command: 0,
            status: STATUS_IDLE,
            received: 0,
            key_name: [0; 4],
            length: 0,
            position: 0,
            output: Vec::new(),
            error: 0,
        };
        smc.insert(
            b"REV ",
            b"{rev",
            vec![0x01, 0x13, 0x0F, 0x00, 0x00, 0x03],
            false,
        );
        smc.insert(b"$Num", b"ui8 ", vec![1], false);
        smc.insert(b"NATJ", b"ui8 ", vec![0], true);
        smc.insert(b"MSSP", b"ui8 ", vec![0], false);
        smc.insert(b"MSSD", b"si8 ", vec![3], false);
        smc.insert(b"NTOK", b"ui8 ", vec![0], false);
        smc
    }

    /// The same, with the operator's 64-byte OSK installed as `OSK0` (the
    /// first 32 bytes) and `OSK1` (the rest).
    #[must_use]
    pub fn with_osk(osk: &[u8; OSK_LEN]) -> Self {
        let mut smc = Self::new();
        smc.insert(b"OSK0", b"ch8*", osk[..32].to_vec(), false);
        smc.insert(b"OSK1", b"ch8*", osk[32..].to_vec(), false);
        smc
    }

    fn insert(&mut self, name: &[u8; 4], type_name: &[u8; 4], data: Vec<u8>, writable: bool) {
        self.keys.insert(
            *name,
            Key {
                type_name: *type_name,
                data,
                writable,
            },
        );
    }

    /// The number of keys, which the guest reads as `#KEY` (big-endian).
    fn key_count(&self) -> u32 {
        // `#KEY` counts itself.
        u32::try_from(self.keys.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1)
    }

    fn lookup(&self, name: [u8; 4]) -> Option<Vec<u8>> {
        if &name == b"#KEY" {
            return Some(self.key_count().to_be_bytes().to_vec());
        }
        self.keys.get(&name).map(|key| key.data.clone())
    }

    fn begin(&mut self, command: u8) {
        self.command = command;
        self.status = STATUS_ACCEPTED;
        self.received = 0;
        self.length = 0;
        self.position = 0;
        self.output.clear();
        self.error = 0;
    }

    fn data_write(&mut self, value: u8) {
        match self.command {
            CMD_READ | CMD_WRITE | CMD_KEY_TYPE => {
                if self.received < 4 {
                    self.key_name[self.received] = value;
                    self.status = STATUS_KEY;
                    self.received += 1;
                    if self.received == 4 && self.command == CMD_KEY_TYPE {
                        self.prepare_type();
                    }
                } else if self.received == 4 && self.command != CMD_KEY_TYPE {
                    self.length = usize::from(value);
                    self.position = 0;
                    self.received += 1;
                    self.status = STATUS_DATA;
                    if self.command == CMD_READ {
                        match self.lookup(self.key_name) {
                            Some(mut data) => {
                                data.resize(self.length, 0);
                                self.output = data;
                            }
                            None => {
                                self.error = 0x84;
                                self.status = STATUS_IDLE;
                            }
                        }
                    } else if !self.keys.get(&self.key_name).is_some_and(|k| k.writable) {
                        self.error = 0x84;
                        self.status = STATUS_IDLE;
                    }
                } else if self.command == CMD_WRITE
                    && self.error == 0
                    && self.position < self.length
                {
                    if let Some(key) = self.keys.get_mut(&self.key_name) {
                        if let Some(slot) = key.data.get_mut(self.position) {
                            *slot = value;
                        }
                    }
                    self.position += 1;
                    if self.position >= self.length {
                        self.status = STATUS_IDLE;
                    }
                }
            }
            CMD_KEY_BY_INDEX if self.received < 4 => {
                // The index, big-endian.
                self.key_name[self.received] = value;
                self.received += 1;
                self.status = STATUS_KEY;
                if self.received == 4 {
                    let index = u32::from_be_bytes(self.key_name) as usize;
                    match self.keys.keys().nth(index) {
                        Some(name) => {
                            self.output = name.to_vec();
                            self.status = STATUS_DATA;
                        }
                        None => {
                            self.error = 0x84;
                            self.status = STATUS_IDLE;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn prepare_type(&mut self) {
        if &self.key_name == b"#KEY" {
            self.output = vec![4];
            self.output.extend_from_slice(b"ui32");
            self.output.push(0);
            self.status = STATUS_DATA;
        } else if let Some(key) = self.keys.get(&self.key_name) {
            self.output = vec![u8::try_from(key.data.len()).unwrap_or(u8::MAX)];
            self.output.extend_from_slice(&key.type_name);
            self.output.push(if key.writable { 0xC0 } else { 0x80 });
            self.status = STATUS_DATA;
        } else {
            self.error = 0x84;
            self.status = STATUS_IDLE;
        }
    }

    fn data_read(&mut self) -> u8 {
        if self.command == CMD_WRITE || self.position >= self.output.len() {
            return 0;
        }
        let byte = self.output[self.position];
        self.position += 1;
        if self.position >= self.output.len() {
            self.status = STATUS_IDLE;
        } else {
            self.status = STATUS_DATA;
        }
        byte
    }
}

/// The SMC register protocol over a table of keys.
#[derive(Debug)]
pub struct AppleSmc {
    inner: std::sync::Mutex<Inner>,
}

impl AppleSmc {
    /// A controller with the keys that are not secret, and no OSK.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(Inner::new()),
        }
    }

    /// The same, with the operator's 64-byte OSK installed as `OSK0` (the
    /// first 32 bytes) and `OSK1` (the rest).
    #[must_use]
    pub fn with_osk(osk: &[u8; OSK_LEN]) -> Self {
        Self {
            inner: std::sync::Mutex::new(Inner::with_osk(osk)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A data-port read, which consumes a byte.
    pub fn read_data(&self) -> u8 {
        self.lock().data_read()
    }
}

impl Default for AppleSmc {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for AppleSmc {
    fn device_type(&self) -> DeviceType {
        DeviceType::Custom
    }

    fn name(&self) -> &str {
        "Apple SMC"
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        let mut inner = self.lock();
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = match offset + i as u64 {
                COMMAND_PORT => inner.status,
                ERROR_PORT => inner.error,
                DATA_PORT => inner.data_read(),
                _ => 0,
            };
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let mut inner = self.lock();
        for (i, byte) in data.iter().enumerate() {
            match offset + i as u64 {
                COMMAND_PORT => match *byte {
                    cmd @ (CMD_READ | CMD_WRITE | CMD_KEY_BY_INDEX | CMD_KEY_TYPE) => {
                        inner.begin(cmd);
                    }
                    _ => {
                        inner.error = 0x81;
                        inner.status = STATUS_IDLE;
                    }
                },
                DATA_PORT => inner.data_write(*byte),
                _ => {}
            }
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut inner = self.lock();
        inner.status = STATUS_IDLE;
        inner.command = 0;
        inner.received = 0;
        inner.position = 0;
        inner.output.clear();
        inner.error = 0;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn command(smc: &mut AppleSmc, cmd: u8) {
        smc.write(COMMAND_PORT, &[cmd]).await.unwrap();
    }

    async fn send(smc: &mut AppleSmc, bytes: &[u8]) {
        for b in bytes {
            smc.write(DATA_PORT, &[*b]).await.unwrap();
        }
    }

    async fn read_key(smc: &mut AppleSmc, key: &[u8; 4], len: u8) -> Vec<u8> {
        command(smc, CMD_READ).await;
        send(smc, key).await;
        send(smc, &[len]).await;
        (0..len).map(|_| smc.read_data()).collect()
    }

    #[tokio::test]
    async fn the_key_count_includes_itself_and_is_big_endian() {
        let mut smc = AppleSmc::new();
        let count = read_key(&mut smc, b"#KEY", 4).await;
        assert_eq!(u32::from_be_bytes(count.try_into().unwrap()), 7);
    }

    #[tokio::test]
    async fn a_command_is_accepted_with_status_0c() {
        let mut smc = AppleSmc::new();
        command(&mut smc, CMD_READ).await;
        let mut status = [0u8];
        smc.read(COMMAND_PORT, &mut status).await.unwrap();
        assert_eq!(status[0], STATUS_ACCEPTED);
    }

    #[tokio::test]
    async fn the_osk_is_absent_unless_the_operator_supplies_it() {
        let mut smc = AppleSmc::new();
        command(&mut smc, CMD_READ).await;
        send(&mut smc, b"OSK0").await;
        send(&mut smc, &[32]).await;
        let mut err = [0u8];
        smc.read(ERROR_PORT, &mut err).await.unwrap();
        assert_eq!(err[0], 0x84, "no such key");
    }

    #[tokio::test]
    async fn an_operator_supplied_osk_reads_back_in_two_halves() {
        let mut osk = [0u8; OSK_LEN];
        for (i, b) in osk.iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut smc = AppleSmc::with_osk(&osk);
        assert_eq!(read_key(&mut smc, b"OSK0", 32).await, osk[..32]);
        assert_eq!(read_key(&mut smc, b"OSK1", 32).await, osk[32..]);
    }

    #[tokio::test]
    async fn a_key_by_index_returns_its_name() {
        let mut smc = AppleSmc::new();
        command(&mut smc, CMD_KEY_BY_INDEX).await;
        send(&mut smc, &[0, 0, 0, 0]).await;
        let name: Vec<u8> = (0..4).map(|_| smc.read_data()).collect();
        assert_eq!(name, b"$Num");
    }

    #[tokio::test]
    async fn a_key_type_reports_length_type_and_flags() {
        let mut smc = AppleSmc::new();
        command(&mut smc, CMD_KEY_TYPE).await;
        send(&mut smc, b"REV ").await;
        let reply: Vec<u8> = (0..6).map(|_| smc.read_data()).collect();
        assert_eq!(reply[0], 6);
        assert_eq!(&reply[1..5], b"{rev");
        assert_eq!(reply[5], 0x80);
    }

    #[tokio::test]
    async fn only_a_writable_key_accepts_a_write() {
        let mut smc = AppleSmc::new();
        command(&mut smc, CMD_WRITE).await;
        send(&mut smc, b"NATJ").await;
        send(&mut smc, &[1, 9]).await;
        assert_eq!(read_key(&mut smc, b"NATJ", 1).await, vec![9]);

        command(&mut smc, CMD_WRITE).await;
        send(&mut smc, b"REV ").await;
        send(&mut smc, &[1, 0xFF]).await;
        assert_eq!(read_key(&mut smc, b"REV ", 1).await, vec![0x01]);
    }
}
