//! The forward pass.
//!
//! A Llama-shaped decoder: for each block, normalise, project to queries, keys
//! and values, rotate the first two by position, attend causally over
//! everything seen so far, project back and add; then normalise again and run a
//! gated feed-forward, and add. At the end, normalise once more and project to
//! the vocabulary.
//!
//! Nothing here is novel and that is the point. What was missing from this
//! repository was not an idea, it was any code at all that turns weights into a
//! token: every figure the project has published about inference — the memory a
//! fleet needs, the bandwidth a forward pass has, the channel a tool call
//! travels — was the floor under something that had never run.
//!
//! # Two conventions this had to get right, and one it cannot check
//!
//! **Grouped-query attention.** There are 32 query heads and 8 key/value heads,
//! so four query heads share each key/value head. Reading that the wrong way
//! round produces a model that runs at full speed and is subtly wrong, because
//! every shape still matches.
//!
//! **The rotation.** Position is encoded by rotating pairs of components of the
//! query and key vectors. Which components pair up is a convention, and the two
//! in use disagree: the reference implementation pairs `i` with `i + head/2`,
//! and the format this file reads pairs `2i` with `2i + 1`, with the weights
//! permuted at conversion time so that the two agree. A file converted by one
//! convention and read by the other yields fluent-looking nonsense — so
//! [`Rope`] names both and [`Model::detect_rope`] picks by measurement rather
//! than by assumption.
//!
//! **What it cannot check** is the long-context frequency scaling this model
//! family applies above a few thousand tokens. The parameters for it are not in
//! the file, so they are not applied, and nothing here goes near a context long
//! enough for the difference to appear. Said plainly rather than left as a
//! silent divergence.

use crate::gguf::{Error, Gguf, Value};
use crate::tensor::{dequant, rms_norm, silu, softmax, Tensor};
use crate::tokenizer::Tokenizer;

/// Which components of a head pair up under rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rope {
    /// `2i` with `2i + 1`. What `llama.cpp` and the GGUF ecosystem use.
    Interleaved,
    /// `i` with `i + head_dim / 2`. What the reference implementation uses.
    HalfSplit,
}

impl Rope {
    /// The name a report would print.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interleaved => "interleaved",
            Self::HalfSplit => "half-split",
        }
    }
}

/// The shape of the network, all of it read from the file.
#[derive(Debug, Clone)]
pub struct Shape {
    pub layers: usize,
    pub width: usize,
    pub heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    pub ffn: usize,
    pub vocab: usize,
    pub rms_epsilon: f32,
    pub rope_base: f32,
    pub context: usize,
}

/// One block's weights.
struct Block<'a> {
    attn_norm: Vec<f32>,
    ffn_norm: Vec<f32>,
    q: Tensor<'a>,
    k: Tensor<'a>,
    v: Tensor<'a>,
    o: Tensor<'a>,
    gate: Tensor<'a>,
    up: Tensor<'a>,
    down: Tensor<'a>,
}

/// A loaded model: the mapping, the shape, the weights and the tokeniser.
pub struct Model {
    gguf: Gguf,
    pub shape: Shape,
    pub tokenizer: Tokenizer,
    pub rope: Rope,
    /// The token that starts a sequence, if the vocabulary names one.
    pub bos: Option<u32>,
    /// Tokens that end a turn. More than one, because an instruction-tuned
    /// model may stop with any of them and treating only the first as an
    /// ending produces a model that never stops talking.
    pub stops: Vec<u32>,
    /// How long deciding the rotation took.
    ///
    /// Reported separately because it is most of what loading costs and it is
    /// not loading: it is a dozen forward passes. A single "model loaded in N
    /// seconds" would describe reading a file as being ten times slower than it
    /// is, which is the kind of number this project keeps having to take apart
    /// afterwards.
    pub detection: std::time::Duration,
}

impl Model {
    /// Map `path` and read everything the forward pass needs from it.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let gguf = Gguf::open(path)?;

        let architecture = gguf
            .get("general.architecture")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if architecture != "llama" {
            return Err(Error::Missing(format!(
                "an architecture this implements — the file says {architecture:?}, and only \
                 \"llama\" is written here"
            )));
        }

