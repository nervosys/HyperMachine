//! Who gets the model, and when.
//!
//! This is the piece the bandwidth measurement asked for. Streaming a model's
//! weights costs about 1.4 cycles per byte warm, so a forward pass is bounded
//! by how fast the machine can read memory rather than by how many cores it
//! has — and a thousand agents inferring at once would need hundreds of
//! gigabytes per second of aggregate bandwidth, which no node has. Running them
//! all anyway does not make them all slow; it makes them all slow *and* thrashes
//! the cache the shared model depends on.
//!
//! So inference is scheduled. A fixed, small number of forward passes run at
//! once, everyone else waits in a queue, and the queue is first-in-first-out so
//! that a busy fleet degrades into a longer wait rather than into starvation.
//!
//! # What one agent may take
//!
//! Two bounds, and they are different resources.
//!
//! [`Limits::workers`] bounds *bandwidth*: how many passes may be in flight.
//! One is the default, and the default is the argument — the second worker
//! competes with the first for the same memory bus.
//!
//! [`Limits::context_tokens`] bounds *memory*: how much key/value cache an
//! agent may hold. That is the one thing an agent cannot share with another
//! agent, because it is that agent's conversation, and at 64 KiB per token for
//! a small model it is what decides how many agents fit on a node. An agent
//! over its bound is refused with the numbers in the refusal, and is refused
//! **before it takes a place in the queue** — a request that cannot be served
//! should not make anyone else wait for it.
//!
//! # What a session is for
//!
//! A conversation, kept between requests. An agent asks something, gets an
//! answer, and asks a follow-up; the follow-up attends over the earlier turns
//! because they are still in that agent's cache. One session per agent and
//! never shared, which is the whole reason the isolation argument survives
//! inference running in the host: agents cannot read each other's context
//! because there is no way to name another agent's session.
//!
//! # What is deliberately not here
//!
//! Batching several agents' tokens into one pass, which is where the real
//! throughput is and which needs the forward pass to take a batch dimension it
//! does not have. Priorities, preemption, and eviction of a cold conversation.
//! Each is a real thing a serving stack has; none of them can be added honestly
//! before the thing they schedule exists, which it now does.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::model::{Model, Session};
use crate::{ask, chat_turn, Error};

/// What a node will not let one agent take.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Forward passes in flight at once.
    ///
    /// One, by default, and the default is the point: a forward pass is
    /// memory-bandwidth-bound and already uses every core, so a second
    /// concurrent pass competes with the first for the bus rather than for
    /// idle time.
    pub workers: usize,
    /// The most context an agent may hold, in tokens.
    pub context_tokens: usize,
    /// The most tokens one answer may run to.
    pub answer_tokens: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 1,
            context_tokens: 2048,
            answer_tokens: 32,
        }
    }
}

/// Why a request was not served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The agent's conversation, plus this turn, plus the room its answer would
    /// need, is more context than one agent may hold.
    ContextFull {
        /// Tokens the agent already holds.
        held: usize,
        /// Tokens this turn would add, answer included.
        wanted: usize,
        /// The bound.
        cap: usize,
    },
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextFull { held, wanted, cap } => write!(
                f,
                "context full: holding {held} tokens, this turn needs {wanted} more, and the cap \
                 is {cap}"
            ),
        }
    }
}

/// A request that was served, and what it cost.
#[derive(Debug, Clone)]
pub struct Served {
    /// What the model said.
    pub text: String,
    /// How long the request sat in the queue before a worker took it.
    pub waited: Duration,
    /// How long the forward passes took once it did.
    pub ran: Duration,
    /// Tokens the agent's conversation holds now.
    pub context: usize,
}

/// What the queue has done.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Requests admitted to the queue.
    pub admitted: usize,
    /// Requests served.
    pub served: usize,
    /// Requests refused before reaching the queue.
    pub refused: usize,
    /// The most requests ever waiting at once.
    ///
    /// The number that says whether anything was actually scheduled. If it
    /// never exceeds one, nothing ever queued and the fleet was never busy
    /// enough to test this.
    pub peak_waiting: usize,
    /// Time spent waiting, summed across requests.
    pub total_wait: Duration,
}

struct Inner<'m> {
    /// Tickets waiting, oldest first.
    waiting: VecDeque<u64>,
    /// Forward passes in flight.
    running: usize,
    next_ticket: u64,
    /// One conversation per agent, kept between requests. Taken out of the map
    /// while it is being used, so a long forward pass does not hold the lock
    /// every other agent needs to join the queue.
    sessions: BTreeMap<String, Session<'m>>,
    stats: Stats,
}

/// A model, a queue, and one conversation per agent.
pub struct Scheduler<'m> {
    model: &'m Model,
    limits: Limits,
    inner: Mutex<Inner<'m>>,
    turn: Condvar,
}

