//! Virtual Trusted Platform Module (vTPM)
//!
//! This module provides a software TPM 2.0 implementation for secure
//! key storage, attestation, and cryptographic operations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

/// TPM 2.0 command codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TpmCommandCode {
    /// TPM2_Startup
    Startup = 0x0000_0144,
    /// TPM2_Shutdown
    Shutdown = 0x0000_0145,
    /// TPM2_SelfTest
    SelfTest = 0x0000_0143,
    /// TPM2_GetCapability
    GetCapability = 0x0000_017A,
    /// TPM2_GetRandom
    GetRandom = 0x0000_017B,
    /// TPM2_PCR_Read
    PcrRead = 0x0000_017E,
    /// TPM2_PCR_Extend
    PcrExtend = 0x0000_0182,
    /// TPM2_PCR_Reset
    PcrReset = 0x0000_013D,
    /// TPM2_CreatePrimary
    CreatePrimary = 0x0000_0131,
    /// TPM2_Create
    Create = 0x0000_0153,
    /// TPM2_Load
    Load = 0x0000_0157,
    /// TPM2_Sign
    Sign = 0x0000_015D,
    /// TPM2_VerifySignature
    VerifySignature = 0x0000_0177,
    /// TPM2_Quote
    Quote = 0x0000_0158,
    /// TPM2_Hash
    Hash = 0x0000_017D,
    /// TPM2_NV_Read
    NvRead = 0x0000_014E,
    /// TPM2_NV_Write
    NvWrite = 0x0000_0137,
    /// TPM2_NV_DefineSpace
    NvDefineSpace = 0x0000_012A,
}

impl TpmCommandCode {
    /// Create from u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x0000_0144 => Some(Self::Startup),
            0x0000_0145 => Some(Self::Shutdown),
            0x0000_0143 => Some(Self::SelfTest),
            0x0000_017A => Some(Self::GetCapability),
            0x0000_017B => Some(Self::GetRandom),
            0x0000_017E => Some(Self::PcrRead),
            0x0000_0182 => Some(Self::PcrExtend),
            0x0000_013D => Some(Self::PcrReset),
            0x0000_0131 => Some(Self::CreatePrimary),
            0x0000_0153 => Some(Self::Create),
            0x0000_0157 => Some(Self::Load),
            0x0000_015D => Some(Self::Sign),
            0x0000_0177 => Some(Self::VerifySignature),
            0x0000_0158 => Some(Self::Quote),
            0x0000_017D => Some(Self::Hash),
            0x0000_014E => Some(Self::NvRead),
            0x0000_0137 => Some(Self::NvWrite),
            0x0000_012A => Some(Self::NvDefineSpace),
            _ => None,
        }
    }
}

/// TPM response codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TpmResponseCode {
    /// Success
    Success = 0x0000_0000,
    /// TPM not initialized
    Initialize = 0x0000_0100,
    /// Command not supported
    CommandCode = 0x0000_0143,
    /// Bad parameter
    BadParam = 0x0000_01C4,
    /// Authorization failure
    AuthFail = 0x0000_098E,
    /// PCR index out of range
    BadPcr = 0x0000_01E5,
    /// NV space not found
    NvNotFound = 0x0000_018B,
    /// NV space already defined
    NvDefined = 0x0000_0149,
    /// Internal error
    Failure = 0x0000_0101,
}

/// TPM startup type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupType {
    /// Clear - TPM state is cleared
    Clear,
    /// State - Resume from saved state
    State,
}

/// TPM state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmState {
    /// TPM not initialized
    Uninitialized,
    /// TPM is ready
    Ready,
    /// Self-test in progress
    SelfTesting,
    /// TPM is in failure mode
    Failure,
}

/// Hash algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    /// SHA-1 (160 bits)
    Sha1,
    /// SHA-256 (256 bits)
    Sha256,
    /// SHA-384 (384 bits)
    Sha384,
    /// SHA-512 (512 bits)
    Sha512,
    /// SM3 (256 bits)
    Sm3,
}