        let width = gguf.count("llama.embedding_length")?;
        let heads = gguf.count("llama.attention.head_count")?;
        let shape = Shape {
            layers: gguf.count("llama.block_count")?,
            width,
            heads,
            kv_heads: gguf.count("llama.attention.head_count_kv")?,
            // Not in the file, and derived rather than assumed to be 128: a
            // model whose heads are not the width divided by their number is a
            // model this would silently mis-slice.
            head_dim: width / heads,
            ffn: gguf.count("llama.feed_forward_length")?,
            vocab: gguf.count("llama.vocab_size")?,
            rms_epsilon: gguf.real("llama.attention.layer_norm_rms_epsilon")?,
            rope_base: gguf.real("llama.rope.freq_base")?,
            context: gguf.count("llama.context_length")?,
        };

        let tokens = gguf
            .get("tokenizer.ggml.tokens")
            .and_then(Value::as_strings)
            .ok_or_else(|| Error::Missing("tokenizer.ggml.tokens".into()))?
            .to_vec();
        let merges = gguf
            .get("tokenizer.ggml.merges")
            .and_then(Value::as_strings)
            .ok_or_else(|| Error::Missing("tokenizer.ggml.merges".into()))?
            .to_vec();
        let tokenizer = Tokenizer::new(&tokens, &merges);

        // By name, not by the id in the metadata. This file says its beginning
        // marker is token 1; token 1 is `"`.
        let bos = tokenizer.id_of("<|begin_of_text|>");
        let stops = ["<|eot_id|>", "<|end_of_text|>", "<|eom_id|>"]
            .iter()
            .filter_map(|name| tokenizer.id_of(name))
            .collect();

        let mut model = Self {
            gguf,
            shape,
            tokenizer,
            rope: Rope::Interleaved,
            bos,
            stops,
            detection: std::time::Duration::ZERO,
        };
        let started = std::time::Instant::now();
        model.rope = model.detect_rope()?;
        model.detection = started.elapsed();
        Ok(model)
    }

    /// How many bytes of weights are mapped.
    pub fn mapped_bytes(&self) -> usize {
        self.gguf.mapped_bytes()
    }

    fn block(&self, layer: usize) -> Result<Block<'_>, Error> {
        let norm = |name: &str| -> Result<Vec<f32>, Error> {
            let t = Tensor::find(&self.gguf, name)?;
            let mut out = vec![0.0; t.row * t.rows];
            dequant(t.quant, t.bytes, &mut out);
            Ok(out)
        };
        Ok(Block {
            attn_norm: norm(&format!("blk.{layer}.attn_norm.weight"))?,
            ffn_norm: norm(&format!("blk.{layer}.ffn_norm.weight"))?,
            q: Tensor::find(&self.gguf, &format!("blk.{layer}.attn_q.weight"))?,
            k: Tensor::find(&self.gguf, &format!("blk.{layer}.attn_k.weight"))?,
            v: Tensor::find(&self.gguf, &format!("blk.{layer}.attn_v.weight"))?,
            o: Tensor::find(&self.gguf, &format!("blk.{layer}.attn_output.weight"))?,
            gate: Tensor::find(&self.gguf, &format!("blk.{layer}.ffn_gate.weight"))?,
            up: Tensor::find(&self.gguf, &format!("blk.{layer}.ffn_up.weight"))?,
            down: Tensor::find(&self.gguf, &format!("blk.{layer}.ffn_down.weight"))?,
        })
    }

    /// Decide which rotation convention this file was written with.
    ///
    /// By running the model, not by reading the metadata, because the metadata
    /// does not say. Two forward passes over the same three tokens, one under
    /// each convention; the one whose next-token distribution is sharper is the
    /// one the weights were permuted for.
    ///
    /// That works because the wrong convention does not break the model, it
    /// blurs it: every query and key is rotated by an angle belonging to a
    /// different component, attention loses its position information, and the
    /// output distribution flattens. Comparing the largest probability is a
    /// cheap and decisive way to see that — a correct pass on an ordinary
    /// English prefix is confident, and a scrambled one is not.
    fn detect_rope(&self) -> Result<Rope, Error> {
        // A prefix with an overwhelming continuation, so that "confident" is a
        // property of the model working rather than of the sentence.
        let probe = self.tokenizer.encode("The capital of France is");
        let mut best = (Rope::Interleaved, f32::NEG_INFINITY);
        for rope in [Rope::Interleaved, Rope::HalfSplit] {
            let mut session = Session::new(self, rope);
            let mut logits = Vec::new();
            for (position, token) in probe.iter().enumerate() {
                logits = session.forward(*token, position)?;
            }
            softmax(&mut logits);
            let peak = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            if peak > best.1 {
                best = (rope, peak);
            }
        }
        Ok(best.0)
    }

    /// Bytes of key/value cache one token costs.
    ///
    /// The number that decides fleet density, since it is the one thing an
    /// agent cannot share with another agent: it is that agent's conversation.
    pub fn cache_bytes_per_token(&self) -> usize {
        self.shape.layers * self.shape.kv_heads * self.shape.head_dim * 2 * 4
    }
}

