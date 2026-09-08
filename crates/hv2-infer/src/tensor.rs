//! The arithmetic: dequantise a row of weights and dot it with a vector.
//!
//! This is the whole of what a forward pass costs. Everything else in a
//! transformer — the norms, the softmax, the rotations — is linear in the width
//! of the model; the matrix-vector products are linear in its *parameters*, and
//! a 1.2-billion-parameter model in `Q8_0` is 1.3 GB of weights read per token.
//!
//! That number is why this file exists in the shape it does. `shared_weights`
//! measured a guest streaming a region at 1.38 cycles per byte warm, and the
//! arithmetic here is the same loop: read a weight, multiply by an activation,
//! accumulate. The measurement was taken before there were any weights, as the
//! floor a model would stand on. This is the model standing on it.

use rayon::prelude::*;

use crate::gguf::{Gguf, Quant, TensorInfo};

/// Elements in one `Q8_0` block, and the bytes that hold them.
const Q8_BLOCK: usize = 32;
const Q8_BYTES: usize = 34;

/// Rows of a matrix given to one thread at a time.
///
/// One row was the obvious unit and the wrong one. A row is a couple of
/// kilobytes read and a couple of thousand multiply-accumulates, and the
/// embedding table has 128,256 of them — so a pass handed rayon a hundred
/// thousand tasks each too small to pay for being one, and the cost of handing
/// them out grew with the number of threads competing to take one.
///
/// That is what the knee was. Reading the same bytes with no arithmetic at all
/// does not knee: `examples/bandwidth` reaches 18.7 GiB/s on a *single* thread
/// and stays flat through twenty-four, while a forward pass peaked at 7 and got
/// worse as threads were added. The machine was never the limit; the
/// granularity was.
const ROWS_PER_TASK: usize = 64;

/// A tensor, as bytes inside the model's mapping plus what they mean.
///
/// Borrowed rather than owned, so a fleet of sessions over one model shares one
/// mapping and the weights are never copied — which is the property this whole
/// project measured before it had a model to apply it to.
pub struct Tensor<'a> {
    pub bytes: &'a [u8],
    pub quant: Quant,
    /// Length of one row: the fastest-varying dimension.
    pub row: usize,
    /// How many rows.
    pub rows: usize,
}

impl<'a> Tensor<'a> {
    /// Find `name` in `model`.
    pub fn find(model: &'a Gguf, name: &str) -> Result<Self, crate::gguf::Error> {
        let info: &TensorInfo = model.tensor(name)?;
        Ok(Self {
            bytes: model.bytes(info)?,
            quant: info.quant,
            row: info.row(),
            rows: info.rows(),
        })
    }

    /// Dequantise row `r` into `out`, which must be `self.row` long.
    pub fn row_into(&self, r: usize, out: &mut [f32]) {
        let start = self.quant.size_of(r * self.row);
        let len = self.quant.size_of(self.row);
        dequant(self.quant, &self.bytes[start..start + len], out);
    }

    /// `out[r] = row_r · x`, for every row.
    ///
    /// The rows are independent, which is the only parallelism a matrix-vector
    /// product has and all it needs: 24 threads against 1.3 GB per token is the
    /// difference between a second and a tenth of one.
    pub fn matvec(&self, x: &[f32], out: &mut [f32]) {
        debug_assert_eq!(x.len(), self.row);
        debug_assert_eq!(out.len(), self.rows);

        let row_bytes = self.quant.size_of(self.row);
        let quant = self.quant;
        out.par_chunks_mut(ROWS_PER_TASK)
            .enumerate()
            .for_each(|(chunk, slots)| {
                let first = chunk * ROWS_PER_TASK;
                for (offset, slot) in slots.iter_mut().enumerate() {
                    let r = first + offset;
                    *slot = dot(quant, &self.bytes[r * row_bytes..(r + 1) * row_bytes], x);
                }
            });
    }

