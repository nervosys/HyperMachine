//! The four memory routines the compiler assumes exist.
//!
//! LLVM lowers array initialisation, struct copies and slice comparisons into
//! calls to `memset`, `memcpy`, `memmove` and `memcmp` whatever the language
//! is, and normally libc supplies them. This guest has no libc, and
//! `compiler_builtins` only offers its own copies behind a feature that
//! `-Z build-std` would be needed to turn on — which is exactly the nightly
//! dependency this crate is built to avoid.
//!
//! So they live here. The first symptom of their absence is a link error
//! naming `memset` and pointing at a function that does not mention memory at
//! all — in this crate's case, one that declared a `[u8; 256]`.
//!
//! Written to be obviously correct rather than fast. A unikernel that moves
//! enough bytes for a byte-at-a-time loop to matter has outgrown this file,
//! and the version that is fast is the version with the off-by-one in it.
//!
//! The signatures are libc's, in `c_void` rather than `u8`. They took `*mut
//! u8` until clippy's `suspicious_runtime_symbol_definition` pointed out that
//! the standard library calls these with the C signature: the two agree on
//! every ABI this targets, so it worked, but a symbol the compiler emits calls
//! to is the wrong place to be relying on that. The casts moved inward instead.

use core::ffi::c_void;

/// Set `n` bytes at `dest` to `c`.
///
/// # Safety
///
/// `dest` must be valid for `n` writes. Called by the compiler, which
/// guarantees that.
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let byte = c as u8;
    let bytes = dest.cast::<u8>();
    for i in 0..n {
        *bytes.add(i) = byte;
    }
    dest
}

/// Copy `n` bytes from `src` to `dest`, which must not overlap.
///
/// # Safety
///
/// `src` must be valid for `n` reads, `dest` for `n` writes, and the two must
/// not overlap. Called by the compiler, which guarantees that.
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (to, from) = (dest.cast::<u8>(), src.cast::<u8>());
    for i in 0..n {
        *to.add(i) = *from.add(i);
    }
    dest
}

/// Copy `n` bytes from `src` to `dest`, which may overlap.
///
/// # Safety
///
/// `src` must be valid for `n` reads and `dest` for `n` writes. Overlap is
/// permitted, which is the whole difference from `memcpy`.
#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (to, from) = (dest.cast::<u8>(), src.cast::<u8>());
    // Copy in whichever direction does not overwrite a byte before reading it.
    if (to as usize) < (from as usize) {
        for i in 0..n {
            *to.add(i) = *from.add(i);
        }
    } else {
        for i in (0..n).rev() {
            *to.add(i) = *from.add(i);
        }
    }
    dest
}

/// Compare `n` bytes of `a` and `b`.
///
/// # Safety
///
/// Both pointers must be valid for `n` reads.
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    for i in 0..n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}
