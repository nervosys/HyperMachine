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
        match self.quant {
            Quant::Q8_0 => out.par_iter_mut().enumerate().for_each(|(r, slot)| {
                *slot = dot_q8_0(&self.bytes[r * row_bytes..(r + 1) * row_bytes], x);
            }),
            Quant::F32 => out.par_iter_mut().enumerate().for_each(|(r, slot)| {
                let row = &self.bytes[r * row_bytes..(r + 1) * row_bytes];
                *slot = row
                    .chunks_exact(4)
                    .zip(x)
                    .map(|(w, a)| f32::from_le_bytes([w[0], w[1], w[2], w[3]]) * a)
                    .sum();
            }),
            Quant::F16 => out.par_iter_mut().enumerate().for_each(|(r, slot)| {
                let row = &self.bytes[r * row_bytes..(r + 1) * row_bytes];
                *slot = row
                    .chunks_exact(2)
                    .zip(x)
                    .map(|(w, a)| f32::from(half::f16::from_le_bytes([w[0], w[1]])) * a)
                    .sum();
            }),
        }
    }
}

/// One `Q8_0` row dotted with `x`.
///
/// A block is a 16-bit scale and thirty-two signed bytes. The scale comes out
/// of the inner loop — every weight in a block shares it — so the loop is an
/// integer multiply-accumulate over thirty-two bytes and one float multiply per
/// block, which is why `Q8_0` costs about what reading the bytes costs.
fn dot_q8_0(row: &[u8], x: &[f32]) -> f32 {
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