impl HashAlgorithm {
    /// Get hash output size in bytes
    pub fn output_size(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 | Self::Sm3 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// Get algorithm ID
    pub fn algorithm_id(&self) -> u16 {
        match self {
            Self::Sha1 => 0x0004,
            Self::Sha256 => 0x000B,
            Self::Sha384 => 0x000C,
            Self::Sha512 => 0x000D,
            Self::Sm3 => 0x0012,
        }
    }
}

/// PCR (Platform Configuration Register) bank
#[derive(Debug)]
pub struct PcrBank {
    /// Hash algorithm
    algorithm: HashAlgorithm,
    /// PCR values (24 PCRs)
    pcrs: Vec<Vec<u8>>,
}

impl PcrBank {
    /// Create a new PCR bank
    pub fn new(algorithm: HashAlgorithm) -> Self {
        let size = algorithm.output_size();
        let pcrs = (0..24).map(|_| vec![0u8; size]).collect();
        Self { algorithm, pcrs }
    }

    /// Get algorithm
    pub fn algorithm(&self) -> HashAlgorithm {
        self.algorithm
    }

    /// Read PCR value
    pub fn read(&self, index: usize) -> Option<&[u8]> {
        self.pcrs.get(index).map(|v| v.as_slice())
    }

    /// Extend PCR with data
    pub fn extend(&mut self, index: usize, data: &[u8]) -> bool {
        if index >= self.pcrs.len() {
            return false;
        }

        // In real TPM: new_value = hash(old_value || data)
        // Simplified: XOR for demonstration
        let pcr = &mut self.pcrs[index];
        for (i, &byte) in data.iter().take(pcr.len()).enumerate() {
            pcr[i] ^= byte;
        }
        true
    }

    /// Reset PCR (only PCRs 16-23 are resettable)
    pub fn reset(&mut self, index: usize) -> bool {
        if index < 16 || index >= self.pcrs.len() {
            return false;
        }
        self.pcrs[index].fill(0);
        true
    }
}

/// NV (Non-Volatile) storage index
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NvIndex(pub u32);

impl NvIndex {
    /// Create new NV index
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    /// Get raw index value
    pub fn raw(&self) -> u32 {
        self.0
    }
}

/// NV storage entry
#[derive(Debug, Clone)]
pub struct NvEntry {
    /// Index
    pub index: NvIndex,
    /// Size
    pub size: u16,
    /// Attributes
    pub attributes: u32,
    /// Data
    pub data: Vec<u8>,
    /// Write count
    pub write_count: u64,
}

impl NvEntry {
    /// Create new NV entry
    pub fn new(index: NvIndex, size: u16, attributes: u32) -> Self {
        Self {
            index,
            size,
            attributes,
            data: vec![0u8; size as usize],
            write_count: 0,
        }
    }
}

/// TPM key handle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyHandle(pub u32);

impl KeyHandle {
    /// Create new handle
    pub fn new(handle: u32) -> Self {
        Self(handle)
    }

    /// Get raw handle value
    pub fn raw(&self) -> u32 {
        self.0
    }

    /// Endorsement key handle
    pub const EK: Self = Self(0x8100_0001);

    /// Storage root key handle
    pub const SRK: Self = Self(0x8100_0002);
}

/// Key type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// RSA key
    Rsa,
    /// ECC key
    Ecc,
    /// Symmetric key
    Symmetric,
}

/// TPM key
#[derive(Debug, Clone)]
pub struct TpmKey {
    /// Handle
    pub handle: KeyHandle,
    /// Key type
    pub key_type: KeyType,
    /// Key bits
    pub bits: u16,
    /// Public key data
    pub public: Vec<u8>,
    /// Private key data (encrypted)
    pub private: Vec<u8>,
    /// Parent handle
    pub parent: Option<KeyHandle>,
}

