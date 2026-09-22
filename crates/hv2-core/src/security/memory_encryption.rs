//! Memory encryption for confidential computing
//!
//! # This module encrypts nothing
//!
//! Stated first because the names below promise otherwise. There is no
//! `ioctl`, no `libc` call and no `unsafe` block in this file, and no
//! `KVM_MEMORY_ENCRYPT_OP` anywhere in this workspace. It is a *model* of the
//! state AMD SEV and Intel TDX keep -- which key is live, which guest page is
//! marked encrypted -- and not a path to either.
//!
//! The distinction is the whole point. A VM whose memory is genuinely
//! encrypted is protected from the host it runs on; that is the property
//! confidential computing sells and the reason it appears in requirements for
//! handling controlled data. A VM that merely *reports* encryption is running
//! in plaintext host memory while its operator believes otherwise, which is
//! worse than one that reports nothing, because the belief is what the data
//! was entrusted to.
//!
//! So [`EncryptionManager::enable`] refuses. It does not refuse because the
//! hardware is missing -- it would refuse on an EPYC with SEV-SNP too --
//! because what is missing is the code that would drive it. When that code
//! exists it will arrive as a [`MemoryEncryptionBackend`], and `enable` will
//! succeed exactly when one is attached and not before.
//!
//! [`EncryptionTechnology::available_on_this_host`] is separate and does tell
//! the truth: it asks the running kernel what this machine supports. Useful
//! for deciding where a workload can go, and it is the first thing a real
//! implementation has to get right.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::RwLock;

/// Does this host advertise `flag` in `/proc/cpuinfo` *and* expose `/dev/sev`?
///
/// Both, for the reason in [`EncryptionTechnology::available_on_this_host`].
/// Non-Linux answers `false`: there is no `/proc` to consult, and guessing in
/// the permissive direction is how a control ends up reporting a protection
/// nobody has.
#[cfg(target_os = "linux")]
fn host_has_amd_sev(flag: &str) -> bool {
    let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") else {
        return false;
    };
    let advertised = info
        .lines()
        .filter(|l| l.starts_with("flags"))
        .any(|l| l.split_whitespace().any(|f| f == flag));
    advertised && std::path::Path::new("/dev/sev").exists()
}

#[cfg(not(target_os = "linux"))]
fn host_has_amd_sev(_flag: &str) -> bool {
    false
}