    /// `out[r * lanes + b] = row_r · x[b]`, for every row and every lane.
    ///
    /// This is the reason batching is worth anything. A matrix-vector product
    /// reads 1.25 GiB of weights to produce one token for one agent; this reads
    /// each row *once* and uses it for every lane, so eight agents cost one
    /// pass over the weights rather than eight. The arithmetic is eight times
    /// as much and the memory traffic is unchanged, which is the right trade on
    /// any machine where the weights do not fit in cache — that is, on every
    /// machine.
    ///
    /// The output is row-major (`rows × lanes`) rather than lane-major, because
    /// that is what lets the rows be handed to separate threads as contiguous
    /// slices. [`to_lanes`] turns it back the other way round, which costs a
    /// transpose of `rows × lanes` floats against a read of the whole matrix.
    ///
    /// `x` is lane-major: lane `b`'s activations are `x[b * row..][..row]`.
    pub fn matmul(&self, x: &[f32], lanes: usize, out: &mut [f32]) {
        debug_assert_eq!(x.len(), self.row * lanes);
        debug_assert_eq!(out.len(), self.rows * lanes);
        if lanes == 1 {
            // The same work, and it keeps the one-agent path off the wider one
            // while that is still the path everything else uses.
            return self.matvec(x, out);
        }

        let row_bytes = self.quant.size_of(self.row);
        let quant = self.quant;
        let width = self.row;
        out.par_chunks_mut(lanes * ROWS_PER_TASK)
            .enumerate()
            .for_each(|(chunk, slots)| {
                let first = chunk * ROWS_PER_TASK;
                for (offset, cells) in slots.chunks_mut(lanes).enumerate() {
                    let r = first + offset;
                    let row = &self.bytes[r * row_bytes..(r + 1) * row_bytes];
                    for (b, cell) in cells.iter_mut().enumerate() {
                        *cell = dot(quant, row, &x[b * width..(b + 1) * width]);
                    }
                }
            });
    }
}

/// Whether this CPU has the instructions the wide dot product needs.
///
/// Decided once. `is_x86_feature_detected!` caches its answer, but this is
/// called once per row of a matrix with up to 128,256 rows, and a branch that
/// cheap is still worth not taking a hundred thousand times per token.
///
/// `HV2_INFER_SCALAR=1` forces the narrow path, which is how the two are
/// compared on the same machine at the same moment — the only comparison worth
/// making on a host whose load moves as much as this one's.
#[cfg(target_arch = "x86_64")]
static WIDE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    if std::env::var_os("HV2_INFER_SCALAR").is_some() {
        return false;
    }
    is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma")
});

/// Whether this CPU has the *wider* instructions, sixteen floats at a time.
///
/// `HV2_INFER_AVX2=1` forces the narrower path, which is how the two are
/// compared on one machine at one moment — the only comparison worth making
/// here.
#[cfg(target_arch = "x86_64")]
static WIDER: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    if std::env::var_os("HV2_INFER_SCALAR").is_some()
        || std::env::var_os("HV2_INFER_AVX2").is_some()
    {
        return false;
    }
    is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw")
});

/// Turn a `rows × lanes` result into `lanes × rows`.
///
/// Small: a transpose of the *output* of a matrix product, which is the model's
/// width rather than its parameters. Even for the output head — 128,256 rows by
/// eight lanes, four megabytes — it is a rounding error against the 268 MB of
/// weights the product just read.
pub fn to_lanes(rowmajor: &[f32], rows: usize, lanes: usize, out: &mut [f32]) {
    debug_assert_eq!(rowmajor.len(), rows * lanes);
    debug_assert_eq!(out.len(), rows * lanes);
    for r in 0..rows {
        for b in 0..lanes {
            out[b * rows + r] = rowmajor[r * lanes + b];
        }
    }
}

/// One row of any of the three kinds, dotted with `x`.
fn dot(quant: Quant, row: &[u8], x: &[f32]) -> f32 {
    match quant {
        Quant::Q8_0 => dot_q8_0(row, x),
        Quant::F32 => row
            .chunks_exact(4)
            .zip(x)
            .map(|(w, a)| f32::from_le_bytes([w[0], w[1], w[2], w[3]]) * a)
            .sum(),
        Quant::F16 => row
            .chunks_exact(2)
            .zip(x)
            .map(|(w, a)| f32::from(half::f16::from_le_bytes([w[0], w[1]])) * a)
            .sum(),
    }
}

/// One `Q8_0` row dotted with `x`.
///
/// Dispatched at runtime rather than at build time. Compiling the whole crate
/// with `-C target-cpu=native` is worth 1.29x on this host — 268 ms per forward
/// pass against 347, back to back at the same thread count — and produces a
/// binary that dies with an illegal instruction on an older machine. Detecting
/// once and calling the wide version keeps both.
fn dot_q8_0(row: &[u8], x: &[f32]) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if *WIDER {
        // SAFETY: `WIDER` is exactly the check that AVX-512F and BW are present.
        return unsafe { dot_q8_0_wider(row, x) };
    }
    #[cfg(target_arch = "x86_64")]
    if *WIDE {
        // SAFETY: `WIDE` is exactly the check that AVX2 and FMA are present.
        return unsafe { dot_q8_0_wide(row, x) };
    }
    dot_q8_0_scalar(row, x)
}