/// One conversation: the key/value cache, and the scratch a pass needs.
///
/// Per agent, by definition. The weights are shared and this is not — which is
/// the whole shape of the arithmetic this project measured before it had a
/// model: the model once, plus per agent exactly what that agent holds.
pub struct Session<'a> {
    model: &'a Model,
    blocks: Vec<Block<'a>>,
    output_norm: Vec<f32>,
    embeddings: Tensor<'a>,
    rope: Rope,

    /// `[layer][position * kv_width + i]`.
    keys: Vec<Vec<f32>>,
    values: Vec<Vec<f32>>,
    /// How many positions the cache holds.
    filled: usize,

    // Scratch, allocated once. A forward pass that allocates per token spends
    // its time in the allocator rather than in the arithmetic.
    x: Vec<f32>,
    normed: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    attended: Vec<f32>,
    scores: Vec<f32>,
    gate: Vec<f32>,
    up: Vec<f32>,
    projected: Vec<f32>,
    logits: Vec<f32>,
}

impl<'a> Session<'a> {
    /// Open a conversation over `model`.
    pub fn new(model: &'a Model, rope: Rope) -> Self {
        let s = &model.shape;
        let kv_width = s.kv_heads * s.head_dim;
        let blocks = (0..s.layers)
            .map(|l| model.block(l).expect("every block was found at load"))
            .collect();
        let mut output_norm = vec![0.0; s.width];
        let t = Tensor::find(&model.gguf, "output_norm.weight").expect("an output norm");
        dequant(t.quant, t.bytes, &mut output_norm);

        Self {
            model,
            blocks,
            output_norm,
            embeddings: Tensor::find(&model.gguf, "token_embd.weight").expect("an embedding table"),
            rope,
            keys: vec![Vec::new(); s.layers],
            values: vec![Vec::new(); s.layers],
            filled: 0,
            x: vec![0.0; s.width],
            normed: vec![0.0; s.width],
            q: vec![0.0; s.heads * s.head_dim],
            k: vec![0.0; kv_width],
            v: vec![0.0; kv_width],
            attended: vec![0.0; s.heads * s.head_dim],
            scores: Vec::new(),
            gate: vec![0.0; s.ffn],
            up: vec![0.0; s.ffn],
            projected: vec![0.0; s.width],
            logits: vec![0.0; s.vocab],
        }
    }

    /// Open a conversation using whichever rotation the model was detected with.
    pub fn open(model: &'a Model) -> Self {
        Self::new(model, model.rope)
    }

