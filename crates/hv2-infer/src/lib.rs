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
//! - [`schedule`] is who gets it and when — a queue, a bound on how many passes
//!   run at once, and a bound on how much context one agent may hold.
//!
//! # What is not
//!
//! Batching several agents into one pass, speculative decoding, a cache that
//! evicts, and every quantisation but `Q8_0` and the two float kinds. Nothing
//! here is a serving stack; it is the smallest complete thing that turns
//! weights into a token and then decides who may.
//!
//! ```no_run
//! use hv2_infer::{ask, Model, Runner, Session};
//!
//! let model = Model::load("model.gguf")?;
//! // The runner holds the scratch a pass needs and can carry several
//! // conversations at once; the session holds one conversation.
//! let mut runner = Runner::new(&model, 1);
//! let mut session = Session::open(&model);
//! let answer = ask(&mut runner, &mut session, "What is the capital of France?", 24)?;
//! # Ok::<(), hv2_infer::Error>(())
//! ```

pub mod gguf;
pub mod model;
pub mod schedule;
pub mod tensor;
pub mod tokenizer;

pub use gguf::{Error, Gguf};
pub use model::{argmax, Model, Rope, Runner, Session, Shape};
pub use schedule::{Limits, Refused, Scheduler, Served, Stats};
pub use tokenizer::Tokenizer;

/// The tokens for one user turn, and the header that invites an answer.
///
/// `opening` adds the beginning-of-text marker, which belongs once at the start
/// of a conversation and nowhere else. A second turn that repeated it would be
/// telling the model the conversation had started twice.
///
/// The markers are looked up by name, so a model that does not have them falls
/// back to the bare question rather than emitting the literal text of a marker
/// it has no token for.
pub fn chat_turn(model: &Model, question: &str, opening: bool) -> Vec<u32> {
    /// What follows a role header before the turn's text. Named because it is
    /// part of the template the model was tuned with, not whitespace anyone is
    /// free to reformat.
    const NEWLINES: &str = "\n\n";

    let mut ids = Vec::new();
    if opening {
        if let Some(bos) = model.bos {
            ids.push(bos);
        }
    }

    let start = model.tokenizer.id_of("<|start_header_id|>");
    let end = model.tokenizer.id_of("<|end_header_id|>");
    let eot = model.tokenizer.id_of("<|eot_id|>");

    match (start, end, eot) {
        (Some(start), Some(end), Some(eot)) => {
            ids.push(start);
            ids.extend(model.tokenizer.encode("user"));
            ids.push(end);
            ids.extend(model.tokenizer.encode(NEWLINES));
            ids.extend(model.tokenizer.encode(question));
            ids.push(eot);
            // The assistant's header with nothing after it: the model's turn
            // begins where the prompt ends, which is what makes it answer
            // rather than continue.
            ids.push(start);
            ids.extend(model.tokenizer.encode("assistant"));
            ids.push(end);
            ids.extend(model.tokenizer.encode(NEWLINES));
        }
        _ => ids.extend(model.tokenizer.encode(question)),
    }
    ids
}

/// Wrap a question as a whole conversation of one turn.
pub fn chat_prompt(model: &Model, question: &str) -> Vec<u32> {
    chat_turn(model, question, true)
}

/// Ask `question` in `session`, continuing whatever it already holds.
///
/// This is what makes an agent's second question a follow-up. The session's
/// key/value cache already holds every token of the conversation so far, and
/// the new turn is fed in at the position after them — so the model attends
/// over the earlier turns without their being re-read, which is the entire
/// reason a cache exists.
///
/// Returns the text the model produced. Stops at a stop token, which is the
/// difference between an answer and a model that runs to the limit.
pub fn ask(
    runner: &mut Runner<'_>,
    session: &mut Session,
    question: &str,
    limit: usize,
) -> Result<String, Error> {
    let model = runner.model();
    let opening = session.is_empty();
    let turn = chat_turn(model, question, opening);

    let mut position = session.len();
    let mut next = 0u32;
    for token in &turn {
        next = argmax(runner.forward(session, *token, position)?);
        position += 1;
    }

    let mut produced = Vec::new();
    for _ in 0..limit {
        if model.stops.contains(&next) {
            break;
        }
        produced.push(next);
        next = argmax(runner.forward(session, next, position)?);
        position += 1;
    }

    // Close the assistant's turn in the cache. Without this the next user turn
    // is appended to an answer the model still believes it is in the middle of,
    // and it reads as one run-on turn rather than two.
    if let Some(eot) = model.tokenizer.id_of("<|eot_id|>") {
        runner.forward(session, eot, position)?;
    }

    Ok(model.tokenizer.decode(&produced))
}