/// The same product, sixteen lanes at a time.
///
/// A `Q8_0` block is thirty-two weights, so it is exactly two of these
/// registers — which is the tidiest the arithmetic gets, and the reason this is
/// worth writing separately rather than letting the compiler widen the other
/// one.
///
/// What this deliberately is *not* is a VNNI kernel. `avx512_vnni` multiplies
/// and accumulates bytes in a single instruction and is the obvious shape for a
/// quantised product — but it wants both operands as integers, and the
/// activations here are floats. Using it means quantising the activation vector
/// too, which is a change to what the model computes and belongs behind its own
/// measurement of whether the answers survive it.
///
/// # Safety
///
/// The caller must have established that AVX-512F and AVX-512BW are available.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn dot_q8_0_wider(row: &[u8], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;

    let mut total = _mm512_setzero_ps();
    let blocks = row.len() / Q8_BYTES;

    for block in 0..blocks {
        let at = block * Q8_BYTES;
        let scale = f32::from(half::f16::from_le_bytes([row[at], row[at + 1]]));
        let quants = row.as_ptr().add(at + 2);
        let activations = x.as_ptr().add(block * Q8_BLOCK);

        // Two halves of sixteen.
        let mut sum = _mm512_setzero_ps();
        for half in 0..2 {
            let sixteen = _mm_loadu_si128(quants.add(half * 16) as *const __m128i);
            let widened = _mm512_cvtepi32_ps(_mm512_cvtepi8_epi32(sixteen));
            let a = _mm512_loadu_ps(activations.add(half * 16));
            sum = _mm512_fmadd_ps(widened, a, sum);
        }
        total = _mm512_fmadd_ps(sum, _mm512_set1_ps(scale), total);
    }

    _mm512_reduce_add_ps(total)
}

/// The same product, eight lanes at a time.
///
/// A block's thirty-two bytes are widened to floats in four groups of eight,
/// multiplied by their activations and summed into a vector accumulator; the
/// block's shared scale is applied once at the end of the block rather than per
/// weight, which is the whole reason `Q8_0` is cheap.
///
/// # Safety
///
/// The caller must have established that AVX2 and FMA are available.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_q8_0_wide(row: &[u8], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;

    let mut total = _mm256_setzero_ps();
    let blocks = row.len() / Q8_BYTES;

    for block in 0..blocks {
        let at = block * Q8_BYTES;
        let scale = f32::from(half::f16::from_le_bytes([row[at], row[at + 1]]));
        let quants = row.as_ptr().add(at + 2);
        let activations = x.as_ptr().add(block * Q8_BLOCK);

        // Four groups of eight, which is what one AVX2 register holds.
        let mut sum = _mm256_setzero_ps();
        for group in 0..4 {
            let eight = _mm_loadl_epi64(quants.add(group * 8) as *const __m128i);
            let widened = _mm256_cvtepi32_ps(_mm256_cvtepi8_epi32(eight));
            let a = _mm256_loadu_ps(activations.add(group * 8));
            sum = _mm256_fmadd_ps(widened, a, sum);
        }
        total = _mm256_fmadd_ps(sum, _mm256_set1_ps(scale), total);
    }

    // Horizontal sum: fold the eight lanes down to one.
    let high = _mm256_extractf128_ps(total, 1);
    let low = _mm256_castps256_ps128(total);
    let four = _mm_add_ps(low, high);
    let two = _mm_add_ps(four, _mm_movehl_ps(four, four));
    let one = _mm_add_ss(two, _mm_shuffle_ps(two, two, 0x55));
    _mm_cvtss_f32(one)
}

/// One `Q8_0` row dotted with `x`, one weight at a time.
///
/// A block is a 16-bit scale and thirty-two signed bytes. The scale comes out
/// of the inner loop — every weight in a block shares it — so the loop is a
/// multiply-accumulate over thirty-two bytes and one float multiply per block,
/// which is why `Q8_0` costs about what reading the bytes costs.
///
/// Kept as the definition of what the wide version must agree with, and used on
/// anything that is not an x86-64 with AVX2.
fn dot_q8_0_scalar(row: &[u8], x: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (block, chunk) in row.chunks_exact(Q8_BYTES).zip(x.chunks(Q8_BLOCK)) {
        let scale = f32::from(half::f16::from_le_bytes([block[0], block[1]]));
        let mut acc = 0.0f32;
        for (q, a) in block[2..].iter().zip(chunk) {
            acc += f32::from(*q as i8) * a;
        }
        sum += scale * acc;
    }
    sum
}