/// Does this host's `kvm_intel` report TDX enabled?
#[cfg(target_os = "linux")]
fn host_has_intel_tdx() -> bool {
    std::fs::read_to_string("/sys/module/kvm_intel/parameters/tdx")
        .map(|v| v.trim().eq_ignore_ascii_case("y"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn host_has_intel_tdx() -> bool {
    false
}

/// Page size for encryption (4KB)
pub const PAGE_SIZE: u64 = 4096;

/// Encryption technology type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionTechnology {
    /// No encryption
    None,
    /// AMD SEV (Secure Encrypted Virtualization)
    AmdSev,
    /// AMD SEV-ES (Encrypted State)
    AmdSevEs,
    /// AMD SEV-SNP (Secure Nested Paging)
    AmdSevSnp,
    /// Intel TDX (Trust Domain Extensions)
    IntelTdx,
    /// Intel MKTME (Multi-Key Total Memory Encryption)
    IntelMktme,
}

impl EncryptionTechnology {
    /// Check if this technology encrypts memory
    ///
    /// A property of the technology, not of this build or this machine: it
    /// says SEV-SNP encrypts memory and `None` does not. Whether *anything
    /// here* will encrypt memory is [`EncryptionManager::enable`], which
    /// refuses, and whether this host could is
    /// [`Self::available_on_this_host`].
    pub fn encrypts_memory(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether the running kernel says this machine can provide it.
    ///
    /// Asks the host rather than assuming, and answers `false` when it cannot
    /// tell -- an unreadable `/proc` or a platform without one is not evidence
    /// of support. The checks are the ones an operator would make by hand:
    ///
    /// * AMD: the CPU flag (`sev`, `sev_es`, `sev_snp`) *and* `/dev/sev`,
    ///   because the flag says the silicon can and the device node says the
    ///   kernel driver bound to it. Either alone is half an answer.
    /// * Intel TDX: `/sys/module/kvm_intel/parameters/tdx` reading `Y`.
    /// * MKTME: reported unsupported. It is total-memory encryption against
    ///   physical attack, not per-guest isolation from the host, so answering
    ///   `true` would invite it to be used for something it does not do.
    ///
    /// This is capability reporting, not a licence: a `true` here still does
    /// not make [`EncryptionManager::enable`] succeed, because the driver is
    /// what is missing.
    #[must_use]
    pub fn available_on_this_host(self) -> bool {
        match self {
            Self::None => true,
            Self::AmdSev => host_has_amd_sev("sev"),
            Self::AmdSevEs => host_has_amd_sev("sev_es"),
            Self::AmdSevSnp => host_has_amd_sev("sev_snp"),
            Self::IntelTdx => host_has_intel_tdx(),
            // See the doc above: deliberately not claimed.
            Self::IntelMktme => false,
        }
    }

    /// Check if this technology encrypts CPU state
    pub fn encrypts_cpu_state(&self) -> bool {
        matches!(self, Self::AmdSevEs | Self::AmdSevSnp | Self::IntelTdx)
    }

    /// Check if this technology provides attestation
    pub fn provides_attestation(&self) -> bool {
        matches!(self, Self::AmdSevSnp | Self::IntelTdx)
    }

    /// Get key size in bits
    pub fn key_size_bits(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::AmdSev | Self::AmdSevEs | Self::AmdSevSnp => 128,
            Self::IntelTdx | Self::IntelMktme => 128,
        }
    }
}

/// Memory encryption key identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyId(pub u16);

impl KeyId {
    /// Host/hypervisor key ID (shared memory)
    pub const HOST: Self = Self(0);

    /// Guest default key ID
    pub const GUEST_DEFAULT: Self = Self(1);

    /// Create a new key ID
    pub fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get raw key ID value
    pub fn raw(&self) -> u16 {
        self.0
    }
}

/// Encryption key state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// Key is not initialized
    Uninitialized,
    /// Key is being generated
    Generating,
    /// Key is active and can be used
    Active,
    /// Key is being rotated
    Rotating,
    /// Key has been revoked
    Revoked,
}

/// Encryption key metadata
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    /// Key identifier
    pub key_id: KeyId,
    /// Key state
    pub state: KeyState,
    /// Whether key is for guest or host
    pub is_guest_key: bool,
    /// Creation timestamp (nanoseconds)
    pub created_at: u64,
    /// Last used timestamp (nanoseconds)
    pub last_used_at: u64,
    /// Pages encrypted with this key
    pub page_count: u64,
}

impl KeyMetadata {
    /// Create new key metadata
    pub fn new(key_id: KeyId, is_guest_key: bool) -> Self {
        Self {
            key_id,
            state: KeyState::Uninitialized,
            is_guest_key,
            created_at: 0,
            last_used_at: 0,
            page_count: 0,
        }
    }
}

/// Page encryption state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEncryptionState {
    /// Page is not encrypted (shared)
    Shared,
    /// Page is encrypted with specified key
    Encrypted(KeyId),
    /// Page is in transition between states
    Transitioning,
}

/// C-bit (encryption bit) position in page table entries
#[derive(Debug, Clone, Copy)]
pub struct CbitPosition {
    /// Bit position in physical address
    pub bit_position: u8,
    /// Mask for the C-bit
    pub mask: u64,
}

impl CbitPosition {
    /// Create a new C-bit position
    pub fn new(bit_position: u8) -> Self {
        Self {
            bit_position,
            mask: 1u64 << bit_position,
        }
    }

    /// Check if address has C-bit set
    pub fn is_encrypted(&self, address: u64) -> bool {
        (address & self.mask) != 0
    }

    /// Set C-bit in address
    pub fn set_encrypted(&self, address: u64) -> u64 {
        address | self.mask
    }

    /// Clear C-bit in address
    pub fn clear_encrypted(&self, address: u64) -> u64 {
        address & !self.mask
    }

    /// Get physical address without C-bit
    pub fn physical_address(&self, address: u64) -> u64 {
        address & !self.mask
    }
}

