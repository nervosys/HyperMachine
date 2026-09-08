//! A transformer forward pass over real weights.
//!
//! Every figure this project has published about inference was a floor under
//! something that had never run: the memory a fleet of agents needs for a
//! model, the bandwidth a forward pass would have, the channel and the
//! permission a tool call travels over. There were no weights in the tree, no
//! tokeniser, and no forward pass, and the handoff said so on its own front
//! page. This is the thing those measurements were measurements *of*.
//!
//! # Where it runs, and why that is a decision
//!
//! In the host, as a capability the sandbox invokes — not inside the guest.
//! That is a choice with the numbers under it and it is worth stating the
//! reasoning, because the other answer is defensible and was nearly taken:
//!
//! - **A fleet cannot all infer at once.** Streaming a model's weights is
//!   about 1.4 cycles per byte warm, so a thousand agents inferring
//!   simultaneously would need hundreds of gigabytes per second of aggregate
//!   bandwidth, which no node has. Inference has to be *scheduled* across the
//!   fleet, and a scheduler that lives inside a thousand independent guests is
//!   not a scheduler.
//! - **The isolation it appears to buy is already spent.** Putting the forward
//!   pass in the guest would keep the prompt and the key/value cache inside the
//!   sandbox — but the weights are a host-owned read-only mapping either way,
//!   and the host is the thing that created the guest's memory. The sandbox
//!   boundary protects the host and an agent's peers *from* the agent; it has
//!   never protected the agent from the host, and pretending otherwise here
//!   would be the first dishonest claim in this repository.
//! - **The sandbox's argument is its size.** "A guest with no kernel has no
//!   kernel attack surface" is the security case for the whole design.
//!   A tokeniser, a matrix kernel and a sampler are the largest body of code in
//!   the system, and putting them inside the thing whose smallness is the claim
//!   would retract the claim.
//! - **The gate already exists.** A tool call is capability-gated, refused at
//!   the tool rather than at the broker, and answered under its own request id.
//!   Inference is a tool by every property that matters.
//!
//! What that costs, stated plainly: an agent's prompt and its key/value cache
//! live in host memory, so agents are separated from each other's inference by
//! the host's own bookkeeping rather than by hardware. [`Session`] is the unit
//! of that bookkeeping — one per agent, holding that agent's cache and nothing
//! else — and the capability graph decides which agents may ask at all.
//!
//! # What is here
//!
//! - [`gguf`] reads the model file, mapped rather than copied.
//! - [`tensor`] is the arithmetic: dequantise a row, dot it with a vector.
//! - [`tokenizer`] is byte-level BPE, built from the vocabulary in the file.
//! - [`model`] is the forward pass, and [`Session`] is one agent's conversation.
//!
//! # What is not
//!
//! Batching, speculative decoding, a KV cache that evicts, and every
//! quantisation but `Q8_0` and the two float kinds. Nothing here is a serving
//! stack; it is the smallest complete thing that turns weights into a token,
//! which is what the measurements were waiting for.
//!
//! ```no_run
//! use hv2_infer::{Model, Session, generate};
//!
//! let model = Model::load("model.gguf")?;
//! let mut session = Session::open(&model);
//! let answer = generate(&model, &mut session, "What is the capital of France?", 24)?;
//! # Ok::<(), hv2_infer::Error>(())
//! ```

pub mod gguf;
pub mod model;
pub mod tensor;
pub mod tokenizer;

pub use gguf::{Error, Gguf};
pub use model::{argmax, Model, Rope, Session, Shape};
pub use tokenizer::Tokenizer;

/// Wrap a question in the turn markers this model family was tuned with.
///
/// An instruction-tuned model given bare text continues it; given its own chat
/// markers, it answers. The markers are looked up by name, so a model that does
/// not have them falls back to the bare prompt rather than emitting the literal
/// text of a marker it has no token for.
pub fn chat_prompt(model: &Model, question: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    if let Some(bos) = model.bos {
        ids.push(bos);
    }

    let start = model.tokenizer.id_of("<|start_header_id|>");
    let end = model.tokenizer.id_of("<|end_header_id|>");
    let eot = model.tokenizer.id_of("<|eot_id|>");

    match (start, end, eot) {
        (Some(start), Some(end), Some(eot)) => {
            let mut turn = |role: &str, text: &str| {
                ids.push(start);
                ids.extend(model.tokenizer.encode(role));
                ids.push(end);
                ids.extend(model.tokenizer.encode("\n\n"));
                if !text.is_empty() {
                    ids.extend(model.tokenizer.encode(text));
                    ids.push(eot);
                }
            };
            turn("user", question);
            // The assistant's header with nothing after it: the model's turn
            // begins where the prompt ends, which is what makes it answer
            // rather than continue.
            turn("assistant", "");
        }
        _ => ids.extend(model.tokenizer.encode(question)),
    }
    ids
}

/// Answer `question`, greedily, for at most `limit` tokens.
///
/// Returns the text the model produced. Stops at a stop token, which is the
/// difference between an answer and a model that keeps going until the limit.
pub fn generate(
    model: &Model,
    session: &mut Session<'_>,
    question: &str,
    limit: usize,
) -> Result<String, Error> {
    let prompt = chat_prompt(model, question);
    let mut position = 0;
    let mut logits = Vec::new();

    for token in &prompt {
        logits = session.forward(*token, position)?;
        position += 1;
    }

    let mut produced = Vec::new();
    for _ in 0..limit {
        let next = argmax(&logits);
        if model.stops.contains(&next) {
            break;
        }
        produced.push(next);
        logits = session.forward(next, position)?;
        position += 1;
    }

    Ok(model.tokenizer.decode(&produced))
}
