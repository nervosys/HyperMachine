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

// `Error` is only constructed by the Linux and Windows pinning paths; on other
// platforms `pin_current_thread` is an infallible no-op.
#[cfg(any(target_os = "linux", windows))]
use crate::Error;
use crate::Result;

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

/// The host NUMA node a given logical core belongs to, if the topology can be
/// queried. Returns `None` when unknown (caller falls back to host-default
/// memory placement).
pub fn numa_node_for_core(core: usize) -> Option<u32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::GetNumaProcessorNode;
        if core > u8::MAX as usize {
            return None;
        }
        let mut node: u8 = 0;
        // SAFETY: writes a single u8; returns 0 (FALSE) on failure.
        let ok = unsafe { GetNumaProcessorNode(core as u8, &mut node) };
        if ok != 0 {
            Some(node as u32)
        } else {
            None
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Find the node whose `cpulist` contains `core`.
        let dir = std::fs::read_dir("/sys/devices/system/node").ok()?;
        for entry in dir.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(num) = name
                .strip_prefix("node")
                .and_then(|n| n.parse::<u32>().ok())
            {
                let path = format!("/sys/devices/system/node/{name}/cpulist");
                if let Ok(list) = std::fs::read_to_string(&path) {
                    if cpulist_contains(&list, core) {
                        return Some(num);
                    }
                }
            }
        }
        None
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = core;
        None
    }
}

/// Whether a Linux sysfs `cpulist` string (e.g. `"0-3,8,12-15"`) includes
/// `core`. Kept platform-independent so the range parsing is unit-tested
/// everywhere even though it is only consulted on Linux.
fn cpulist_contains(list: &str, core: usize) -> bool {
    for part in list.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                if core >= a && core <= b {
                    return true;
                }
            }
        } else if part.parse::<usize>() == Ok(core) {
            return true;
        }
    }
    false
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
    fn cpulist_parsing() {
        let list = "0-3,8,12-15";
        assert!(cpulist_contains(list, 0));
        assert!(cpulist_contains(list, 2));
        assert!(cpulist_contains(list, 3));
        assert!(cpulist_contains(list, 8));
        assert!(cpulist_contains(list, 13));
        assert!(cpulist_contains(list, 15));
        assert!(!cpulist_contains(list, 4));
        assert!(!cpulist_contains(list, 9));
        assert!(!cpulist_contains(list, 16));
        // Trailing newline (as sysfs returns) is tolerated.
        assert!(cpulist_contains("0-1\n", 1));
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
