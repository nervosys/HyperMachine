//! Cross-platform CPU affinity for pinning vCPU threads to host cores.
//!
//! Pinning a vCPU's OS thread to a fixed physical core keeps its working set
//! warm in that core's caches and avoids cross-core / cross-NUMA migration —
//! the dominant source of tail-latency jitter for AI inference and of cache
//! thrash for training. [`pin_current_thread`] is the low-level primitive; the
//! VM runtime calls it from a dedicated per-vCPU thread (see
//! [`crate::vm::VMConfig::vcpu_affinity`]).
//!
//! Platform support: Linux (`sched_setaffinity`) and Windows
//! (`SetThreadAffinityMask`) pin for real; other platforms (e.g. macOS, whose
//! Hypervisor.framework runs one VM per process and offers no hard per-thread
//! pinning) treat it as a best-effort no-op.

use crate::{Error, Result};

/// Number of logical host cores available for scheduling.
pub fn core_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Number of host NUMA nodes (always `>= 1`).
///
/// Used to validate a VM's requested memory node. Falls back to `1` (a single
/// node) when the host topology cannot be queried.
pub fn numa_node_count() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetNumaHighestNodeNumber;
        let mut highest: u32 = 0;
        // SAFETY: writes a single u32; returns 0 (FALSE) on failure.
        let ok = unsafe { GetNumaHighestNodeNumber(&mut highest) };
        if ok != 0 {
            highest.saturating_add(1)
        } else {
            1
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Count `nodeN` directories under the sysfs NUMA node tree.
        let count = std::fs::read_dir("/sys/devices/system/node")
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name();
                        let name = name.to_string_lossy();
                        name.strip_prefix("node")
                            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                    })
                    .count()
            })
            .unwrap_or(0);
        (count as u32).max(1)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        1
    }
}

/// Pin the **current OS thread** to a single host core.
///
/// Returns an error if the OS rejects the request (e.g. the core does not
/// exist). On platforms without hard per-thread pinning this is a no-op that
/// returns `Ok(())`.
#[cfg(target_os = "linux")]
pub fn pin_current_thread(core: usize) -> Result<()> {
    // SAFETY: a zeroed `cpu_set_t` is a valid empty set; CPU_SET writes only the
    // bit for `core`; sched_setaffinity reads `set` for `size_of` bytes. pid 0
    // targets the calling thread.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(core, &mut set);
        let rc = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
        if rc != 0 {
            return Err(Error::Config(format!(
                "sched_setaffinity(core {core}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    Ok(())
}

/// Pin the current OS thread to a single host core (Windows).
#[cfg(windows)]
pub fn pin_current_thread(core: usize) -> Result<()> {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

    if core >= usize::BITS as usize {
        return Err(Error::Config(format!(
            "core {core} exceeds the {}-bit affinity mask width",
            usize::BITS
        )));
    }
    let mask: usize = 1usize << core;
    // SAFETY: GetCurrentThread returns a valid pseudo-handle for the calling
    // thread; SetThreadAffinityMask takes that handle and a mask and returns the
    // previous mask, or 0 on failure.
    let prev = unsafe { SetThreadAffinityMask(GetCurrentThread(), mask) };
    if prev == 0 {
        return Err(Error::Config(format!(
            "SetThreadAffinityMask(core {core}) failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

/// Best-effort no-op on platforms without hard per-thread pinning.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn pin_current_thread(_core: usize) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_count_is_positive() {
        assert!(core_count() >= 1);
    }

    #[test]
    fn numa_node_count_is_positive() {
        assert!(numa_node_count() >= 1);
    }

    #[test]
    fn pin_to_core_zero_succeeds() {
        // Core 0 always exists. Run on a throwaway thread so we don't leave the
        // test runner pinned.
        std::thread::spawn(|| pin_current_thread(0))
            .join()
            .expect("thread panicked")
            .expect("pinning to core 0 should succeed");
    }
}