/// Answer `question` in a session, greedily, for at most `limit` tokens.
///
/// Kept as the one-shot spelling of [`ask`]. A caller with no conversation to
/// continue has the same thing either way.
pub fn generate(
    runner: &mut Runner<'_>,
    session: &mut Session,
    question: &str,
    limit: usize,
) -> Result<String, Error> {
    ask(runner, session, question, limit)
}

/// What one lane of a batched generation is doing.
enum Lane {
    /// Still feeding the prompt, at this index into it.
    Prompt(usize),
    /// Generating, with this token to feed next.
    Answering(u32),
    /// Finished, and the tail of the turn has been closed.
    Done,
}

/// Ask several questions at once, one per conversation.
///
/// The point of batching, and the only reason it is worth the bookkeeping: one
/// pass over the weights advances every conversation by a token. A pass reads
/// 1.25 GiB whether it is producing one token or eight, so a fleet with eight
/// agents waiting should not read the model eight times.
///
/// Lanes are independent in every way that matters. They are at different
/// positions, their prompts are different lengths, and they stop at different
/// times — a lane that finishes drops out of the batch and the rest carry on.
/// Nothing is shared between them but the weights, which are read-only.
///
/// # Panics
///
/// If given more conversations than the runner has lanes, or a different number
/// of questions than conversations.
pub fn ask_many(
    runner: &mut Runner<'_>,
    sessions: &mut [&mut Session],
    questions: &[&str],
    limit: usize,
) -> Result<Vec<String>, Error> {
    assert_eq!(
        sessions.len(),
        questions.len(),
        "a question per conversation"
    );
    assert!(
        sessions.len() <= runner.lanes(),
        "{} conversations into a runner with {} lanes",
        sessions.len(),
        runner.lanes()
    );

    let model = runner.model();
    let lanes = sessions.len();
    let turns: Vec<Vec<u32>> = questions
        .iter()
        .zip(sessions.iter())
        .map(|(question, session)| chat_turn(model, question, session.is_empty()))
        .collect();

    let mut state: Vec<Lane> = (0..lanes).map(|_| Lane::Prompt(0)).collect();
    let mut positions: Vec<usize> = sessions.iter().map(|s| s.len()).collect();
    let mut produced: Vec<Vec<u32>> = vec![Vec::new(); lanes];

    loop {
        // Which lanes still have a token to feed, and what it is.
        let mut active: Vec<usize> = Vec::with_capacity(lanes);
        let mut tokens: Vec<u32> = Vec::with_capacity(lanes);
        for (lane, s) in state.iter().enumerate() {
            match s {
                Lane::Prompt(at) => {
                    active.push(lane);
                    tokens.push(turns[lane][*at]);
                }
                Lane::Answering(token) => {
                    active.push(lane);
                    tokens.push(*token);
                }
                Lane::Done => {}
            }
        }
        if active.is_empty() {
            break;
        }

        {
            // The borrow checker needs the active sessions gathered as a slice
            // of exclusive references, and they come from disjoint indices of
            // `sessions` — which it cannot see, so they are taken one at a time
            // with `split_at_mut` folded into an index walk.
            let mut work: Vec<(&mut Session, u32, usize)> = Vec::with_capacity(active.len());
            let mut rest: &mut [&mut Session] = sessions;
            let mut taken = 0usize;
            for (slot, &lane) in active.iter().enumerate() {
                let (_, tail) = rest.split_at_mut(lane - taken);
                let (head, tail) = tail.split_at_mut(1);
                taken = lane + 1;
                rest = tail;
                work.push((head[0], tokens[slot], positions[lane]));
            }
            runner.step(&mut work)?;
        }

        for (slot, &lane) in active.iter().enumerate() {
            let next = argmax(runner.logits(slot));
            positions[lane] += 1;
            state[lane] = match &state[lane] {
                Lane::Prompt(at) if at + 1 < turns[lane].len() => Lane::Prompt(at + 1),
                // The prompt is in; `next` is the first token of the answer.
                Lane::Prompt(_) => {
                    if model.stops.contains(&next) || limit == 0 {
                        Lane::Done
                    } else {
                        produced[lane].push(next);
                        Lane::Answering(next)
                    }
                }
                Lane::Answering(_) => {
                    if model.stops.contains(&next) || produced[lane].len() >= limit {
                        Lane::Done
                    } else {
                        produced[lane].push(next);
                        Lane::Answering(next)
                    }
                }
                Lane::Done => Lane::Done,
            };
        }
    }

    // Close each turn in its own cache, so a follow-up is a new turn rather
    // than a continuation of an answer the model thinks it is still giving.
    if let Some(eot) = model.tokenizer.id_of("<|eot_id|>") {
        for (lane, session) in sessions.iter_mut().enumerate() {
            runner.forward(session, eot, positions[lane])?;
        }
    }

    Ok(produced
        .iter()
        .map(|tokens| model.tokenizer.decode(tokens))
        .collect())
}