/// Dequantise `bytes` into `out`.
pub fn dequant(quant: Quant, bytes: &[u8], out: &mut [f32]) {
    match quant {
        Quant::F32 => {
            for (slot, w) in out.iter_mut().zip(bytes.chunks_exact(4)) {
                *slot = f32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            }
        }
        Quant::F16 => {
            for (slot, w) in out.iter_mut().zip(bytes.chunks_exact(2)) {
                *slot = f32::from(half::f16::from_le_bytes([w[0], w[1]]));
            }
        }
        Quant::Q8_0 => {
            for (block, slots) in bytes.chunks_exact(Q8_BYTES).zip(out.chunks_mut(Q8_BLOCK)) {
                let scale = f32::from(half::f16::from_le_bytes([block[0], block[1]]));
                for (slot, q) in slots.iter_mut().zip(&block[2..]) {
                    *slot = scale * f32::from(*q as i8);
                }
            }
        }
    }
}

/// Root-mean-square normalisation, scaled by a learned weight.
///
/// No mean subtraction and no bias — that is the whole difference from layer
/// normalisation, and it is why it is two lines.
pub fn rms_norm(x: &[f32], weight: &[f32], epsilon: f32, out: &mut [f32]) {
    let mean_square = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let scale = 1.0 / (mean_square + epsilon).sqrt();
    for ((slot, v), w) in out.iter_mut().zip(x).zip(weight) {
        *slot = v * scale * w;
    }
}

/// The sigmoid-weighted linear unit: `x * sigmoid(x)`.
pub fn silu(x: f32) -> f32 {
    x / (1.0 + (-x).exp())
}

/// Softmax in place, shifted by the maximum so that `exp` cannot overflow.
///
/// The shift is not defensive decoration. Attention logits over a long context
/// reach the tens, `exp(89)` is already infinity in `f32`, and a single
/// infinity turns the whole distribution into a NaN — which reads downstream as
/// a model that has simply stopped saying anything sensible.
pub fn softmax(values: &mut [f32]) {
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut total = 0.0;
    for v in values.iter_mut() {
        *v = (*v - max).exp();
        total += *v;
    }
    for v in values.iter_mut() {
        *v /= total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `Q8_0` row of `blocks` blocks, and the activations to dot it
    /// with, from a cheap deterministic sequence — a random-looking pattern
    /// that is the same on every machine and in every run.
    fn row_and_activations(blocks: usize) -> (Vec<u8>, Vec<f32>) {
        let mut row = Vec::with_capacity(blocks * Q8_BYTES);
        let mut x = Vec::with_capacity(blocks * Q8_BLOCK);
        let mut state = 0x2BAD_B002u32;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state
        };
        for block in 0..blocks {
            // A scale that varies per block, including a small one, so that a
            // version applying it in the wrong place is visible.
            let scale = half::f16::from_f32(0.001 + block as f32 * 0.017);
            row.extend_from_slice(&scale.to_le_bytes());
            for _ in 0..Q8_BLOCK {
                row.push((next() >> 24) as u8);
                x.push((next() >> 16) as i32 as f32 / 65_536.0 - 0.5);
            }
        }
        (row, x)
    }

    /// The wide version has to agree with the scalar one. It is the only test
    /// that matters for a hand-written SIMD kernel: everything else it could
    /// get wrong still produces a number.
    #[test]
    fn the_wide_dot_product_agrees_with_the_scalar_one() {
        for blocks in [1, 2, 7, 64] {
            let (row, x) = row_and_activations(blocks);
            let scalar = dot_q8_0_scalar(&row, &x);
            let dispatched = dot_q8_0(&row, &x);
            let slack = scalar.abs().max(1.0) * 1e-4;
            assert!(
                (scalar - dispatched).abs() <= slack,
                "{blocks} blocks: scalar {scalar}, dispatched {dispatched}"
            );
        }
    }

    /// Summing in a different order gives a different rounding, so the tolerance
    /// above is real rather than decoration — but it must not be so loose that
    /// a wrong answer fits inside it.
    #[test]
    fn the_tolerance_would_not_hide_a_wrong_answer() {
        let (row, x) = row_and_activations(64);
        let right = dot_q8_0_scalar(&row, &x);
        // Dropping one block is the smallest plausible mistake a blocked kernel
        // makes, and it has to be outside the tolerance the test above allows.
        let short = dot_q8_0_scalar(&row[..63 * Q8_BYTES], &x[..63 * Q8_BLOCK]);
        let slack = right.abs().max(1.0) * 1e-4;
        assert!(
            (right - short).abs() > slack,
            "a missing block should not pass as agreement"
        );
    }
}
