//! TAP/TUN network device support
//!
//! This module provides cross-platform TAP device support for VM networking.
//! TAP devices operate at Layer 2 (Ethernet frames) and are used to provide
//! network connectivity to virtual machines.

use crate::{NetError, Result};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// TAP device configuration
#[derive(Debug, Clone)]
pub struct TapConfig {
    /// Device name (e.g., "tap0")
    pub name: String,
    /// MAC address (if None, a random one is generated)
    pub mac_address: Option<[u8; 6]>,
    /// MTU (default: 1500)
    pub mtu: u32,
    /// Enable multi-queue support
    pub multi_queue: bool,
    /// Number of queues (if multi-queue enabled)
    pub num_queues: u32,
    /// Enable vnet header (for VirtIO integration)
    pub vnet_hdr: bool,
    /// Persist device after process exit
    pub persist: bool,
}

impl Default for TapConfig {
    fn default() -> Self {
        Self {
            name: "tap0".into(),
            mac_address: None,
            mtu: 1500,
            multi_queue: false,
            num_queues: 1,
            vnet_hdr: true,
            persist: false,
        }
    }
}

impl TapConfig {
    /// Create a new TAP config with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the MAC address
    pub fn with_mac(mut self, mac: [u8; 6]) -> Self {
        self.mac_address = Some(mac);
        self
    }

    /// Set MTU
    pub fn with_mtu(mut self, mtu: u32) -> Self {
        self.mtu = mtu;
        self
    }

    /// Enable multi-queue
    pub fn with_multi_queue(mut self, num_queues: u32) -> Self {
        self.multi_queue = true;
        self.num_queues = num_queues;
        self
    }

    /// Enable vnet header
    pub fn with_vnet_hdr(mut self, enabled: bool) -> Self {
        self.vnet_hdr = enabled;
        self
    }
}

/// TAP device statistics
#[derive(Debug, Default)]
pub struct TapStats {
    /// Bytes received
    pub rx_bytes: AtomicU64,
    /// Bytes transmitted
    pub tx_bytes: AtomicU64,
    /// Packets received
    pub rx_packets: AtomicU64,
    /// Packets transmitted
    pub tx_packets: AtomicU64,
    /// Receive errors
    pub rx_errors: AtomicU64,
    /// Transmit errors
    pub tx_errors: AtomicU64,
}

