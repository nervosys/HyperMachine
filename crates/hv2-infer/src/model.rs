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
use crate::tensor::{dequant, rms_norm, silu, softmax, to_lanes, Tensor};
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

    /// Bytes of tensor data, as stored.
    ///
    /// Not the file size: the header carries a 128,256-entry vocabulary and
    /// 280,147 merge rules, which are several megabytes that no forward pass
    /// ever reads. A rate computed against the file would flatter itself.
    pub fn weight_bytes(&self) -> usize {
        self.gguf
            .tensors
            .values()
            .map(|t| t.quant.size_of(t.elements()))
            .sum()
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
            let mut runner = Runner::with_rope(self, 1, rope);
            let mut session = Session::new(self);
            let mut logits = Vec::new();
            for (position, token) in probe.iter().enumerate() {
                logits = runner.forward(&mut session, *token, position)?.to_vec();
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

/// One agent's conversation: its key/value cache, and nothing else.
///
/// Deliberately nothing else. This used to carry the scratch a forward pass
/// needs too — the logits alone are 128,256 floats, half a megabyte — which
/// made a session cost about 800 KiB before it held a single token of context.
/// A thousand idle agents were paying 800 MiB for buffers only the one being
/// served was using. The scratch belongs to whoever is running a pass, which is
/// [`Runner`], and there are as many of those as there are workers rather than
/// as there are agents.
///
/// It also has no lifetime any more, which is what lets a scheduler keep a map
/// of them without threading the model's borrow through everything that touches
/// it.
pub struct Session {
    /// `[layer][position * kv_width + i]`.
    pub(crate) keys: Vec<Vec<f32>>,
    pub(crate) values: Vec<Vec<f32>>,
    /// How many positions the cache holds.
    pub(crate) filled: usize,
    /// Remembered rather than asked of the model, so this type needs no
    /// reference to one.
    bytes_per_token: usize,
}

impl Session {
    /// An empty conversation for `model`.
    pub fn new(model: &Model) -> Self {
        Self {
            keys: vec![Vec::new(); model.shape.layers],
            values: vec![Vec::new(); model.shape.layers],
            filled: 0,
            bytes_per_token: model.cache_bytes_per_token(),
        }
    }

    /// The same. Kept because every caller already spells it this way.
    pub fn open(model: &Model) -> Self {
        Self::new(model)
    }

    /// How many positions this conversation holds.
    ///
    /// Also the position the next token goes at, which is what makes a second
    /// turn a continuation rather than a new conversation.
    pub fn len(&self) -> usize {
        self.filled
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Forget everything, keeping the allocations.
    pub fn forget(&mut self) {
        for cache in self.keys.iter_mut().chain(self.values.iter_mut()) {
            cache.clear();
        }
        self.filled = 0;
    }

    /// Bytes of key/value cache this conversation is currently holding.
    pub fn cache_bytes(&self) -> usize {
        self.filled * self.bytes_per_token
    }
}

/// What runs a forward pass: the weights it needs to touch, and the scratch.
///
/// One per worker, not one per agent. A `Runner` can advance several
/// conversations by one token each in a single pass over the weights, which is
/// the only way a fleet gets cheap: the pass reads 1.25 GiB to produce a token,
/// and whether that produces one token or eight is decided here.
pub struct Runner<'a> {
    model: &'a Model,
    blocks: Vec<Block<'a>>,
    output_norm: Vec<f32>,
    embeddings: Tensor<'a>,
    rope: Rope,
    /// The most conversations one pass may carry.
    lanes: usize,

    // Scratch, lane-major: lane `b`'s vector starts at `b * width`.
    x: Vec<f32>,
    normed: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    attended: Vec<f32>,
    gate: Vec<f32>,
    up: Vec<f32>,
    projected: Vec<f32>,
    logits: Vec<f32>,
    /// Row-major staging for a batched product, before it is turned lane-major.
    wide: Vec<f32>,
    /// Attention weights for the lane being attended, which is one at a time:
    /// every lane is at a different position in a different conversation, so
    /// there is nothing to share.
    scores: Vec<f32>,
}

impl<'a> Runner<'a> {
    /// A runner over `model` that can carry `lanes` conversations at once.
    pub fn new(model: &'a Model, lanes: usize) -> Self {
        let lanes = lanes.max(1);
        let s = &model.shape;
        let kv_width = s.kv_heads * s.head_dim;
        let blocks = (0..s.layers)
            .map(|l| model.block(l).expect("every block was found at load"))
            .collect();
        let mut output_norm = vec![0.0; s.width];
        let t = Tensor::find(&model.gguf, "output_norm.weight").expect("an output norm");
        dequant(t.quant, t.bytes, &mut output_norm);
        let embeddings =
            Tensor::find(&model.gguf, "token_embd.weight").expect("an embedding table");

        // The widest product is the output head, so the staging buffer is sized
        // for that and every other product fits inside it.
        let widest = s.vocab.max(s.ffn).max(s.width);

        Self {
            model,
            blocks,
            output_norm,
            embeddings,
            rope: model.rope,
            lanes,
            x: vec![0.0; lanes * s.width],
            normed: vec![0.0; lanes * s.width],
            q: vec![0.0; lanes * s.heads * s.head_dim],
            k: vec![0.0; lanes * kv_width],
            v: vec![0.0; lanes * kv_width],
            attended: vec![0.0; lanes * s.heads * s.head_dim],
            gate: vec![0.0; lanes * s.ffn],
            up: vec![0.0; lanes * s.ffn],
            projected: vec![0.0; lanes * s.width],
            logits: vec![0.0; lanes * s.vocab],
            wide: vec![0.0; lanes * widest],
            scores: Vec::new(),
        }
    }

    /// A runner for one conversation.
    pub fn single(model: &'a Model) -> Self {
        Self::new(model, 1)
    }

    /// A runner using a rotation the model has not settled on yet.
    ///
    /// Only [`Model::detect_rope`] wants this: it decides the convention by
    /// running the model under both, which it cannot do through the field it is
    /// in the middle of deciding.
    pub(crate) fn with_rope(model: &'a Model, lanes: usize, rope: Rope) -> Self {
        let mut runner = Self::new(model, lanes);
        runner.rope = rope;
        runner
    }

    /// The model it runs.
    pub fn model(&self) -> &'a Model {
        self.model
    }

    /// The most conversations it can carry at once.
    pub fn lanes(&self) -> usize {
        self.lanes
    }

    /// Bytes of scratch this runner holds.
    ///
    /// Worth knowing because it is the cost that used to be per agent and is
    /// now per worker: at eight lanes it is a few megabytes once, rather than
    /// 800 KiB times however many agents exist.
    pub fn scratch_bytes(&self) -> usize {
        (self.x.len()
            + self.normed.len()
            + self.q.len()
            + self.k.len()
            + self.v.len()
            + self.attended.len()
            + self.gate.len()
            + self.up.len()
            + self.projected.len()
            + self.logits.len()
            + self.wide.len())
            * core::mem::size_of::<f32>()
    }

    /// Advance every conversation in `work` by one token.
    ///
    /// Each entry is a conversation, the token to feed it, and the position to
    /// feed it at — positions differ between lanes because the conversations
    /// are different lengths. Afterwards [`Runner::logits`] gives each lane's
    /// distribution over the vocabulary.
    ///
    /// The weights are read once for the whole batch. Everything that is *not*
    /// a weight — the norms, the rotation, the attention over each
    /// conversation's own cache — is done per lane, because none of it is
    /// shared and none of it is the expensive part.
    pub fn step(&mut self, work: &mut [(&mut Session, u32, usize)]) -> Result<(), Error> {
        let lanes = work.len();
        assert!(
            lanes <= self.lanes,
            "a runner with {} lanes was given {lanes} conversations",
            self.lanes
        );
        if lanes == 0 {
            return Ok(());
        }

        let s = self.model.shape.clone();
        let kv_width = s.kv_heads * s.head_dim;
        let group = s.heads / s.kv_heads;
        let scale = 1.0 / (s.head_dim as f32).sqrt();

        // A token is a row index into the embedding table, not a product.
        for (b, (_, token, _)) in work.iter().enumerate() {
            self.embeddings
                .row_into(*token as usize, &mut self.x[b * s.width..(b + 1) * s.width]);
        }

        for layer in 0..s.layers {
            let block = &self.blocks[layer];

            for b in 0..lanes {
                let at = b * s.width;
                rms_norm(
                    &self.x[at..at + s.width],
                    &block.attn_norm,
                    s.rms_epsilon,
                    &mut self.normed[at..at + s.width],
                );
            }

            product(&block.q, &self.normed, lanes, &mut self.wide, &mut self.q);
            product(&block.k, &self.normed, lanes, &mut self.wide, &mut self.k);
            product(&block.v, &self.normed, lanes, &mut self.wide, &mut self.v);

            for (b, (_, _, position)) in work.iter().enumerate() {
                let q_at = b * s.heads * s.head_dim;
                for head in 0..s.heads {
                    rotate(
                        &mut self.q[q_at + head * s.head_dim..q_at + (head + 1) * s.head_dim],
                        *position,
                        s.rope_base,
                        self.rope,
                    );
                }
                let kv_at = b * kv_width;
                for head in 0..s.kv_heads {
                    rotate(
                        &mut self.k[kv_at + head * s.head_dim..kv_at + (head + 1) * s.head_dim],
                        *position,
                        s.rope_base,
                        self.rope,
                    );
                }
            }

            // Attention, per lane, over that lane's own cache. Nothing here is
            // shared between lanes and nothing here reads a weight.
            for (b, (session, _, position)) in work.iter_mut().enumerate() {
                let keys = &mut session.keys[layer];
                let values = &mut session.values[layer];
                keys.truncate(*position * kv_width);
                values.truncate(*position * kv_width);
                keys.extend_from_slice(&self.k[b * kv_width..(b + 1) * kv_width]);
                values.extend_from_slice(&self.v[b * kv_width..(b + 1) * kv_width]);

                let seen = *position + 1;
                self.scores.resize(seen, 0.0);
                let q_at = b * s.heads * s.head_dim;

                for head in 0..s.heads {
                    let kv_head = head / group;
                    let q = &self.q[q_at + head * s.head_dim..q_at + (head + 1) * s.head_dim];

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

                    let out = &mut self.attended
                        [q_at + head * s.head_dim..q_at + (head + 1) * s.head_dim];
                    out.fill(0.0);
                    for (p, weight) in self.scores.iter().enumerate() {
                        let at = p * kv_width + kv_head * s.head_dim;
                        for (slot, value) in out.iter_mut().zip(&values[at..at + s.head_dim]) {
                            *slot += weight * value;
                        }
                    }
                }
            }

            product(
                &block.o,
                &self.attended,
                lanes,
                &mut self.wide,
                &mut self.projected,
            );
            for (x, delta) in self.x[..lanes * s.width]
                .iter_mut()
                .zip(&self.projected[..lanes * s.width])
            {
                *x += delta;
            }

            for b in 0..lanes {
                let at = b * s.width;
                rms_norm(
                    &self.x[at..at + s.width],
                    &block.ffn_norm,
                    s.rms_epsilon,
                    &mut self.normed[at..at + s.width],
                );
            }
            product(
                &block.gate,
                &self.normed,
                lanes,
                &mut self.wide,
                &mut self.gate,
            );
            product(&block.up, &self.normed, lanes, &mut self.wide, &mut self.up);
            for (g, u) in self.gate[..lanes * s.ffn]
                .iter_mut()
                .zip(&self.up[..lanes * s.ffn])
            {
                *g = silu(*g) * u;
            }
            product(
                &block.down,
                &self.gate,
                lanes,
                &mut self.wide,
                &mut self.projected,
            );
            for (x, delta) in self.x[..lanes * s.width]
                .iter_mut()
                .zip(&self.projected[..lanes * s.width])
            {
                *x += delta;
            }
        }

        for b in 0..lanes {
            let at = b * s.width;
            rms_norm(
                &self.x[at..at + s.width],
                &self.output_norm,
                s.rms_epsilon,
                &mut self.normed[at..at + s.width],
            );
        }
        // The embedding table again, transposed — this model ties its input and
        // output vocabularies, which is why the file has no separate head.
        product(
            &self.embeddings,
            &self.normed,
            lanes,
            &mut self.wide,
            &mut self.logits,
        );

        for (session, _, position) in work.iter_mut() {
            session.filled = *position + 1;
        }
        Ok(())
    }

    /// The distribution over the vocabulary for one lane of the last step.
    pub fn logits(&self, lane: usize) -> &[f32] {
        let vocab = self.model.shape.vocab;
        &self.logits[lane * vocab..(lane + 1) * vocab]
    }

    /// Advance one conversation by one token, and give back its logits.
    ///
    /// The single-lane spelling, kept because most callers have one
    /// conversation and should not have to build a slice of one to say so.
    pub fn forward(
        &mut self,
        session: &mut Session,
        token: u32,
        position: usize,
    ) -> Result<&[f32], Error> {
        self.step(&mut [(session, token, position)])?;
        Ok(self.logits(0))
    }
}

/// One batched product, staged row-major and handed back lane-major.
///
/// A free function rather than a method so that the tensor, the input and the
/// two buffers can be four disjoint borrows of the same runner.
fn product(t: &Tensor<'_>, x: &[f32], lanes: usize, wide: &mut [f32], out: &mut [f32]) {
    let cells = t.rows * lanes;
    t.matmul(&x[..t.row * lanes], lanes, &mut wide[..cells]);
    to_lanes(&wide[..cells], t.rows, lanes, &mut out[..cells]);
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