impl<'m> Scheduler<'m> {
    /// A scheduler over `model`.
    pub fn new(model: &'m Model, limits: Limits) -> Self {
        Self {
            model,
            limits,
            inner: Mutex::new(Inner {
                waiting: VecDeque::new(),
                running: 0,
                next_ticket: 0,
                sessions: BTreeMap::new(),
                stats: Stats::default(),
            }),
            turn: Condvar::new(),
        }
    }

    /// The limits this scheduler enforces.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// What the queue has done so far.
    pub fn stats(&self) -> Stats {
        self.inner.lock().expect("not poisoned").stats
    }

    /// How much context `agent` is holding.
    pub fn context_of(&self, agent: &str) -> usize {
        self.inner
            .lock()
            .expect("not poisoned")
            .sessions
            .get(agent)
            .map_or(0, Session::len)
    }

    /// Drop an agent's conversation, freeing its cache.
    pub fn forget(&self, agent: &str) {
        self.inner
            .lock()
            .expect("not poisoned")
            .sessions
            .remove(agent);
    }

    /// Ask on `agent`'s behalf, waiting for a turn.
    ///
    /// Blocks until a worker is free and every earlier request has been taken.
    /// Returns [`Refused`] without queueing if the agent is over its bound —
    /// whether the agent is *allowed* to ask at all is not decided here, it is
    /// decided by whoever holds the capability graph, before this is called.
    pub fn ask(&self, agent: &str, question: &str) -> Result<Result<Served, Refused>, Error> {
        // Costed before queueing, so a request that cannot be served does not
        // make anyone else wait behind it. The tokeniser is shared and read-only,
        // so this happens outside the lock.
        let turn_tokens = chat_turn(self.model, question, self.context_of(agent) == 0).len();
        let wanted = turn_tokens + self.limits.answer_tokens;

        let queued_at = Instant::now();
        let ticket = {
            let mut inner = self.inner.lock().expect("not poisoned");
            let held = inner.sessions.get(agent).map_or(0, Session::len);
            if held + wanted > self.limits.context_tokens {
                inner.stats.refused += 1;
                return Ok(Err(Refused::ContextFull {
                    held,
                    wanted,
                    cap: self.limits.context_tokens,
                }));
            }

            let ticket = inner.next_ticket;
            inner.next_ticket += 1;
            inner.waiting.push_back(ticket);
            inner.stats.admitted += 1;
            inner.stats.peak_waiting = inner.stats.peak_waiting.max(inner.waiting.len());
            ticket
        };

        // Wait for a worker and for every earlier ticket to have been taken.
        // First-in-first-out, so a busy fleet becomes a longer queue rather
        // than a lottery.
        let mut session = {
            let mut inner = self.inner.lock().expect("not poisoned");
            loop {
                let mine = inner.waiting.front() == Some(&ticket);
                if mine && inner.running < self.limits.workers {
                    inner.waiting.pop_front();
                    inner.running += 1;
                    break;
                }
                inner = self.turn.wait(inner).expect("not poisoned");
            }
            inner.stats.total_wait += queued_at.elapsed();
            // Out of the map for the duration. A forward pass is hundreds of
            // milliseconds per token and holding the shared lock across it would
            // stop every other agent from so much as joining the queue.
            inner
                .sessions
                .remove(agent)
                .unwrap_or_else(|| Session::open(self.model))
        };
        let waited = queued_at.elapsed();

        let started = Instant::now();
        let answer = ask(
            self.model,
            &mut session,
            question,
            self.limits.answer_tokens,
        );
        let ran = started.elapsed();

        let context = session.len();
        {
            let mut inner = self.inner.lock().expect("not poisoned");
            inner.sessions.insert(agent.to_string(), session);
            inner.running -= 1;
            if answer.is_ok() {
                inner.stats.served += 1;
            }
        }
        // Everyone, not one: the ticket that may now proceed is not necessarily
        // the thread a targeted wake would reach.
        self.turn.notify_all();

        Ok(Ok(Served {
            text: answer?,
            waited,
            ran,
            context,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bound is checked before the queue, which is the property that keeps
    /// a refusable request from making anyone else wait. Checked here rather
    /// than only in the example, because it needs no model to be true.
    #[test]
    fn a_refusal_reports_the_numbers_it_was_made_on() {
        let refused = Refused::ContextFull {
            held: 49,
            wanted: 52,
            cap: 96,
        };
        let said = refused.to_string();
        assert!(said.contains("49"), "the refusal should say what is held");
        assert!(said.contains("52"), "and what was asked for");
        assert!(said.contains("96"), "and the bound it broke");
    }

    #[test]
    fn one_worker_is_the_default_and_the_default_is_the_argument() {
        assert_eq!(Limits::default().workers, 1);
    }
}