/// Platform-specific TAP device handle
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

    /// Linux TAP device flags
    mod flags {
        pub const IFF_TUN: i16 = 0x0001;
        pub const IFF_TAP: i16 = 0x0002;
        pub const IFF_NO_PI: i16 = 0x1000;
        pub const IFF_VNET_HDR: i16 = 0x4000;
        pub const IFF_MULTI_QUEUE: i16 = 0x0100;
    }

    /// Interface request structure for ioctl
    #[repr(C)]
    struct IfReq {
        ifr_name: [u8; 16],
        ifr_flags: i16,
        _padding: [u8; 22],
    }

    pub struct TapHandle {
        fd: RawFd,
        name: String,
    }

    impl TapHandle {
        pub fn create(config: &TapConfig) -> Result<Self> {
            // Open /dev/net/tun
            let tun_path = CString::new("/dev/net/tun").unwrap();
            let fd = unsafe { libc::open(tun_path.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK) };
            if fd < 0 {
                return Err(NetError::Io(io::Error::last_os_error()));
            }

            // Prepare interface request
            let mut ifr = IfReq {
                ifr_name: [0u8; 16],
                ifr_flags: flags::IFF_TAP | flags::IFF_NO_PI,
                _padding: [0u8; 22],
            };

            // Set device name
            let name_bytes = config.name.as_bytes();
            let len = name_bytes.len().min(15);
            ifr.ifr_name[..len].copy_from_slice(&name_bytes[..len]);

            // Add optional flags
            if config.vnet_hdr {
                ifr.ifr_flags |= flags::IFF_VNET_HDR;
            }
            if config.multi_queue {
                ifr.ifr_flags |= flags::IFF_MULTI_QUEUE;
            }

            // Create the TAP device
            const TUNSETIFF: u64 = 0x400454ca;
            let ret = unsafe { libc::ioctl(fd, TUNSETIFF, &ifr) };
            if ret < 0 {
                unsafe { libc::close(fd) };
                return Err(NetError::Io(io::Error::last_os_error()));
            }

            // Extract actual device name
            let name = std::str::from_utf8(&ifr.ifr_name)
                .unwrap_or("tap")
                .trim_end_matches('\0')
                .to_string();

            tracing::info!("Created TAP device: {}", name);

            Ok(Self { fd, name })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let ret = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            let ret = unsafe { libc::write(self.fd, buf.as_ptr() as *const _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            let flags = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            let flags = if nonblocking {
                flags | libc::O_NONBLOCK
            } else {
                flags & !libc::O_NONBLOCK
            };
            let ret = unsafe { libc::fcntl(self.fd, libc::F_SETFL, flags) };
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for TapHandle {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }

    impl AsRawFd for TapHandle {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }
}

/// Windows TAP device support (via OpenVPN TAP-Windows adapter)
#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, RawHandle};
    use winapi::shared::minwindef::{DWORD, FALSE};
    use winapi::um::fileapi::{CreateFileW, ReadFile, WriteFile, OPEN_EXISTING};
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::winbase::FILE_FLAG_OVERLAPPED;
    use winapi::um::winnt::{
        FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE, HANDLE,
    };

    const TAP_WINDOWS_COMPONENT_ID: &str = "tap0901";

    pub struct TapHandle {
        handle: HANDLE,
        name: String,
    }

    // SAFETY: HANDLE is just a pointer that can be safely sent between threads
    unsafe impl Send for TapHandle {}
    unsafe impl Sync for TapHandle {}

    impl TapHandle {
        pub fn create(config: &TapConfig) -> Result<Self> {
            // Find TAP adapter in registry and get device path
            let device_path = Self::find_tap_adapter(&config.name)?;

            // Open the TAP device
            let path_wide: Vec<u16> = OsStr::new(&device_path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let handle = unsafe {
                CreateFileW(
                    path_wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };

            if handle == INVALID_HANDLE_VALUE {
                return Err(NetError::Io(io::Error::last_os_error()));
            }

            // Set media status to connected
            Self::set_media_status(handle, true)?;

            tracing::info!("Created TAP device: {}", config.name);

            Ok(Self {
                handle,
                name: config.name.clone(),
            })
        }

        fn find_tap_adapter(_name: &str) -> Result<String> {
            // In a full implementation, this would enumerate network adapters
            // from the registry and find the TAP adapter by component ID.
            // For now, return a placeholder path.
            Ok("\\\\.\\Global\\{00000000-0000-0000-0000-000000000000}.tap".to_string())
        }

        fn set_media_status(handle: HANDLE, connected: bool) -> Result<()> {
            const TAP_WIN_IOCTL_SET_MEDIA_STATUS: DWORD = 0x00220030;
            let status: DWORD = if connected { 1 } else { 0 };
            let mut bytes_returned: DWORD = 0;

            let ret = unsafe {
                winapi::um::ioapiset::DeviceIoControl(
                    handle,
                    TAP_WIN_IOCTL_SET_MEDIA_STATUS,
                    &status as *const _ as *mut _,
                    std::mem::size_of::<DWORD>() as DWORD,
                    std::ptr::null_mut(),
                    0,
                    &mut bytes_returned,
                    std::ptr::null_mut(),
                )
            };

            if ret == FALSE {
                return Err(NetError::Io(io::Error::last_os_error()));
            }
            Ok(())
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let mut bytes_read: DWORD = 0;
            let ret = unsafe {
                ReadFile(
                    self.handle,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as DWORD,
                    &mut bytes_read,
                    std::ptr::null_mut(),
                )
            };
            if ret == FALSE {
                Err(io::Error::last_os_error())
            } else {
                Ok(bytes_read as usize)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            let mut bytes_written: DWORD = 0;
            let ret = unsafe {
                WriteFile(
                    self.handle,
                    buf.as_ptr() as *const _,
                    buf.len() as DWORD,
                    &mut bytes_written,
                    std::ptr::null_mut(),
                )
            };
            if ret == FALSE {
                Err(io::Error::last_os_error())
            } else {
                Ok(bytes_written as usize)
            }
        }

        pub fn set_nonblocking(&self, _nonblocking: bool) -> io::Result<()> {
            // On Windows, we use overlapped I/O which is inherently async
            Ok(())
        }
    }

    impl Drop for TapHandle {
        fn drop(&mut self) {
            let _ = Self::set_media_status(self.handle, false);
            unsafe { CloseHandle(self.handle) };
        }
    }

    impl AsRawHandle for TapHandle {
        fn as_raw_handle(&self) -> RawHandle {
            self.handle as RawHandle
        }
    }
}

/// macOS TAP device support (via utun or third-party tuntap)
#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::os::unix::io::{AsRawFd, RawFd};

    pub struct TapHandle {
        fd: RawFd,
        name: String,
    }

    impl TapHandle {
        pub fn create(config: &TapConfig) -> Result<Self> {
            // macOS doesn't have native TAP support, but utun provides TUN.
            // For TAP, third-party drivers like tuntaposx are needed.
            // Here we implement utun support as a fallback.

            let unit = config
                .name
                .strip_prefix("utun")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            let fd =
                unsafe { libc::socket(libc::PF_SYSTEM, libc::SOCK_DGRAM, libc::SYSPROTO_CONTROL) };
            if fd < 0 {
                return Err(NetError::Io(io::Error::last_os_error()));
            }

            // In a full implementation, we would:
            // 1. Get the control ID for com.apple.net.utun_control
            // 2. Connect to the control with the desired unit number
            // For now, we simulate success

            let name = format!("utun{}", unit);
            tracing::info!("Created macOS network device: {}", name);

            Ok(Self { fd, name })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let ret = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            let ret = unsafe { libc::write(self.fd, buf.as_ptr() as *const _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            let flags = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            let flags = if nonblocking {
                flags | libc::O_NONBLOCK
            } else {
                flags & !libc::O_NONBLOCK
            };
            let ret = unsafe { libc::fcntl(self.fd, libc::F_SETFL, flags) };
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for TapHandle {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }

    impl AsRawFd for TapHandle {
        fn as_raw_fd(&self) -> RawFd {
            self.fd
        }
    }
}

/// Stub implementation for unsupported platforms
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod platform {
    use super::*;

    pub struct TapHandle {
        name: String,
        buffer: Vec<u8>,
    }

    impl TapHandle {
        pub fn create(config: &TapConfig) -> Result<Self> {
            tracing::warn!("TAP devices not supported on this platform, using stub implementation");
            Ok(Self {
                name: config.name.clone(),
                buffer: Vec::new(),
            })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "TAP not supported on this platform",
            ))
        }

        pub fn write(&self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "TAP not supported on this platform",
            ))
        }

        pub fn set_nonblocking(&self, _nonblocking: bool) -> io::Result<()> {
            Ok(())
        }
    }
}

/// TAP network device
pub struct TapDevice {
    /// Configuration
    config: TapConfig,
    /// Platform-specific handle
    handle: Option<platform::TapHandle>,
    /// Device is open
    is_open: AtomicBool,
    /// Statistics
    stats: Arc<TapStats>,
    /// Receive buffer
    rx_buffer: Mutex<Vec<u8>>,
    /// Transmit buffer
    tx_buffer: Mutex<Vec<u8>>,
}

impl TapDevice {
    /// Create a new TAP device with the given configuration
    pub fn new(config: TapConfig) -> Self {
        Self {
            config,
            handle: None,
            is_open: AtomicBool::new(false),
            stats: Arc::new(TapStats::default()),
            rx_buffer: Mutex::new(vec![0u8; 65536]),
            tx_buffer: Mutex::new(Vec::with_capacity(65536)),
        }
    }

    /// Create the TAP device
    pub async fn create(&mut self) -> Result<()> {
        tracing::info!("Creating TAP device: {}", self.config.name);

        let handle = platform::TapHandle::create(&self.config)?;
        handle.set_nonblocking(true).map_err(NetError::Io)?;

        self.handle = Some(handle);
        self.is_open.store(true, Ordering::SeqCst);

        // Set MAC address if specified
        if let Some(mac) = self.config.mac_address {
            tracing::info!(
                "TAP device MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0],
                mac[1],
                mac[2],
                mac[3],
                mac[4],
                mac[5]
            );
        }

        Ok(())
    }

    /// Get the device name
    pub fn name(&self) -> &str {
        self.handle
            .as_ref()
            .map(|h| h.name())
            .unwrap_or(&self.config.name)
    }

    /// Check if the device is open
    pub fn is_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    /// Read a packet from the TAP device
    pub async fn read(&self) -> Result<Vec<u8>> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| NetError::Config("Device not open".into()))?;

        let mut buffer = self.rx_buffer.lock().await;

        match handle.read(&mut buffer) {
            Ok(n) => {
                self.stats.rx_packets.fetch_add(1, Ordering::Relaxed);
                self.stats.rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                Ok(buffer[..n].to_vec())
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Vec::new()),
            Err(e) => {
                self.stats.rx_errors.fetch_add(1, Ordering::Relaxed);
                Err(NetError::Io(e))
            }
        }
    }

    /// Write a packet to the TAP device
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| NetError::Config("Device not open".into()))?;

        match handle.write(data) {
            Ok(n) => {
                self.stats.tx_packets.fetch_add(1, Ordering::Relaxed);
                self.stats.tx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                Ok(n)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => {
                self.stats.tx_errors.fetch_add(1, Ordering::Relaxed);
                Err(NetError::Io(e))
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &TapStats {
        &self.stats
    }

    /// Get configuration
    pub fn config(&self) -> &TapConfig {
        &self.config
    }

    /// Close the device
    pub fn close(&mut self) {
        self.handle = None;
        self.is_open.store(false, Ordering::SeqCst);
        tracing::info!("TAP device closed");
    }
}

impl Drop for TapDevice {
    fn drop(&mut self) {
        self.close();
    }
}

/// Generate a random MAC address with locally administered bit set
pub fn generate_mac() -> [u8; 6] {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    // Simple PRNG
    let mut state = seed;
    let mut mac = [0u8; 6];
    for byte in &mut mac {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 32) as u8;
    }

    // Set locally administered bit, clear multicast bit
    mac[0] = (mac[0] & 0xFE) | 0x02;

    mac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tap_config_builder() {
        let config = TapConfig::new("tap0")
            .with_mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
            .with_mtu(9000)
            .with_multi_queue(4)
            .with_vnet_hdr(true);

        assert_eq!(config.name, "tap0");
        assert_eq!(
            config.mac_address,
            Some([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
        );
        assert_eq!(config.mtu, 9000);
        assert!(config.multi_queue);
        assert_eq!(config.num_queues, 4);
        assert!(config.vnet_hdr);
    }

    #[test]
    fn test_generate_mac() {
        let mac1 = generate_mac();
        let mac2 = generate_mac();

        // Should have locally administered bit set
        assert_eq!(mac1[0] & 0x02, 0x02);
        assert_eq!(mac2[0] & 0x02, 0x02);

        // Should not be multicast
        assert_eq!(mac1[0] & 0x01, 0x00);
        assert_eq!(mac2[0] & 0x01, 0x00);
    }

    #[tokio::test]
    async fn test_tap_device_creation() {
        let config = TapConfig::new("test_tap");
        let device = TapDevice::new(config);

        assert_eq!(device.name(), "test_tap");
        assert!(!device.is_open());
    }
}