impl Default for CbitPosition {
    fn default() -> Self {
        // AMD SEV typically uses bit 47
        Self::new(47)
    }
}

/// Memory encryption configuration
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// Encryption technology
    pub technology: EncryptionTechnology,
    /// C-bit position
    pub cbit: CbitPosition,
    /// Maximum number of key IDs
    pub max_key_ids: u16,
    /// Guest policy (SEV-SNP)
    pub guest_policy: u64,
    /// Enable debug mode
    pub debug_mode: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            technology: EncryptionTechnology::None,
            cbit: CbitPosition::default(),
            max_key_ids: 16,
            guest_policy: 0,
            debug_mode: false,
        }
    }
}

/// Encryption error types
///
/// `#[non_exhaustive]` for the same reason as
/// [`super::secure_boot::VerificationResult`]: `NoBackend` was added when
/// `enable` stopped succeeding, and attaching a real backend will bring the
/// failures a driver can actually have.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EncryptionError {
    /// No backend is attached, so nothing can encrypt.
    #[error(
        "no memory-encryption backend is attached: this build models {0:?} state          but cannot drive it, so guest memory would not be encrypted"
    )]
    NoBackend(EncryptionTechnology),
    /// Technology not supported
    #[error("Encryption technology not supported: {0:?}")]
    NotSupported(EncryptionTechnology),
    /// Key ID not found
    #[error("Key not found: {0:?}")]
    KeyNotFound(KeyId),
    /// Key in invalid state
    #[error("Invalid key state: {0:?}")]
    InvalidKeyState(KeyState),
    /// Page already encrypted
    #[error("Page already encrypted: 0x{0:x}")]
    AlreadyEncrypted(u64),
    /// Page not encrypted
    #[error("Page not encrypted: 0x{0:x}")]
    NotEncrypted(u64),
    /// Maximum keys exceeded
    #[error("Maximum number of keys exceeded")]
    MaxKeysExceeded,
    /// Invalid C-bit position
    #[error("Invalid C-bit position")]
    InvalidCbitPosition,
    /// Attestation failed
    #[error("Attestation failed: {0}")]
    AttestationFailed(String),
    /// Hardware error
    #[error("Hardware error: {0}")]
    HardwareError(String),
}

/// Result type for encryption operations
pub type EncryptionResult<T> = Result<T, EncryptionError>;

/// Memory encryption manager
#[derive(Debug)]
pub struct EncryptionManager {
    /// Configuration
    config: EncryptionConfig,
    /// Key metadata
    keys: RwLock<HashMap<KeyId, KeyMetadata>>,
    /// Page encryption states (GPA -> state)
    page_states: RwLock<HashMap<u64, PageEncryptionState>>,
    /// Whether encryption is enabled
    enabled: AtomicBool,
    /// Total encrypted pages
    encrypted_pages: AtomicU64,
    /// Total shared pages
    shared_pages: AtomicU64,
    /// Encryption operations count
    operations: AtomicU64,
}

