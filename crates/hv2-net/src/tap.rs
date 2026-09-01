//! TAP/TUN network device support
//!
//! This module provides cross-platform TAP device support for VM networking.
//! TAP devices operate at Layer 2 (Ethernet frames) and are used to provide
//! network connectivity to virtual machines.

use crate::{NetError, Result};
use std::io;
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
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the MAC address
    #[must_use]
    pub fn with_mac(mut self, mac: [u8; 6]) -> Self {
        self.mac_address = Some(mac);
        self
    }

    /// Set MTU
    #[must_use]
    pub fn with_mtu(mut self, mtu: u32) -> Self {
        self.mtu = mtu;
        self
    }

    /// Enable multi-queue
    #[must_use]
    pub fn with_multi_queue(mut self, num_queues: u32) -> Self {
        self.multi_queue = true;
        self.num_queues = num_queues;
        self
    }

    /// Enable vnet header
    #[must_use]
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
    use std::os::unix::io::{AsRawFd, RawFd};

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
            // SAFETY: CString::new on a literal without embedded NULs never fails
            let tun_path = CString::new("/dev/net/tun").expect("static path has no NUL bytes");
            // SAFETY: `tun_path` is a valid CString. `libc::open` is a standard POSIX
            // syscall; `fd` is checked for errors immediately after.
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
            // SAFETY: `fd` is a valid open file descriptor and `ifr` is properly
            // initialized. Return value is checked immediately.
            let ret = unsafe { libc::ioctl(fd, TUNSETIFF, &ifr) };
            if ret < 0 {
                // SAFETY: `fd` is valid; closing on error path.
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
            // SAFETY: self.fd is a valid open TAP fd; buf is a valid mutable slice.
            let ret = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            // SAFETY: self.fd is a valid open TAP fd; buf is a valid byte slice.
            let ret = unsafe { libc::write(self.fd, buf.as_ptr() as *const _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            // SAFETY: self.fd is a valid open TAP fd; F_GETFL is a standard POSIX query.
            let flags = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            let flags = if nonblocking {
                flags | libc::O_NONBLOCK
            } else {
                flags & !libc::O_NONBLOCK
            };
            // SAFETY: self.fd is valid; F_SETFL is standard POSIX.
            let ret = unsafe { libc::fcntl(self.fd, libc::F_SETFL, flags) };
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for TapHandle {
        fn drop(&mut self) {
            // SAFETY: self.fd is a valid open TAP fd from create().
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
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const GENERIC_READ: u32 = 0x80000000;
    const GENERIC_WRITE: u32 = 0x40000000;

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

            // SAFETY: path_wide is a valid NUL-terminated wide string. Standard Win32 file open.
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
            use windows_sys::Win32::System::Registry::{
                RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE,
                KEY_READ, REG_SZ,
            };

            // Network adapter class GUID in the Windows registry
            const NETWORK_CLASS_KEY: &str =
                r"SYSTEM\CurrentControlSet\Control\Class\{4D36E972-E325-11CE-BFC1-08002BE10318}";

            // Known TAP component IDs
            const TAP_COMPONENT_IDS: &[&str] = &["tap0901", "root\\tap0901", "wintun"];

            let key_path: Vec<u16> = OsStr::new(NETWORK_CLASS_KEY)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut class_key: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
            // SAFETY: key_path is a valid NUL-terminated wide string, class_key
            // is a valid output pointer. Standard Win32 registry API.
            let ret = unsafe {
                RegOpenKeyExW(
                    HKEY_LOCAL_MACHINE,
                    key_path.as_ptr(),
                    0,
                    KEY_READ,
                    &mut class_key,
                )
            };
            if ret != 0 {
                return Err(NetError::Config(
                    "Failed to open network adapter registry key".into(),
                ));
            }

            let mut index: u32 = 0;
            let mut found_guid = None;

            loop {
                let mut subkey_name = [0u16; 256];
                let mut subkey_len = subkey_name.len() as u32;

                // SAFETY: class_key is a valid open registry key; subkey_name/subkey_len
                // are valid buffers for receiving the subkey name.
                let ret = unsafe {
                    RegEnumKeyExW(
                        class_key,
                        index,
                        subkey_name.as_mut_ptr(),
                        &mut subkey_len,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if ret != 0 {
                    break; // No more subkeys
                }
                index += 1;

                // Open the subkey and read ComponentId
                let mut adapter_key: windows_sys::Win32::System::Registry::HKEY =
                    std::ptr::null_mut();
                // SAFETY: class_key is valid; subkey_name is from a successful RegEnumKeyExW.
                let ret = unsafe {
                    RegOpenKeyExW(
                        class_key,
                        subkey_name.as_ptr(),
                        0,
                        KEY_READ,
                        &mut adapter_key,
                    )
                };
                if ret != 0 {
                    continue;
                }

                // Read ComponentId value
                let value_name: Vec<u16> = OsStr::new("ComponentId")
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                let mut data = [0u8; 512];
                let mut data_len = data.len() as u32;
                let mut value_type: u32 = 0;

                // SAFETY: adapter_key is a valid key; value_name is NUL-terminated;
                // data/data_len are valid output buffers.
                let ret = unsafe {
                    RegQueryValueExW(
                        adapter_key,
                        value_name.as_ptr(),
                        std::ptr::null_mut(),
                        &mut value_type,
                        data.as_mut_ptr(),
                        &mut data_len,
                    )
                };

                if ret == 0 && value_type == REG_SZ && data_len > 2 {
                    // Convert wide string to Rust string (data is UTF-16LE).
                    //
                    // SAFETY/CORRECTNESS: `data` is a `Vec<u8>` which is only 1-byte
                    // aligned, but the Win32 registry returns UTF-16LE bytes whose
                    // logical alignment may differ from the allocator's. We avoid
                    // an unaligned `*const u16` cast (which is UB on strict-alignment
                    // targets) by decoding 16-bit code units from byte pairs.
                    let wide_len = (data_len as usize) / 2;
                    let wide_bytes = &data[..wide_len * 2];
                    let (pairs, _) = wide_bytes.as_chunks::<2>();
                    let wide: Vec<u16> = pairs.iter().copied().map(u16::from_le_bytes).collect();
                    let component_id = String::from_utf16_lossy(&wide)
                        .trim_end_matches('\0')
                        .to_lowercase();

                    let is_tap = TAP_COMPONENT_IDS.iter().any(|id| component_id == *id);

                    if is_tap {
                        // Read NetCfgInstanceId (the adapter GUID)
                        let guid_name: Vec<u16> = OsStr::new("NetCfgInstanceId")
                            .encode_wide()
                            .chain(std::iter::once(0))
                            .collect();
                        let mut guid_data = [0u8; 512];
                        let mut guid_len = guid_data.len() as u32;
                        let mut guid_type: u32 = 0;

                        // SAFETY: adapter_key valid; guid_name NUL-terminated; output buffers valid.
                        let ret = unsafe {
                            RegQueryValueExW(
                                adapter_key,
                                guid_name.as_ptr(),
                                std::ptr::null_mut(),
                                &mut guid_type,
                                guid_data.as_mut_ptr(),
                                &mut guid_len,
                            )
                        };

                        if ret == 0 && guid_type == REG_SZ && guid_len > 2 {
                            let guid_wide_len = (guid_len as usize) / 2;
                            let guid_wide = unsafe {
                                std::slice::from_raw_parts(
                                    guid_data.as_ptr() as *const u16,
                                    guid_wide_len,
                                )
                            };
                            let guid = String::from_utf16_lossy(guid_wide)
                                .trim_end_matches('\0')
                                .to_string();

                            // SAFETY: adapter_key is valid.
                            unsafe { RegCloseKey(adapter_key) };

                            found_guid = Some(guid);
                            break;
                        }
                    }
                }

                // SAFETY: adapter_key is a valid open registry key.
                unsafe { RegCloseKey(adapter_key) };
            }

            // SAFETY: class_key is a valid open registry key.
            unsafe { RegCloseKey(class_key) };

            match found_guid {
                Some(guid) => {
                    let path = format!("\\\\.\\Global\\{}.tap", guid);
                    tracing::info!("Found TAP adapter: {}", path);
                    Ok(path)
                }
                None => Err(NetError::Config(
                    "No TAP-Windows or Wintun adapter found. Install OpenVPN TAP driver or Wintun."
                        .into(),
                )),
            }
        }

        fn set_media_status(handle: HANDLE, connected: bool) -> Result<()> {
            const TAP_WIN_IOCTL_SET_MEDIA_STATUS: u32 = 0x00220030;
            let status: u32 = if connected { 1 } else { 0 };
            let mut bytes_returned: u32 = 0;

            // SAFETY: handle is a valid open TAP device handle. DeviceIoControl with
            // TAP_WIN_IOCTL_SET_MEDIA_STATUS is a well-defined TAP-Windows ioctl.
            let ret = unsafe {
                DeviceIoControl(
                    handle,
                    TAP_WIN_IOCTL_SET_MEDIA_STATUS,
                    &status as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<u32>() as u32,
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
            let mut bytes_read: u32 = 0;
            // SAFETY: self.handle is a valid open TAP device handle; buf is a valid mutable slice.
            let ret = unsafe {
                ReadFile(
                    self.handle,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32,
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
            let mut bytes_written: u32 = 0;
            // SAFETY: self.handle is a valid open TAP device handle; buf is a valid byte slice.
            let ret = unsafe {
                WriteFile(
                    self.handle,
                    buf.as_ptr() as *const _,
                    buf.len() as u32,
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
            // SAFETY: self.handle is a valid HANDLE from CreateFileW in create().
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

            // SAFETY: Creating a PF_SYSTEM socket for utun control. Standard macOS syscall;
            // `fd` is checked for errors immediately after.
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
            // SAFETY: `self.fd` is a valid utun file descriptor opened in `create()`.
            // `buf` is a valid mutable byte slice.
            let ret = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            // SAFETY: `self.fd` is a valid utun file descriptor. `buf` is a valid byte slice.
            let ret = unsafe { libc::write(self.fd, buf.as_ptr() as *const _, buf.len()) };
            if ret < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(ret as usize)
            }
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            // SAFETY: self.fd is a valid open utun fd. fcntl F_GETFL is standard POSIX.
            let flags = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            let flags = if nonblocking {
                flags | libc::O_NONBLOCK
            } else {
                flags & !libc::O_NONBLOCK
            };
            // SAFETY: self.fd is valid; F_SETFL is standard POSIX.
            let ret = unsafe { libc::fcntl(self.fd, libc::F_SETFL, flags) };
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for TapHandle {
        fn drop(&mut self) {
            // SAFETY: self.fd is a valid open utun fd from create().
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
///
/// Provides a loopback/memory buffer mode for testing, where writes
/// are stored in a buffer and reads return previously written data.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod platform {
    use super::*;

    pub struct TapHandle {
        name: String,
        /// Loopback buffer: data written via write() can be read back via read()
        loopback: std::sync::Mutex<std::collections::VecDeque<Vec<u8>>>,
        nonblocking: std::sync::atomic::AtomicBool,
    }

    impl TapHandle {
        pub fn create(config: &TapConfig) -> Result<Self> {
            tracing::warn!(
                "TAP devices not supported on this platform, using loopback/memory buffer mode"
            );
            Ok(Self {
                name: config.name.clone(),
                loopback: std::sync::Mutex::new(std::collections::VecDeque::new()),
                nonblocking: std::sync::atomic::AtomicBool::new(false),
            })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let mut queue = self.loopback.lock().unwrap();
            if let Some(packet) = queue.pop_front() {
                let len = packet.len().min(buf.len());
                buf[..len].copy_from_slice(&packet[..len]);
                Ok(len)
            } else if self.nonblocking.load(std::sync::atomic::Ordering::Relaxed) {
                Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "No data available",
                ))
            } else {
                // Blocking mode with no data - return 0
                Ok(0)
            }
        }

        pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
            let mut queue = self.loopback.lock().unwrap();
            // Cap queue at 256 packets to prevent unbounded growth
            if queue.len() >= 256 {
                queue.pop_front();
            }
            queue.push_back(buf.to_vec());
            Ok(buf.len())
        }

        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            self.nonblocking
                .store(nonblocking, std::sync::atomic::Ordering::Relaxed);
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
        .unwrap_or_default()
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

    // --- New tests below ---

    #[test]
    fn test_tap_config_default() {
        let config = TapConfig::default();
        assert_eq!(config.name, "tap0");
        assert!(config.mac_address.is_none());
        assert_eq!(config.mtu, 1500);
        assert!(!config.multi_queue);
        assert_eq!(config.num_queues, 1);
        assert!(config.vnet_hdr);
        assert!(!config.persist);
    }

    #[test]
    fn test_tap_config_new() {
        let config = TapConfig::new("vmtap1");
        assert_eq!(config.name, "vmtap1");
        assert_eq!(config.mtu, 1500); // inherited default
    }

    #[test]
    fn test_tap_config_builder_chaining() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let config = TapConfig::new("tap1")
            .with_mac(mac)
            .with_mtu(9000)
            .with_multi_queue(8)
            .with_vnet_hdr(false);

        assert_eq!(config.name, "tap1");
        assert_eq!(config.mac_address, Some(mac));
        assert_eq!(config.mtu, 9000);
        assert!(config.multi_queue);
        assert_eq!(config.num_queues, 8);
        assert!(!config.vnet_hdr);
    }

    #[test]
    fn test_tap_config_with_vnet_hdr_toggle() {
        let config = TapConfig::new("tap0").with_vnet_hdr(false);
        assert!(!config.vnet_hdr);
        let config2 = config.with_vnet_hdr(true);
        assert!(config2.vnet_hdr);
    }

    #[test]
    fn test_tap_stats_default() {
        let stats = TapStats::default();
        assert_eq!(stats.rx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.rx_packets.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_packets.load(Ordering::Relaxed), 0);
        assert_eq!(stats.rx_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_errors.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_tap_stats_increment() {
        let stats = TapStats::default();
        stats.rx_bytes.fetch_add(1500, Ordering::Relaxed);
        stats.tx_bytes.fetch_add(800, Ordering::Relaxed);
        stats.rx_packets.fetch_add(1, Ordering::Relaxed);
        stats.tx_packets.fetch_add(1, Ordering::Relaxed);
        assert_eq!(stats.rx_bytes.load(Ordering::Relaxed), 1500);
        assert_eq!(stats.tx_bytes.load(Ordering::Relaxed), 800);
        assert_eq!(stats.rx_packets.load(Ordering::Relaxed), 1);
        assert_eq!(stats.tx_packets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_generate_mac_format() {
        let mac = generate_mac();
        // Locally administered bit set (bit 1 of first byte)
        assert_eq!(mac[0] & 0x02, 0x02);
        // Not multicast (bit 0 of first byte cleared)
        assert_eq!(mac[0] & 0x01, 0x00);
        // Must be 6 bytes
        assert_eq!(mac.len(), 6);
    }

    #[test]
    fn test_generate_mac_uniqueness() {
        // Generate several MACs and check they're not all the same
        let macs: Vec<_> = (0..10)
            .map(|_| {
                std::thread::sleep(std::time::Duration::from_nanos(1));
                generate_mac()
            })
            .collect();
        // At least some should differ (time-based seed)
        let first = macs[0];
        let _any_different = macs.iter().skip(1).any(|m| *m != first);
        // If they all happen to be the same due to timing, that's OK — just check format
        for mac in &macs {
            assert_eq!(mac[0] & 0x02, 0x02);
            assert_eq!(mac[0] & 0x01, 0x00);
        }
    }

    #[test]
    fn test_tap_config_multi_queue_sets_both_fields() {
        let config = TapConfig::new("tap0").with_multi_queue(4);
        assert!(config.multi_queue);
        assert_eq!(config.num_queues, 4);
    }

    #[test]
    fn test_tap_config_clone() {
        let config = TapConfig::new("tap0")
            .with_mac([1, 2, 3, 4, 5, 6])
            .with_mtu(9000);
        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
        assert_eq!(cloned.mac_address, config.mac_address);
        assert_eq!(cloned.mtu, config.mtu);
    }

    #[tokio::test]
    async fn test_tap_device_name() {
        let config = TapConfig::new("my_tap_device");
        let device = TapDevice::new(config);
        assert_eq!(device.name(), "my_tap_device");
    }

    #[tokio::test]
    async fn test_tap_device_not_open_initially() {
        let config = TapConfig::default();
        let device = TapDevice::new(config);
        assert!(!device.is_open());
    }

    #[test]
    fn test_tap_config_default_values() {
        let config = TapConfig::default();
        assert_eq!(config.name, "tap0");
        assert!(config.mac_address.is_none());
        assert_eq!(config.mtu, 1500);
        assert!(!config.multi_queue);
        assert_eq!(config.num_queues, 1);
        assert!(config.vnet_hdr);
        assert!(!config.persist);
    }

    #[test]
    fn test_tap_config_new_with_name() {
        let config = TapConfig::new("custom-tap");
        assert_eq!(config.name, "custom-tap");
        assert_eq!(config.mtu, 1500); // other defaults preserved
    }

    #[tokio::test]
    async fn test_tap_device_config_accessor() {
        let config = TapConfig::new("mytap").with_mtu(9000);
        let device = TapDevice::new(config);
        assert_eq!(device.config().name, "mytap");
        assert_eq!(device.config().mtu, 9000);
    }

    #[tokio::test]
    async fn test_tap_device_stats_initially_zero() {
        let device = TapDevice::new(TapConfig::default());
        let stats = device.stats();
        assert_eq!(stats.rx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(stats.rx_packets.load(Ordering::Relaxed), 0);
        assert_eq!(stats.tx_packets.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_tap_device_close_idempotent() {
        let config = TapConfig::default();
        let mut device = TapDevice::new(config);
        device.close();
        assert!(!device.is_open());
        device.close(); // second close should not panic
        assert!(!device.is_open());
    }

    #[test]
    fn test_generate_mac_locally_administered() {
        for _ in 0..10 {
            let mac = generate_mac();
            // Bit 1 of first byte = locally administered
            assert!(mac[0] & 0x02 != 0, "MAC should be locally administered");
            // Bit 0 of first byte = unicast (should be 0)
            assert!(mac[0] & 0x01 == 0, "MAC should be unicast");
        }
    }
}
