//! One copy of something large, mapped into many guests.
//!
//! An agent VM costs 0.221 MiB today because it holds nothing but its own code.
//! An agent that runs a model holds the model, and even a small one is hundreds
//! of megabytes: a 0.6-billion-parameter model at four bits is around 350 MB,
//! so a thousand agents each with their own copy is 350 GB and the entire
//! premise of running a thousand of them is gone.
//!
//! They do not need their own copy. Every agent in a fleet runs the *same*
//! weights, reads them and never writes them, which is the exact shape of a
//! shared read-only mapping. This is that mapping.
//!
//! # Why this costs nothing per guest
//!
//! Every VM in this process is backed by host memory in this process. Handing
//! several VMs the same host address for a memory slot does not copy anything —
//! KVM records the mapping, and the pages behind it are the same physical pages
//! for every guest that has it. The host pays once, at the size of the region,
//! however many guests are looking at it.
//!
//! The read-only flag is not a nicety. A writable region shared by a thousand
//! agents is a thousand agents able to rewrite each other's model, which is
//! both a correctness problem and precisely the isolation this project exists
//! to provide. With the flag, a guest that writes takes an MMIO exit the host
//! can see and report.
//!
//! # What it is not
//!
//! Not a filesystem, not a loader, and not paged. The whole region is resident
//! once it is touched, which is the right trade for weights that every agent
//! reads constantly and the wrong one for something read rarely.

use std::sync::Arc;

use crate::{Error, Result};

/// A read-only region of host memory that many guests can be shown.
///
/// Owns its mapping and unmaps it on drop, so the region outlives every VM that
/// borrows it only if the caller keeps the [`Arc`] alive — which is the
/// intended shape: build one, hand it to every agent, drop it when the fleet
/// is gone.
#[derive(Debug)]
pub struct SharedRom {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the mapping is owned exclusively by this value, and everything a
// caller can do with it after construction is read the base address and length.
// Guests write to it only through KVM, which refuses.
unsafe impl Send for SharedRom {}
unsafe impl Sync for SharedRom {}

impl SharedRom {
    /// Allocate `len` bytes and fill them from `contents`.
    ///
    /// Rounded up to a page, because a memory slot is described in pages and
    /// KVM refuses a size that is not a multiple of one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Memory`] if the mapping cannot be made.
    pub fn from_bytes(contents: &[u8]) -> Result<Arc<Self>> {
        let rom = Self::zeroed(contents.len() as u64)?;
        // SAFETY: `rom.ptr` is a fresh mapping of at least `contents.len()`
        // bytes and nothing else refers to it yet.
        unsafe {
            std::ptr::copy_nonoverlapping(contents.as_ptr(), rom.ptr, contents.len());
        }
        Ok(rom)
    }

    /// Allocate `len` bytes of zeroed, page-aligned host memory.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Memory`] if the mapping cannot be made.
    pub fn zeroed(len: u64) -> Result<Arc<Self>> {
        const PAGE: u64 = 4096;
        if len == 0 {
            return Err(Error::Memory("a shared region of zero bytes".into()));
        }
        let len = len.div_ceil(PAGE) * PAGE;

        // SAFETY: a fresh anonymous mapping. The result is checked before use.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(Error::Memory(format!(
                "could not map {len} bytes for a shared region: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(Arc::new(Self {
            ptr: ptr as *mut u8,
            len: len as usize,
        }))
    }

    /// Host address of the first byte. What a memory slot is told.
    pub fn host_addr(&self) -> u64 {
        self.ptr as u64
    }

    /// Size of the region, in bytes and a multiple of a page.
    pub fn len(&self) -> u64 {
        self.len as u64
    }

    /// Whether the region is empty. Never true — [`Self::zeroed`] refuses a
    /// zero length — and present because clippy asks for it next to `len`.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The contents, for a host that wants to check what it published.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `ptr` maps `len` readable bytes for as long as `self` lives.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for SharedRom {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `len` describe this value's own mapping, made in
        // `zeroed` and not unmapped anywhere else.
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.len);
        }
    }
}