impl EncryptionManager {
    /// Create a new encryption manager
    pub fn new(config: EncryptionConfig) -> Self {
        Self {
            config,
            keys: RwLock::new(HashMap::new()),
            page_states: RwLock::new(HashMap::new()),
            enabled: AtomicBool::new(false),
            encrypted_pages: AtomicU64::new(0),
            shared_pages: AtomicU64::new(0),
            operations: AtomicU64::new(0),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &EncryptionConfig {
        &self.config
    }

    /// Check if encryption is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Refuse to enable encryption, because this build cannot perform it.
    ///
    /// See the module header. This used to set a flag and return `Ok(())` on
    /// any machine, which meant a caller could switch on "memory encryption"
    /// and be told it worked while the guest ran in plaintext host memory.
    /// Nothing called it, so nothing was misled -- but it was one caller away
    /// from being the most dangerous kind of wrong, and a security control
    /// that cannot do its job should say so rather than succeed.
    ///
    /// # Errors
    ///
    /// [`EncryptionError::NotSupported`] when the configured technology is
    /// [`EncryptionTechnology::None`], and [`EncryptionError::NoBackend`]
    /// otherwise -- including on hardware that supports the technology, since
    /// the missing piece is the driver and not the silicon.
    pub fn enable(&self) -> EncryptionResult<()> {
        if self.config.technology == EncryptionTechnology::None {
            return Err(EncryptionError::NotSupported(EncryptionTechnology::None));
        }
        Err(EncryptionError::NoBackend(self.config.technology))
    }

    /// Switch the *model* on, for tests of the bookkeeping in this file.
    ///
    /// Deliberately not public: the bookkeeping is worth testing, and the way
    /// to test it is not to make production code able to claim an encryption
    /// it does not have.
    #[cfg(test)]
    fn enable_model_for_tests(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    /// Disable encryption
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }

    /// Create a new encryption key
    pub fn create_key(&self, key_id: KeyId, is_guest_key: bool) -> EncryptionResult<()> {
        let mut keys = self.keys.write();

        if keys.len() >= self.config.max_key_ids as usize {
            return Err(EncryptionError::MaxKeysExceeded);
        }

        let mut metadata = KeyMetadata::new(key_id, is_guest_key);
        metadata.state = KeyState::Active;

        keys.insert(key_id, metadata);
        Ok(())
    }

    /// Get key metadata
    pub fn get_key(&self, key_id: KeyId) -> Option<KeyMetadata> {
        self.keys.read().get(&key_id).cloned()
    }

    /// Revoke a key
    pub fn revoke_key(&self, key_id: KeyId) -> EncryptionResult<()> {
        let mut keys = self.keys.write();
        let key = keys
            .get_mut(&key_id)
            .ok_or(EncryptionError::KeyNotFound(key_id))?;

        if key.state != KeyState::Active {
            return Err(EncryptionError::InvalidKeyState(key.state));
        }

        key.state = KeyState::Revoked;
        Ok(())
    }

    /// Mark page as encrypted
    pub fn encrypt_page(&self, gpa: u64, key_id: KeyId) -> EncryptionResult<()> {
        if !self.is_enabled() {
            return Err(EncryptionError::NotSupported(EncryptionTechnology::None));
        }

        // Verify key exists and is active
        {
            let keys = self.keys.read();
            let key = keys
                .get(&key_id)
                .ok_or(EncryptionError::KeyNotFound(key_id))?;
            if key.state != KeyState::Active {
                return Err(EncryptionError::InvalidKeyState(key.state));
            }
        }

        let page_gpa = gpa & !(PAGE_SIZE - 1);

        let mut pages = self.page_states.write();
        if let Some(PageEncryptionState::Encrypted(_)) = pages.get(&page_gpa) {
            return Err(EncryptionError::AlreadyEncrypted(page_gpa));
        }

        pages.insert(page_gpa, PageEncryptionState::Encrypted(key_id));
        self.encrypted_pages.fetch_add(1, Ordering::Relaxed);
        self.operations.fetch_add(1, Ordering::Relaxed);

        // Update key page count
        if let Some(key) = self.keys.write().get_mut(&key_id) {
            key.page_count += 1;
        }

        Ok(())
    }

    /// Mark page as shared (unencrypted)
    pub fn share_page(&self, gpa: u64) -> EncryptionResult<()> {
        let page_gpa = gpa & !(PAGE_SIZE - 1);

        let mut pages = self.page_states.write();
        let old_state = pages.insert(page_gpa, PageEncryptionState::Shared);

        if let Some(PageEncryptionState::Encrypted(key_id)) = old_state {
            self.encrypted_pages.fetch_sub(1, Ordering::Relaxed);
            if let Some(key) = self.keys.write().get_mut(&key_id) {
                key.page_count = key.page_count.saturating_sub(1);
            }
        }

        self.shared_pages.fetch_add(1, Ordering::Relaxed);
        self.operations.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Get page encryption state
    pub fn get_page_state(&self, gpa: u64) -> PageEncryptionState {
        let page_gpa = gpa & !(PAGE_SIZE - 1);
        self.page_states
            .read()
            .get(&page_gpa)
            .copied()
            .unwrap_or(PageEncryptionState::Shared)
    }

    /// Check if page is encrypted
    pub fn is_page_encrypted(&self, gpa: u64) -> bool {
        matches!(self.get_page_state(gpa), PageEncryptionState::Encrypted(_))
    }

    /// Translate address with C-bit handling
    pub fn translate_address(&self, gpa: u64) -> u64 {
        if !self.is_enabled() {
            return gpa;
        }

        let page_gpa = gpa & !(PAGE_SIZE - 1);
        let offset = gpa & (PAGE_SIZE - 1);

        let encrypted = self.is_page_encrypted(page_gpa);
        let translated = if encrypted {
            self.config.cbit.set_encrypted(page_gpa)
        } else {
            self.config.cbit.clear_encrypted(page_gpa)
        };

        translated | offset
    }

    /// Get encryption statistics
    pub fn stats(&self) -> EncryptionStats {
        EncryptionStats {
            technology: self.config.technology,
            enabled: self.is_enabled(),
            encrypted_pages: self.encrypted_pages.load(Ordering::Relaxed),
            shared_pages: self.shared_pages.load(Ordering::Relaxed),
            key_count: self.keys.read().len() as u32,
            operations: self.operations.load(Ordering::Relaxed),
        }
    }

    /// Get number of keys
    pub fn key_count(&self) -> usize {
        self.keys.read().len()
    }

    /// List all key IDs
    pub fn list_keys(&self) -> Vec<KeyId> {
        self.keys.read().keys().copied().collect()
    }
}

/// Encryption statistics
#[derive(Debug, Clone)]
pub struct EncryptionStats {
    /// Encryption technology
    pub technology: EncryptionTechnology,
    /// Whether encryption is enabled
    pub enabled: bool,
    /// Number of encrypted pages
    pub encrypted_pages: u64,
    /// Number of shared pages
    pub shared_pages: u64,
    /// Number of active keys
    pub key_count: u32,
    /// Total operations performed
    pub operations: u64,
}

/// SEV launch state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SevLaunchState {
    /// Not started
    Idle,
    /// Launch started
    Started,
    /// Measuring guest
    Measuring,
    /// Launch finished
    Finished,
    /// Running
    Running,
}

/// SEV guest context
#[derive(Debug)]
pub struct SevContext {
    /// Launch state
    pub state: SevLaunchState,
    /// Guest handle (assigned by PSP)
    pub handle: u32,
    /// Policy
    pub policy: u32,
    /// Launch measurement
    pub measurement: [u8; 48],
    /// API major version
    pub api_major: u8,
    /// API minor version
    pub api_minor: u8,
    /// Build ID
    pub build_id: u8,
}

impl SevContext {
    /// Create new SEV context
    pub fn new() -> Self {
        Self {
            state: SevLaunchState::Idle,
            handle: 0,
            policy: 0,
            measurement: [0u8; 48],
            api_major: 0,
            api_minor: 0,
            build_id: 0,
        }
    }

    /// Start launch
    pub fn start_launch(&mut self, policy: u32) {
        self.state = SevLaunchState::Started;
        self.policy = policy;
    }

    /// Update measurement
    pub fn update_measurement(&mut self, data: &[u8]) {
        self.state = SevLaunchState::Measuring;
        // In real implementation, this would use SHA-256/384
        let len = data.len().min(48);
        self.measurement[..len].copy_from_slice(&data[..len]);
    }

    /// Finish launch
    pub fn finish_launch(&mut self) {
        self.state = SevLaunchState::Finished;
    }

    /// Start running
    pub fn run(&mut self) {
        self.state = SevLaunchState::Running;
    }

    /// Get measurement
    pub fn get_measurement(&self) -> &[u8; 48] {
        &self.measurement
    }
}

impl Default for SevContext {
    fn default() -> Self {
        Self::new()
    }
}

/// TDX TD (Trust Domain) context
#[derive(Debug)]
pub struct TdxContext {
    /// TD attributes
    pub attributes: u64,
    /// XFAM (Extended Feature Attribute Mask)
    pub xfam: u64,
    /// Max VCPUs
    pub max_vcpus: u32,
    /// TDCS pages
    pub tdcs_pages: u32,
    /// Measurement registers (MRs)
    pub mr_td: [u8; 48],
    pub mr_config_id: [u8; 48],
    pub mr_owner: [u8; 48],
    pub mr_owner_config: [u8; 48],
}

impl TdxContext {
    /// Create new TDX context
    pub fn new() -> Self {
        Self {
            attributes: 0,
            xfam: 0,
            max_vcpus: 1,
            tdcs_pages: 0,
            mr_td: [0u8; 48],
            mr_config_id: [0u8; 48],
            mr_owner: [0u8; 48],
            mr_owner_config: [0u8; 48],
        }
    }

    /// Set attributes
    pub fn set_attributes(&mut self, attributes: u64) {
        self.attributes = attributes;
    }

    /// Update MR_TD
    pub fn update_mr_td(&mut self, data: &[u8]) {
        let len = data.len().min(48);
        self.mr_td[..len].copy_from_slice(&data[..len]);
    }
}

impl Default for TdxContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_technology_properties() {
        assert!(!EncryptionTechnology::None.encrypts_memory());
        assert!(EncryptionTechnology::AmdSev.encrypts_memory());
        assert!(EncryptionTechnology::IntelTdx.encrypts_memory());

        assert!(!EncryptionTechnology::AmdSev.encrypts_cpu_state());
        assert!(EncryptionTechnology::AmdSevEs.encrypts_cpu_state());
        assert!(EncryptionTechnology::IntelTdx.encrypts_cpu_state());

        assert!(!EncryptionTechnology::AmdSev.provides_attestation());
        assert!(EncryptionTechnology::AmdSevSnp.provides_attestation());
        assert!(EncryptionTechnology::IntelTdx.provides_attestation());
    }

    #[test]
    fn test_key_id() {
        assert_eq!(KeyId::HOST.raw(), 0);
        assert_eq!(KeyId::GUEST_DEFAULT.raw(), 1);
        assert_eq!(KeyId::new(42).raw(), 42);
    }

    #[test]
    fn test_cbit_position() {
        let cbit = CbitPosition::new(47);

        let addr = 0x1000u64;
        assert!(!cbit.is_encrypted(addr));

        let encrypted = cbit.set_encrypted(addr);
        assert!(cbit.is_encrypted(encrypted));
        assert_eq!(encrypted, addr | (1u64 << 47));

        let cleared = cbit.clear_encrypted(encrypted);
        assert!(!cbit.is_encrypted(cleared));
        assert_eq!(cleared, addr);

        assert_eq!(cbit.physical_address(encrypted), addr);
    }

    #[test]
    fn test_encryption_config_default() {
        let config = EncryptionConfig::default();
        assert_eq!(config.technology, EncryptionTechnology::None);
        assert_eq!(config.cbit.bit_position, 47);
        assert_eq!(config.max_key_ids, 16);
    }

    #[test]
    fn test_encryption_manager_creation() {
        let manager = EncryptionManager::new(EncryptionConfig::default());
        assert!(!manager.is_enabled());
        assert_eq!(manager.key_count(), 0);
    }

    #[test]
    fn test_encryption_manager_enable_none() {
        let manager = EncryptionManager::new(EncryptionConfig::default());
        let result = manager.enable();
        assert!(matches!(result, Err(EncryptionError::NotSupported(_))));
    }

    #[test]
    fn test_enable_refuses_because_nothing_can_encrypt() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSevSnp,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);