/// Virtual TPM device
#[derive(Debug)]
pub struct VirtualTpm {
    /// TPM state
    state: RwLock<TpmState>,
    /// PCR banks
    pcr_banks: RwLock<HashMap<HashAlgorithm, PcrBank>>,
    /// NV storage
    nv_storage: RwLock<HashMap<NvIndex, NvEntry>>,
    /// Loaded keys
    keys: RwLock<HashMap<KeyHandle, TpmKey>>,
    /// Next key handle
    next_handle: AtomicU64,
    /// Command count
    command_count: AtomicU64,
    /// Random seed
    random_state: AtomicU64,
    /// Self-test passed
    self_test_passed: AtomicBool,
}

impl VirtualTpm {
    /// Create a new vTPM
    pub fn new() -> Self {
        let mut pcr_banks = HashMap::new();
        pcr_banks.insert(HashAlgorithm::Sha256, PcrBank::new(HashAlgorithm::Sha256));

        Self {
            state: RwLock::new(TpmState::Uninitialized),
            pcr_banks: RwLock::new(pcr_banks),
            nv_storage: RwLock::new(HashMap::new()),
            keys: RwLock::new(HashMap::new()),
            next_handle: AtomicU64::new(0x8000_0100),
            command_count: AtomicU64::new(0),
            random_state: AtomicU64::new(0x1234_5678_9ABC_DEF0),
            self_test_passed: AtomicBool::new(false),
        }
    }

    /// Get TPM state
    pub fn state(&self) -> TpmState {
        *self.state.read().unwrap()
    }

    /// Startup TPM
    pub fn startup(&self, startup_type: StartupType) -> TpmResponseCode {
        let mut state = self.state.write().unwrap();

        match startup_type {
            StartupType::Clear => {
                *state = TpmState::Ready;
                self.self_test_passed.store(false, Ordering::Release);
            }
            StartupType::State => {
                *state = TpmState::Ready;
            }
        }

        TpmResponseCode::Success
    }

    /// Shutdown TPM
    pub fn shutdown(&self) -> TpmResponseCode {
        *self.state.write().unwrap() = TpmState::Uninitialized;
        TpmResponseCode::Success
    }

    /// Run self-test
    pub fn self_test(&self, full_test: bool) -> TpmResponseCode {
        let state = *self.state.read().unwrap();
        if state != TpmState::Ready {
            return TpmResponseCode::Initialize;
        }

        // Simulate self-test
        if full_test {
            *self.state.write().unwrap() = TpmState::SelfTesting;
        }

        self.self_test_passed.store(true, Ordering::Release);
        *self.state.write().unwrap() = TpmState::Ready;

        TpmResponseCode::Success
    }

    /// Check if self-test passed
    pub fn is_self_test_passed(&self) -> bool {
        self.self_test_passed.load(Ordering::Acquire)
    }

