//! Count allocations on the calling thread; other test-runner threads are excluded.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use hv2_core::boot::linux::LinuxBootParams;
use hv2_core::boot::source::LoadedBoot;

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

fn record(size: usize) {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = BYTES.try_with(|bytes| bytes.set(bytes.get().saturating_add(size)));
    }
}

struct Counting;
// SAFETY: all memory operations delegate unchanged to the system allocator.
// Accounting uses constant-initialized thread-local cells and allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the allocator's required valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the allocator's required valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: allocation ownership and layout are unchanged by this wrapper.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        // SAFETY: allocation ownership, layout and requested size pass through.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn measured<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    BYTES.with(|bytes| bytes.set(0));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, BYTES.with(Cell::get))
}

#[test]
fn linux_sizing_does_not_allocate_kernel_or_initrd_copies() {
    let mut kernel = vec![0u8; 16 * 1024 * 1024];
    kernel[0x1f1] = 4;
    kernel[0x1fe..0x200].copy_from_slice(&[0x55, 0xaa]);
    kernel[0x202..0x206].copy_from_slice(b"HdrS");
    kernel[0x206..0x208].copy_from_slice(&0x020cu16.to_le_bytes());
    let boot = LoadedBoot::Linux(Box::new(LinuxBootParams {
        kernel_image: kernel,
        initrd: Some(vec![0xab; 8 * 1024 * 1024]),
        memory_size: 1024 * 1024 * 1024,
        ..LinuxBootParams::default()
    }));
    let (sized, sizing_bytes) = measured(|| boot.highest_address().unwrap());
    let (regions, region_bytes) = measured(|| boot.memory_regions().unwrap());
    assert_eq!(
        sized,
        regions
            .iter()
            .map(|(addr, data)| addr + data.len() as u64)
            .max()
            .unwrap()
    );
    assert_eq!(
        sizing_bytes, 0,
        "valid sizing should allocate no copied regions"
    );
    assert!(
        region_bytes >= 24 * 1024 * 1024 - 4096,
        "control must observe the image copies it counts"
    );
}