    /// How many positions this conversation holds.
    pub fn len(&self) -> usize {
        self.filled
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Bytes of key/value cache this conversation is currently holding.
    pub fn cache_bytes(&self) -> usize {
        self.filled * self.model.cache_bytes_per_token()
    }

    /// Run one token at `position` and return the logits over the vocabulary.
    ///
    /// The logits are borrowed from the session's own scratch, so the caller
    /// gets them without an allocation of 128,256 floats per token.
    pub fn forward(&mut self, token: u32, position: usize) -> Result<Vec<f32>, Error> {
        let s = self.model.shape.clone();
        let kv_width = s.kv_heads * s.head_dim;

        // The embedding table is the model's largest tensor and is read one row
        // at a time: a token is a row index, not a matrix product.
        self.embeddings.row_into(token as usize, &mut self.x);

        for layer in 0..s.layers {
            let block = &self.blocks[layer];

            rms_norm(&self.x, &block.attn_norm, s.rms_epsilon, &mut self.normed);
            block.q.matvec(&self.normed, &mut self.q);
            block.k.matvec(&self.normed, &mut self.k);
            block.v.matvec(&self.normed, &mut self.v);

            for head in 0..s.heads {
                rotate(
                    &mut self.q[head * s.head_dim..(head + 1) * s.head_dim],
                    position,
                    s.rope_base,
                    self.rope,
                );
            }
            for head in 0..s.kv_heads {
                rotate(
                    &mut self.k[head * s.head_dim..(head + 1) * s.head_dim],
                    position,
                    s.rope_base,
                    self.rope,
                );
            }

            // Everything before this position is already in the cache; this
            // position joins it. Growing rather than indexing, so a conversation
            // costs what it holds rather than what it might hold.
            let keys = &mut self.keys[layer];
            let values = &mut self.values[layer];
            keys.truncate(position * kv_width);
            values.truncate(position * kv_width);
            keys.extend_from_slice(&self.k);
            values.extend_from_slice(&self.v);

            let seen = position + 1;
            self.scores.resize(seen, 0.0);
            let scale = 1.0 / (s.head_dim as f32).sqrt();
            // Four query heads to each key/value head. The other way round is a
            // model that runs and is wrong.
            let group = s.heads / s.kv_heads;

            for head in 0..s.heads {
                let kv_head = head / group;
                let q = &self.q[head * s.head_dim..(head + 1) * s.head_dim];

                for (p, score) in self.scores.iter_mut().enumerate() {
                    let at = p * kv_width + kv_head * s.head_dim;
                    *score = q
                        .iter()
                        .zip(&keys[at..at + s.head_dim])
                        .map(|(a, b)| a * b)
                        .sum::<f32>()
                        * scale;
                }
                softmax(&mut self.scores);

                let out = &mut self.attended[head * s.head_dim..(head + 1) * s.head_dim];
                out.fill(0.0);
                for (p, weight) in self.scores.iter().enumerate() {
                    let at = p * kv_width + kv_head * s.head_dim;
                    for (slot, value) in out.iter_mut().zip(&values[at..at + s.head_dim]) {
                        *slot += weight * value;
                    }
                }
            }

            block.o.matvec(&self.attended, &mut self.projected);
            for (x, delta) in self.x.iter_mut().zip(&self.projected) {
                *x += delta;
            }

            rms_norm(&self.x, &block.ffn_norm, s.rms_epsilon, &mut self.normed);
            block.gate.matvec(&self.normed, &mut self.gate);
            block.up.matvec(&self.normed, &mut self.up);
            for (g, u) in self.gate.iter_mut().zip(&self.up) {
                *g = silu(*g) * u;
            }
            block.down.matvec(&self.gate, &mut self.projected);
            for (x, delta) in self.x.iter_mut().zip(&self.projected) {
                *x += delta;
            }
        }

        rms_norm(&self.x, &self.output_norm, s.rms_epsilon, &mut self.normed);
        // The embedding table again, transposed — this model ties its input and
        // output vocabularies, which is why the file has no separate head.
        self.embeddings.matvec(&self.normed, &mut self.logits);

        self.filled = position + 1;
        Ok(self.logits.clone())
    }
}

/// Rotate one head's vector by its position.
///
/// Each pair of components is turned through an angle that grows with position
/// and shrinks with the pair's index, so the dot product of two rotated vectors
/// depends on the distance between their positions and not on where either one
/// is. That is the whole mechanism: a model with no positional embedding at all
/// still knows how far apart two tokens are.
fn rotate(head: &mut [f32], position: usize, base: f32, style: Rope) {
    let dim = head.len();
    let half = dim / 2;
    for i in 0..half {
        let (a, b) = match style {
            Rope::Interleaved => (2 * i, 2 * i + 1),
            Rope::HalfSplit => (i, i + half),
        };
        let frequency = base.powf(-2.0 * i as f32 / dim as f32);
        let angle = position as f32 * frequency;
        let (sin, cos) = angle.sin_cos();
        let (x, y) = (head[a], head[b]);
        head[a] = x * cos - y * sin;
        head[b] = y * cos + x * sin;
    }
}

/// The index of the largest value.
///
/// Greedy decoding, and deliberately not sampling. A demonstration whose output
/// changes run to run cannot be asserted on, and the question here is whether
/// the arithmetic is right rather than whether the prose is interesting.
pub fn argmax(values: &[f32]) -> u32 {
    let mut best = 0usize;
    for (i, v) in values.iter().enumerate() {
        if *v > values[best] {
            best = i;
        }
    }
    best as u32
}