    /// Get random bytes
    pub fn get_random(&self, count: usize) -> Result<Vec<u8>, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            // Simple PRNG (not cryptographically secure, just for testing)
            let state = self
                .random_state
                .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
            result.push((state >> 24) as u8);
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }

    /// Read PCR
    pub fn pcr_read(
        &self,
        algorithm: HashAlgorithm,
        index: usize,
    ) -> Result<Vec<u8>, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        let banks = self.pcr_banks.read().unwrap();
        let bank = banks.get(&algorithm).ok_or(TpmResponseCode::BadParam)?;
        let value = bank.read(index).ok_or(TpmResponseCode::BadPcr)?;

        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(value.to_vec())
    }

    /// Extend PCR
    pub fn pcr_extend(
        &self,
        algorithm: HashAlgorithm,
        index: usize,
        data: &[u8],
    ) -> TpmResponseCode {
        if *self.state.read().unwrap() != TpmState::Ready {
            return TpmResponseCode::Initialize;
        }

        let mut banks = self.pcr_banks.write().unwrap();
        let bank = match banks.get_mut(&algorithm) {
            Some(b) => b,
            None => return TpmResponseCode::BadParam,
        };

        if !bank.extend(index, data) {
            return TpmResponseCode::BadPcr;
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        TpmResponseCode::Success
    }

    /// Reset PCR (only PCRs 16-23)
    pub fn pcr_reset(&self, algorithm: HashAlgorithm, index: usize) -> TpmResponseCode {
        if *self.state.read().unwrap() != TpmState::Ready {
            return TpmResponseCode::Initialize;
        }

        let mut banks = self.pcr_banks.write().unwrap();
        let bank = match banks.get_mut(&algorithm) {
            Some(b) => b,
            None => return TpmResponseCode::BadParam,
        };

        if !bank.reset(index) {
            return TpmResponseCode::BadPcr;
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        TpmResponseCode::Success
    }

    /// Define NV space
    pub fn nv_define_space(&self, index: NvIndex, size: u16, attributes: u32) -> TpmResponseCode {
        if *self.state.read().unwrap() != TpmState::Ready {
            return TpmResponseCode::Initialize;
        }

        let mut storage = self.nv_storage.write().unwrap();
        if storage.contains_key(&index) {
            return TpmResponseCode::NvDefined;
        }

        storage.insert(index, NvEntry::new(index, size, attributes));
        self.command_count.fetch_add(1, Ordering::Relaxed);
        TpmResponseCode::Success
    }

    /// Read NV
    pub fn nv_read(
        &self,
        index: NvIndex,
        offset: u16,
        size: u16,
    ) -> Result<Vec<u8>, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        let storage = self.nv_storage.read().unwrap();
        let entry = storage.get(&index).ok_or(TpmResponseCode::NvNotFound)?;

        let start = offset as usize;
        let end = start + size as usize;
        if end > entry.data.len() {
            return Err(TpmResponseCode::BadParam);
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(entry.data[start..end].to_vec())
    }

    /// Write NV
    pub fn nv_write(&self, index: NvIndex, offset: u16, data: &[u8]) -> TpmResponseCode {
        if *self.state.read().unwrap() != TpmState::Ready {
            return TpmResponseCode::Initialize;
        }

        let mut storage = self.nv_storage.write().unwrap();
        let entry = match storage.get_mut(&index) {
            Some(e) => e,
            None => return TpmResponseCode::NvNotFound,
        };

        let start = offset as usize;
        let end = start + data.len();
        if end > entry.data.len() {
            return TpmResponseCode::BadParam;
        }

        entry.data[start..end].copy_from_slice(data);
        entry.write_count += 1;

        self.command_count.fetch_add(1, Ordering::Relaxed);
        TpmResponseCode::Success
    }

    /// Create a key
    pub fn create_key(
        &self,
        key_type: KeyType,
        bits: u16,
        parent: Option<KeyHandle>,
    ) -> Result<KeyHandle, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        let handle = KeyHandle::new(self.next_handle.fetch_add(1, Ordering::Relaxed) as u32);

        let key = TpmKey {
            handle,
            key_type,
            bits,
            public: vec![0u8; (bits / 8) as usize],
            private: vec![0u8; (bits / 8) as usize],
            parent,
        };

        self.keys.write().unwrap().insert(handle, key);
        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(handle)
    }

    /// Load a key
    pub fn load_key(&self, handle: KeyHandle) -> Result<&Self, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        if !self.keys.read().unwrap().contains_key(&handle) {
            return Err(TpmResponseCode::BadParam);
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(self)
    }

    /// Get key info
    pub fn get_key(&self, handle: KeyHandle) -> Option<TpmKey> {
        self.keys.read().unwrap().get(&handle).cloned()
    }

    /// Get command count
    pub fn command_count(&self) -> u64 {
        self.command_count.load(Ordering::Relaxed)
    }

    /// Get NV entry count
    pub fn nv_count(&self) -> usize {
        self.nv_storage.read().unwrap().len()
    }

    /// Get key count
    pub fn key_count(&self) -> usize {
        self.keys.read().unwrap().len()
    }

    /// Hash data
    pub fn hash(&self, algorithm: HashAlgorithm, data: &[u8]) -> Result<Vec<u8>, TpmResponseCode> {
        if *self.state.read().unwrap() != TpmState::Ready {
            return Err(TpmResponseCode::Initialize);
        }

        // Simplified hash (XOR folding for demonstration)
        let size = algorithm.output_size();
        let mut result = vec![0u8; size];

        for (i, &byte) in data.iter().enumerate() {
            result[i % size] ^= byte;
        }

        self.command_count.fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }

    /// Add PCR bank
    pub fn add_pcr_bank(&self, algorithm: HashAlgorithm) {
        self.pcr_banks
            .write()
            .unwrap()
            .insert(algorithm, PcrBank::new(algorithm));
    }

    /// Check if PCR bank exists
    pub fn has_pcr_bank(&self, algorithm: HashAlgorithm) -> bool {
        self.pcr_banks.read().unwrap().contains_key(&algorithm)
    }
}