        // The point of the whole module. This used to assert `is_ok()`, which
        // was true and meant nothing: it set a flag. A caller reading that as
        // "guest memory is encrypted" would have been wrong on every machine.
        let err = manager.enable().expect_err("enable must refuse");
        assert!(
            matches!(
                err,
                EncryptionError::NoBackend(EncryptionTechnology::AmdSevSnp)
            ),
            "refusal should name the missing backend, got: {err}"
        );
        assert!(
            !manager.is_enabled(),
            "a refused enable must leave encryption off"
        );
    }

    /// Refusal does not depend on the hardware being absent.
    ///
    /// Worth its own test because the tempting fix is to gate `enable` on
    /// [`EncryptionTechnology::available_on_this_host`], which would make it
    /// succeed on an EPYC and encrypt exactly as much as it does here.
    #[test]
    fn enable_refuses_even_where_the_silicon_would_support_it() {
        for tech in [
            EncryptionTechnology::AmdSev,
            EncryptionTechnology::AmdSevEs,
            EncryptionTechnology::AmdSevSnp,
            EncryptionTechnology::IntelTdx,
        ] {
            let manager = EncryptionManager::new(EncryptionConfig {
                technology: tech,
                ..Default::default()
            });
            assert!(
                manager.enable().is_err(),
                "{tech:?}: enable must refuse whatever the host is"
            );
        }
    }

    /// Capability reporting is allowed to say yes, and must not say yes here.
    ///
    /// This machine is a desktop Ryzen: `svm` but no `sev`, and no `/dev/sev`.
    /// The assertion is written against what the host actually reports rather
    /// than against a hardcoded expectation, so it stays honest if this ever
    /// runs on an EPYC -- what it pins is that detection agrees with the
    /// kernel, not that the answer is `false`.
    #[test]
    fn availability_matches_what_the_kernel_reports() {
        let claimed = EncryptionTechnology::AmdSevSnp.available_on_this_host();
        let truth = cfg!(target_os = "linux")
            && std::path::Path::new("/dev/sev").exists()
            && std::fs::read_to_string("/proc/cpuinfo")
                .map(|i| {
                    i.lines()
                        .filter(|l| l.starts_with("flags"))
                        .any(|l| l.split_whitespace().any(|f| f == "sev_snp"))
                })
                .unwrap_or(false);
        assert_eq!(
            claimed, truth,
            "detection disagrees with the running kernel"
        );

        // `None` is the one technology every host can provide, because it is
        // the absence of one.
        assert!(EncryptionTechnology::None.available_on_this_host());
        // Not claimed even where the CPU has it; see the doc comment.
        assert!(!EncryptionTechnology::IntelMktme.available_on_this_host());
    }

    #[test]
    fn test_key_management() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);

        // Create key
        assert!(manager.create_key(KeyId::GUEST_DEFAULT, true).is_ok());
        assert_eq!(manager.key_count(), 1);

        // Get key
        let key = manager.get_key(KeyId::GUEST_DEFAULT).unwrap();
        assert!(key.is_guest_key);
        assert_eq!(key.state, KeyState::Active);

        // Revoke key
        assert!(manager.revoke_key(KeyId::GUEST_DEFAULT).is_ok());
        let key = manager.get_key(KeyId::GUEST_DEFAULT).unwrap();
        assert_eq!(key.state, KeyState::Revoked);

        // Can't revoke again
        assert!(matches!(
            manager.revoke_key(KeyId::GUEST_DEFAULT),
            Err(EncryptionError::InvalidKeyState(_))
        ));
    }

    #[test]
    fn test_max_keys() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            max_key_ids: 3,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);

        assert!(manager.create_key(KeyId::new(1), true).is_ok());
        assert!(manager.create_key(KeyId::new(2), true).is_ok());
        assert!(manager.create_key(KeyId::new(3), true).is_ok());

        assert!(matches!(
            manager.create_key(KeyId::new(4), true),
            Err(EncryptionError::MaxKeysExceeded)
        ));
    }

    #[test]
    fn test_page_encryption() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);
        manager.enable_model_for_tests();
        manager.create_key(KeyId::GUEST_DEFAULT, true).unwrap();

        // Encrypt page
        assert!(manager.encrypt_page(0x1000, KeyId::GUEST_DEFAULT).is_ok());
        assert!(manager.is_page_encrypted(0x1000));

        // Can't encrypt again
        assert!(matches!(
            manager.encrypt_page(0x1000, KeyId::GUEST_DEFAULT),
            Err(EncryptionError::AlreadyEncrypted(_))
        ));

        // Share page
        assert!(manager.share_page(0x1000).is_ok());
        assert!(!manager.is_page_encrypted(0x1000));
    }

    #[test]
    fn test_address_translation() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            cbit: CbitPosition::new(47),
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);
        manager.enable_model_for_tests();
        manager.create_key(KeyId::GUEST_DEFAULT, true).unwrap();

        // Shared page - no C-bit
        let addr = manager.translate_address(0x1000);
        assert_eq!(addr, 0x1000);

        // Encrypted page - C-bit set
        manager.encrypt_page(0x1000, KeyId::GUEST_DEFAULT).unwrap();
        let addr = manager.translate_address(0x1234);
        assert_eq!(addr & (1u64 << 47), 1u64 << 47);
    }

    #[test]
    fn test_encryption_stats() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);
        manager.enable_model_for_tests();
        manager.create_key(KeyId::GUEST_DEFAULT, true).unwrap();

        manager.encrypt_page(0x1000, KeyId::GUEST_DEFAULT).unwrap();
        manager.encrypt_page(0x2000, KeyId::GUEST_DEFAULT).unwrap();
        manager.share_page(0x3000).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.encrypted_pages, 2);
        assert_eq!(stats.shared_pages, 1);
        assert_eq!(stats.key_count, 1);
        assert_eq!(stats.operations, 3);
    }

    #[test]
    fn test_key_page_count() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);
        manager.enable_model_for_tests();
        manager.create_key(KeyId::GUEST_DEFAULT, true).unwrap();

        manager.encrypt_page(0x1000, KeyId::GUEST_DEFAULT).unwrap();
        manager.encrypt_page(0x2000, KeyId::GUEST_DEFAULT).unwrap();

        let key = manager.get_key(KeyId::GUEST_DEFAULT).unwrap();
        assert_eq!(key.page_count, 2);

        // Share one page
        manager.share_page(0x1000).unwrap();
        let key = manager.get_key(KeyId::GUEST_DEFAULT).unwrap();
        assert_eq!(key.page_count, 1);
    }

    #[test]
    fn test_list_keys() {
        let config = EncryptionConfig {
            technology: EncryptionTechnology::AmdSev,
            ..Default::default()
        };
        let manager = EncryptionManager::new(config);

        manager.create_key(KeyId::new(1), true).unwrap();
        manager.create_key(KeyId::new(2), false).unwrap();

        let keys = manager.list_keys();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&KeyId::new(1)));
        assert!(keys.contains(&KeyId::new(2)));
    }

    #[test]
    fn test_sev_context() {
        let mut ctx = SevContext::new();
        assert_eq!(ctx.state, SevLaunchState::Idle);

        ctx.start_launch(0x01);
        assert_eq!(ctx.state, SevLaunchState::Started);
        assert_eq!(ctx.policy, 0x01);

        ctx.update_measurement(b"test measurement data");
        assert_eq!(ctx.state, SevLaunchState::Measuring);
        assert_eq!(&ctx.measurement[..4], b"test");

        ctx.finish_launch();
        assert_eq!(ctx.state, SevLaunchState::Finished);

        ctx.run();
        assert_eq!(ctx.state, SevLaunchState::Running);
    }

    #[test]
    fn test_tdx_context() {
        let mut ctx = TdxContext::new();

        ctx.set_attributes(0x12345678);
        assert_eq!(ctx.attributes, 0x12345678);

        ctx.update_mr_td(b"measurement data");
        assert_eq!(&ctx.mr_td[..11], b"measurement");
    }

    #[test]
    fn test_key_metadata() {
        let mut meta = KeyMetadata::new(KeyId::new(5), true);
        assert_eq!(meta.key_id, KeyId::new(5));
        assert!(meta.is_guest_key);
        assert_eq!(meta.state, KeyState::Uninitialized);
        assert_eq!(meta.page_count, 0);

        meta.state = KeyState::Active;
        meta.page_count = 100;
        assert_eq!(meta.page_count, 100);
    }

    #[test]
    fn test_page_encryption_state() {
        let state = PageEncryptionState::Encrypted(KeyId::new(1));
        assert!(matches!(state, PageEncryptionState::Encrypted(_)));

        let state = PageEncryptionState::Shared;
        assert!(matches!(state, PageEncryptionState::Shared));
    }

    #[test]
    fn test_encryption_error_display() {
        let err = EncryptionError::KeyNotFound(KeyId::new(5));
        assert!(err.to_string().contains("Key not found"));

        let err = EncryptionError::MaxKeysExceeded;
        assert!(err.to_string().contains("Maximum"));
    }

    #[test]
    fn test_key_size_bits() {
        assert_eq!(EncryptionTechnology::None.key_size_bits(), 0);
        assert_eq!(EncryptionTechnology::AmdSev.key_size_bits(), 128);
        assert_eq!(EncryptionTechnology::IntelTdx.key_size_bits(), 128);
    }
}
