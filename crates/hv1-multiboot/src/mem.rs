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

/// Set `n` bytes at `dest` to `c`.
///
/// # Safety
///
/// `dest` must be valid for `n` writes. Called by the compiler, which
/// guarantees that.
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    let byte = c as u8;
    for i in 0..n {
        *dest.add(i) = byte;
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
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
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
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    // Copy in whichever direction does not overwrite a byte before reading it.
    if (dest as usize) < (src as usize) {
        for i in 0..n {
            *dest.add(i) = *src.add(i);
        }
    } else {
        for i in (0..n).rev() {
            *dest.add(i) = *src.add(i);
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
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}