impl Default for VirtualTpm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_code_from_u32() {
        assert_eq!(
            TpmCommandCode::from_u32(0x0000_0144),
            Some(TpmCommandCode::Startup)
        );
        assert_eq!(
            TpmCommandCode::from_u32(0x0000_017B),
            Some(TpmCommandCode::GetRandom)
        );
        assert_eq!(TpmCommandCode::from_u32(0xFFFF_FFFF), None);
    }

    #[test]
    fn test_hash_algorithm() {
        assert_eq!(HashAlgorithm::Sha1.output_size(), 20);
        assert_eq!(HashAlgorithm::Sha256.output_size(), 32);
        assert_eq!(HashAlgorithm::Sha384.output_size(), 48);
        assert_eq!(HashAlgorithm::Sha512.output_size(), 64);

        assert_eq!(HashAlgorithm::Sha256.algorithm_id(), 0x000B);
    }

    #[test]
    fn test_vtpm_creation() {
        let tpm = VirtualTpm::new();
        assert_eq!(tpm.state(), TpmState::Uninitialized);
        assert!(!tpm.is_self_test_passed());
    }

    #[test]
    fn test_vtpm_startup_shutdown() {
        let tpm = VirtualTpm::new();

        assert_eq!(tpm.startup(StartupType::Clear), TpmResponseCode::Success);
        assert_eq!(tpm.state(), TpmState::Ready);

        assert_eq!(tpm.shutdown(), TpmResponseCode::Success);
        assert_eq!(tpm.state(), TpmState::Uninitialized);
    }

    #[test]
    fn test_vtpm_self_test() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        assert_eq!(tpm.self_test(true), TpmResponseCode::Success);
        assert!(tpm.is_self_test_passed());
    }

    #[test]
    fn test_vtpm_get_random() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        let random = tpm.get_random(32).unwrap();
        assert_eq!(random.len(), 32);

        // Should return different values
        let random2 = tpm.get_random(32).unwrap();
        assert_ne!(random, random2);
    }

    #[test]
    fn test_vtpm_get_random_not_initialized() {
        let tpm = VirtualTpm::new();
        assert!(matches!(
            tpm.get_random(32),
            Err(TpmResponseCode::Initialize)
        ));
    }

    #[test]
    fn test_pcr_read_extend() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        // Read initial PCR (all zeros)
        let pcr0 = tpm.pcr_read(HashAlgorithm::Sha256, 0).unwrap();
        assert_eq!(pcr0.len(), 32);
        assert!(pcr0.iter().all(|&b| b == 0));

        // Extend PCR
        assert_eq!(
            tpm.pcr_extend(HashAlgorithm::Sha256, 0, b"test data"),
            TpmResponseCode::Success
        );

        // Read again - should be different
        let pcr0_after = tpm.pcr_read(HashAlgorithm::Sha256, 0).unwrap();
        assert_ne!(pcr0, pcr0_after);
    }

    #[test]
    fn test_pcr_reset() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        // Can't reset PCR 0-15
        assert_eq!(
            tpm.pcr_reset(HashAlgorithm::Sha256, 0),
            TpmResponseCode::BadPcr
        );

        // Can reset PCR 16-23
        tpm.pcr_extend(HashAlgorithm::Sha256, 16, b"data");
        assert_eq!(
            tpm.pcr_reset(HashAlgorithm::Sha256, 16),
            TpmResponseCode::Success
        );

        let pcr16 = tpm.pcr_read(HashAlgorithm::Sha256, 16).unwrap();
        assert!(pcr16.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_pcr_bad_index() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        assert!(matches!(
            tpm.pcr_read(HashAlgorithm::Sha256, 100),
            Err(TpmResponseCode::BadPcr)
        ));
    }

    #[test]
    fn test_nv_define_read_write() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        let index = NvIndex::new(0x0140_0001);

        // Define space
        assert_eq!(tpm.nv_define_space(index, 64, 0), TpmResponseCode::Success);

        // Can't define same index twice
        assert_eq!(
            tpm.nv_define_space(index, 64, 0),
            TpmResponseCode::NvDefined
        );

        // Write data
        assert_eq!(
            tpm.nv_write(index, 0, b"Hello, TPM!"),
            TpmResponseCode::Success
        );

        // Read data
        let data = tpm.nv_read(index, 0, 11).unwrap();
        assert_eq!(&data, b"Hello, TPM!");
    }

    #[test]
    fn test_nv_not_found() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        let index = NvIndex::new(0x0140_0001);

        assert!(matches!(
            tpm.nv_read(index, 0, 10),
            Err(TpmResponseCode::NvNotFound)
        ));
    }

    #[test]
    fn test_create_key() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        let handle = tpm.create_key(KeyType::Rsa, 2048, None).unwrap();

        let key = tpm.get_key(handle).unwrap();
        assert_eq!(key.key_type, KeyType::Rsa);
        assert_eq!(key.bits, 2048);
    }

    #[test]
    fn test_key_handle() {
        assert_eq!(KeyHandle::EK.raw(), 0x8100_0001);
        assert_eq!(KeyHandle::SRK.raw(), 0x8100_0002);
    }

    #[test]
    fn test_hash() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        let hash = tpm.hash(HashAlgorithm::Sha256, b"test data").unwrap();
        assert_eq!(hash.len(), 32);

        // Same input should give same output
        let hash2 = tpm.hash(HashAlgorithm::Sha256, b"test data").unwrap();
        assert_eq!(hash, hash2);

        // Different input should give different output
        let hash3 = tpm.hash(HashAlgorithm::Sha256, b"different").unwrap();
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_command_count() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        assert_eq!(tpm.command_count(), 0);

        tpm.get_random(16).unwrap();
        assert_eq!(tpm.command_count(), 1);

        tpm.get_random(16).unwrap();
        assert_eq!(tpm.command_count(), 2);
    }

    #[test]
    fn test_add_pcr_bank() {
        let tpm = VirtualTpm::new();

        assert!(tpm.has_pcr_bank(HashAlgorithm::Sha256));
        assert!(!tpm.has_pcr_bank(HashAlgorithm::Sha384));

        tpm.add_pcr_bank(HashAlgorithm::Sha384);
        assert!(tpm.has_pcr_bank(HashAlgorithm::Sha384));
    }

    #[test]
    fn test_nv_entry() {
        let entry = NvEntry::new(NvIndex::new(1), 64, 0x1234);
        assert_eq!(entry.size, 64);
        assert_eq!(entry.attributes, 0x1234);
        assert_eq!(entry.data.len(), 64);
        assert_eq!(entry.write_count, 0);
    }

    #[test]
    fn test_startup_state() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::State);
        assert_eq!(tpm.state(), TpmState::Ready);
    }

    #[test]
    fn test_nv_count() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        assert_eq!(tpm.nv_count(), 0);

        tpm.nv_define_space(NvIndex::new(1), 32, 0);
        assert_eq!(tpm.nv_count(), 1);

        tpm.nv_define_space(NvIndex::new(2), 32, 0);
        assert_eq!(tpm.nv_count(), 2);
    }

    #[test]
    fn test_key_count() {
        let tpm = VirtualTpm::new();
        tpm.startup(StartupType::Clear);

        assert_eq!(tpm.key_count(), 0);

        tpm.create_key(KeyType::Rsa, 2048, None).unwrap();
        assert_eq!(tpm.key_count(), 1);
    }
}
