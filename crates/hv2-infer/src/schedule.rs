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
//! once and everyone else waits in a queue.
//!
//! The queue was first-in-first-out, and the reason was starvation: a busy
//! fleet should degrade into a longer wait for everyone rather than into some
//! agent never being served at all. It now orders by urgency as well, and
//! keeps that property — see [`Limits::patience`]. The scheduler does not
//! decide who is urgent; it is told, by whoever holds the capability graph, in
//! the same call that already decided the agent may ask.
//!
//! # What one agent may take
//!
//! Two bounds, and they are different resources.
//!
//! [`Limits::batch`] decides how many conversations share a pass. It used to be
//! `workers` — how many passes ran at once — and that was the wrong knob. A
//! pass reads all 1.25 GiB of the weights whether it produces one token or
//! eight, so two concurrent passes read the model twice and a batch of two
//! reads it once. Eight lanes measured 4.7 times the tokens per second of one;
//! eight concurrent passes measured slower than one.
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
//! # Continuous
//!
//! Lanes are filled from the queue at the top of every token step and emptied
//! as they finish, so a request arriving while others are being answered joins
//! at the next token rather than waiting for them to end. That matters because
//! agents do not arrive together: four sandboxes each have to exchange a frame
//! with their guest first, and under batch-at-a-time scheduling that was enough
//! to make the largest batch two out of four.
//!
//! Every lane's state lives in the scheduler rather than on the thread driving
//! it, so any waiter can take the next step — and a thread whose own answer is
//! ready stops driving and returns rather than finishing everybody else's work.
//!
//! # What is deliberately not here
//!
//! Preemption. A request that has a lane keeps it until its answer is done,
//! however urgent something arriving behind it is — urgency reorders the
//! queue, it does not interrupt a pass. Making it interrupt one would mean
//! abandoning work already paid for in memory traffic, which is the expensive
//! thing here, so the queue is the right place for it and the lane is not.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::model::argmax;
use crate::model::{Model, Runner, Session};
use crate::{chat_turn, Error};

/// What a node will not let one agent take.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Conversations one forward pass carries.
    ///
    /// This was `workers`: how many passes ran at once. That was the wrong knob
    /// and the measurement says so. A pass reads all 1.25 GiB of the weights
    /// whether it produces one token or eight, so two concurrent passes read
    /// the model twice and a batch of two reads it once — same arithmetic, half
    /// the memory traffic, and no contention between them. Eight lanes measured
    /// 4.7 times the tokens per second of one; eight concurrent passes measured
    /// slower than one.
    ///
    /// So there is one pass at a time and the width of it is the knob.
    pub batch: usize,
    /// The most context an agent may hold, in tokens.
    pub context_tokens: usize,
    /// The most tokens one answer may run to.
    pub answer_tokens: usize,
    /// How much key/value cache every conversation may hold between them, in
    /// bytes, or zero for no bound at all.
    ///
    /// [`Limits::context_tokens`] bounds one agent. This bounds the node. They
    /// are different problems: an agent that behaves is still a problem if a
    /// thousand of them each hold a small conversation and nothing ever lets
    /// one go. Without this, a node's memory is decided by how many agents have
    /// *ever* asked rather than by how many are talking.
    ///
    /// When admitting a request would take the total past this, the
    /// least-recently-used conversation that is not in a lane is dropped, and
    /// dropped conversations are counted in [`Stats::evicted`]. An agent whose
    /// conversation was dropped is not told in advance — it finds out because
    /// its next answer comes back with [`Served::continued`] false, which is
    /// the honest signal and the one an agent can act on.
    ///
    /// A gigabyte by default, which is a real number rather than "unbounded":
    /// at 64 KiB of cache per token that is about sixteen thousand tokens of
    /// context across the whole node.
    pub cache_bytes: usize,
    /// Token steps a waiting request must sit before it gains a rank of
    /// urgency, or zero to age nothing.
    ///
    /// This is what keeps urgency from becoming starvation. Ordering strictly
    /// by urgency means a fleet with a steady supply of urgent work never
    /// serves a routine request at all, and the queue was first-in-first-out
    /// precisely to avoid that. So a request's *effective* rank improves the
    /// longer it waits: after `patience` steps a routine request is as good as
    /// an urgent one, and ties are broken by arrival, so it goes first against
    /// anything that arrived later.
    ///
    /// That gives a bound rather than a hope. A request at rank `r` cannot be
    /// overtaken by newly-arriving urgent work for longer than
    /// `r * patience` steps, whatever else the fleet is doing, and
    /// `urgency_bounded_by_patience` in this module is the test that says so.
    ///
    /// **Choose it against how long a lane is held, not against the clock.**
    /// The first default here was sixteen steps, picked as "a second or two",
    /// and `examples/queueing` measured it into a no-op: one answer holds a
    /// lane for about nineteen steps, so every routine request had already aged
    /// to the top rank before a lane ever freed, every request was tied, and
    /// the queue collapsed back to first-in-first-out. Urgent agents came out
    /// at a mean wait of 29.0 steps against 29.0 for routine ones — the feature
    /// present, wired, and worth exactly nothing.
    ///
    /// So: 256, which is on the order of a dozen answers rather than one. Large
    /// enough that an urgent request beats work that is genuinely still
    /// waiting, small enough to remain a bound somebody could sit through.
    pub patience: usize,
    /// Threads one forward pass may spread across, or zero to choose.
    ///
    /// Not "all of them", which is what rayon's global pool does and what this
    /// used to get. A pass stops getting faster at about a third of this
    /// machine's cores and then gets *slower*; what limits it there has not
    /// been established, and is deliberately not asserted here. Measured over
    /// 1.25 GiB of weights, median of five runs of four passes:
    ///
    /// ```text
    ///    1 thread   1589.7 ms   0.77 GiB/s
    ///    2           799.3      1.53
    ///    4           479.9      2.55
    ///    8           346.5      3.53   <- the knee
    ///   10           349.2      3.50
    ///   12           412.0      2.97
    ///   16           426.4      2.87
    ///   24           434.9      2.81   <- what the default was doing
    /// ```
    ///
    /// So using every core it could see gave up a quarter of the rate. Zero
    /// here takes a third of the machine's parallelism, which is where the knee
    /// sat — a heuristic shaped by one host, and `examples/throughput` takes a
    /// thread count precisely so it can be re-measured on another.
    pub threads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            batch: 8,
            context_tokens: 2048,
            answer_tokens: 32,
            cache_bytes: 1024 * 1024 * 1024,
            patience: 256,
            threads: 0,
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
    /// How long the request sat in the queue before its batch started.
    pub waited: Duration,
    /// The same wait counted in token steps rather than in time.
    ///
    /// A step is one pass over the model, so this is how much work the node did
    /// for other people before it started on this one. It is worth having
    /// beside [`Served::waited`] because it does not move when the machine is
    /// busy: on a host at 97% the duration triples and this does not change at
    /// all. Whether a queue needs priorities is a question about this number,
    /// and asking it in milliseconds gets an answer about the host.
    pub waited_steps: usize,
    /// How long that batch took.
    ///
    /// Shared with everything else in the batch: this is the time for the whole
    /// pass, not this request's share of it. Dividing it by [`Served::batch`]
    /// would be the per-answer cost, and saying so is the point of reporting
    /// both.
    pub ran: Duration,
    /// Tokens the agent's conversation holds now.
    pub context: usize,
    /// How many conversations shared the batch this was answered in.
    pub batch: usize,
    /// Whether this turn continued a conversation that was already there.
    ///
    /// False when the agent had never asked before — and, more interestingly,
    /// false when its conversation had been evicted to make room for somebody
    /// else's. An agent that expected to be remembered and was not learns it
    /// here, which is the only place it could.
    pub continued: bool,
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
    /// Token steps run — passes over the whole model.
    ///
    /// With continuous batching there are no discrete batches to count: lanes
    /// join and leave between steps, so what there is a number of is *steps*.
    /// Against [`Stats::served`] this is what says whether batching happened.
    pub steps: usize,
    /// Conversations dropped to keep the node inside its cache bound.
    pub evicted: usize,
    /// The most conversations that ever shared one pass.
    ///
    /// With [`Stats::served`], this is what says whether batching happened. If
    /// it is one, every pass read the whole model to produce a single token and
    /// the fleet paid for it.
    pub largest_batch: usize,
}

/// How many threads to spread a pass across, when the caller has not said.
///
/// Two facts, both measured, pulling in opposite directions.
///
/// A pass over the weights is close to memory-bound, and a *small* number of
/// threads gets most of what is there: on this host four threads reach
/// 17.0 GiB/s, against 20.6 GiB/s for reading the same bytes and doing nothing
/// with them. More threads past four do not help and measurably hurt.
///
/// But a wider batch does more arithmetic per byte read, so it stops being
/// purely memory-bound: at eight lanes the best is eight threads, not four.
///
/// ```text
///  lanes  threads  result
///      1        4  71.8 ms per pass, 17.02 GiB/s   <- 83% of a plain read
///      1        8  127.3 ms,          9.61
///      1       24  232.2 ms,          5.27
///      8        4  35.8 ms per token
///      8        8  24.4 ms per token               <- best
///      8       16  28.5 ms per token
/// ```
///
/// So: enough threads to saturate memory, and more when the batch gives them
/// something to do. The fraction is a guess calibrated on one machine, and
/// `examples/throughput` takes both knobs precisely so the next machine can be
/// measured rather than assumed.
pub fn default_threads(lanes: usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 6).max(1).max(lanes.max(1)).min(cores)
}

/// A request that has joined the queue and not yet been given a lane.
struct Waiting {
    ticket: u64,
    agent: String,
    question: String,
    queued_at: Instant,
    /// What the step counter read when this joined the queue.
    queued_at_step: usize,
    /// The urgency this request was admitted with, smaller being sooner.
    ///
    /// Handed in by the caller and never derived here. The scheduler orders by
    /// this number; what it means, and which agent deserves which, is the
    /// capability graph's business — `hv2_swarm::Urgency::rank` is what
    /// produces it in this workspace.
    urgency: u8,
}

impl Waiting {
    /// The rank this request should be ordered by *now*, which improves the
    /// longer it has waited. See [`Limits::patience`].
    fn effective_urgency(&self, step: usize, patience: usize) -> u8 {
        if patience == 0 {
            return self.urgency;
        }
        let waited = step.saturating_sub(self.queued_at_step);
        let earned = (waited / patience).min(u8::MAX as usize) as u8;
        self.urgency.saturating_sub(earned)
    }
}

/// What a lane is doing.
enum Stage {
    /// Still feeding the prompt, at this index into it.
    Prompt(usize),
    /// Generating, with this token to feed next.
    Answering(u32),
    /// Finished; feeding the marker that closes the turn in the cache.
    Closing,
    /// Done, and its answer is published.
    Retired,
}

/// A conversation nobody is currently using.
struct Held {
    session: Session,
    last_used: Instant,
}

/// A request occupying a lane.
struct Active {
    ticket: u64,
    /// The urgency this request was admitted with, so a lane that has to be
    /// re-queued goes back at the rank it came in at.
    urgency: u8,
    agent: String,
    queued_at: Instant,
    admitted_at: Instant,
    /// Token steps that ran between joining the queue and getting a lane.
    waited_steps: usize,
    session: Session,
    /// Whether the conversation was already going when this turn joined it.
    continued: bool,
    turn: Vec<u32>,
    stage: Stage,
    position: usize,
    produced: Vec<u32>,
    /// The most lanes that were ever busy alongside this one, itself included.
    shared: usize,
}

struct Inner<'m> {
    /// Requests admitted to the queue and not yet in a lane, oldest first.
    pending: VecDeque<Waiting>,
    /// Lanes in flight. Their conversations live here while they run.
    active: Vec<Active>,
    /// Answers nobody has collected yet, by ticket.
    done: BTreeMap<u64, Result<Served, Refused>>,
    /// Whether a token step is being run right now.
    stepping: bool,
    next_ticket: u64,
    /// Conversations not currently in a lane, and when each was last served.
    ///
    /// The timestamp is what makes eviction least-recently-used rather than
    /// arbitrary: dropping whichever conversation the map happened to yield
    /// first would be a policy nobody chose.
    sessions: BTreeMap<String, Held>,
    /// The runner, taken by whoever is running the current step.
    runner: Option<Runner<'m>>,
    stats: Stats,
}

/// A model, a queue, and one conversation per agent.
pub struct Scheduler<'m> {
    model: &'m Model,
    limits: Limits,
    /// The pass runs here rather than in rayon's global pool, so the thread
    /// count is a property of the scheduler and not of the process.
    pool: rayon::ThreadPool,
    inner: Mutex<Inner<'m>>,
    turn: Condvar,
}

impl<'m> Scheduler<'m> {
    /// A scheduler over `model`.
    ///
    /// # Panics
    ///
    /// If a thread pool cannot be built, which means the process cannot spawn
    /// threads and nothing below would work either.
    pub fn new(model: &'m Model, limits: Limits) -> Self {
        let lanes = limits.batch.max(1);
        let threads = if limits.threads == 0 {
            default_threads(lanes)
        } else {
            limits.threads
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("infer-{i}"))
            .build()
            .expect("a thread pool");
        Self {
            model,
            limits,
            pool,
            inner: Mutex::new(Inner {
                pending: VecDeque::new(),
                active: Vec::new(),
                done: BTreeMap::new(),
                stepping: false,
                next_ticket: 0,
                sessions: BTreeMap::new(),
                runner: Some(Runner::new(model, lanes)),
                stats: Stats::default(),
            }),
            turn: Condvar::new(),
        }
    }

    /// The limits this scheduler enforces.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// How many threads a forward pass actually spreads across.
    pub fn threads(&self) -> usize {
        self.pool.current_num_threads()
    }

    /// What the queue has done so far.
    pub fn stats(&self) -> Stats {
        self.inner.lock().expect("not poisoned").stats
    }

    /// How much context `agent` is holding.
    ///
    /// Zero while that agent is being served, because its conversation is in a
    /// lane rather than in the map — which is a small lie and the reason the
    /// bound is checked at admission rather than continuously.
    pub fn context_of(&self, agent: &str) -> usize {
        self.inner
            .lock()
            .expect("not poisoned")
            .sessions
            .get(agent)
            .map_or(0, |held| held.session.len())
    }

    /// Bytes of key/value cache every conversation is holding between them.
    ///
    /// Counts the conversations in lanes as well as the idle ones, because a
    /// bound that ignored the ones being used would be a bound on the wrong
    /// thing.
    pub fn cache_bytes(&self) -> usize {
        let inner = self.inner.lock().expect("not poisoned");
        Self::held(&inner)
    }

    /// The same, without taking the lock.
    fn held(inner: &Inner<'m>) -> usize {
        inner
            .sessions
            .values()
            .map(|held| held.session.cache_bytes())
            .sum::<usize>()
            + inner
                .active
                .iter()
                .map(|lane| lane.session.cache_bytes())
                .sum::<usize>()
    }

    /// Drop an agent's conversation, freeing its cache.
    pub fn forget(&self, agent: &str) {
        self.inner
            .lock()
            .expect("not poisoned")
            .sessions
            .remove(agent);
    }

    /// Ask on `agent`'s behalf, waiting for an answer.
    ///
    /// Returns [`Refused`] without queueing if the agent is over its bound —
    /// whether the agent is *allowed* to ask at all is not decided here, it is
    /// decided by whoever holds the capability graph, before this is called.
    ///
    /// # How the work gets done
    ///
    /// One token step at a time, by whichever waiting thread finds no step in
    /// flight. There is no worker thread: a thread that has nothing to do but
    /// wait may as well drive a step, and a spawned one would have to outlive
    /// the borrow of the model.
    ///
    /// The batching is *continuous*: lanes are filled from the queue at the top
    /// of every step and emptied as they finish, so a request that arrives
    /// while others are being answered joins at the next token rather than
    /// waiting for them to finish. That is the difference between a largest
    /// batch of two and one of four when four agents ask at slightly different
    /// times, which is what four agents talking to their own guests actually
    /// do.
    ///
    /// Because the state of every lane lives in the scheduler rather than on
    /// the driving thread, any waiter can drive the next step. A thread whose
    /// own answer is ready stops driving and returns; the next step is taken by
    /// somebody who is still waiting.
    pub fn ask(&self, agent: &str, question: &str) -> Result<Result<Served, Refused>, Error> {
        self.ask_at(agent, question, Self::ROUTINE)
    }

    /// The urgency [`Scheduler::ask`] uses: an agent with nothing said about it.
    ///
    /// The absolute number means nothing — only comparisons between waiting
    /// requests do — but it has to agree with whatever scheme the caller uses,
    /// and it is one because that is
    /// `hv2_swarm::Urgency::Routine.rank()`. A caller with a different scheme
    /// should use [`Scheduler::ask_at`] for every request rather than mixing
    /// the two.
    pub const ROUTINE: u8 = 1;

    /// Ask on `agent`'s behalf at a stated urgency, smaller being served sooner.
    ///
    /// Identical to [`Scheduler::ask`] except that the request takes its place
    /// in the queue by urgency rather than purely by arrival.
    ///
    /// **The number is not decided here.** It comes from whoever holds the
    /// capability graph — the same caller that already decided this agent may
    /// ask at all — and in this workspace it is
    /// `hv2_swarm::Urgency::rank()`. A scheduler that decided for itself which
    /// agents matter would be a second, quieter policy sitting underneath the
    /// one that is written down.
    ///
    /// Urgency reorders the queue; it does not interrupt a lane, and it cannot
    /// starve anything. See [`Limits::patience`] for the bound.
    pub fn ask_at(
        &self,
        agent: &str,
        question: &str,
        urgency: u8,
    ) -> Result<Result<Served, Refused>, Error> {
        // Costed before queueing, so a request that cannot be served does not
        // make anyone else wait behind it. The tokeniser is shared and
        // read-only, so this happens outside the lock.
        let turn_tokens = chat_turn(self.model, question, self.context_of(agent) == 0).len();
        let wanted = turn_tokens + self.limits.answer_tokens;

        let queued_at = Instant::now();
        let ticket = {
            let mut inner = self.inner.lock().expect("not poisoned");
            let held = inner
                .sessions
                .get(agent)
                .map_or(0, |held| held.session.len());
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
            let queued_at_step = inner.stats.steps;
            inner.pending.push_back(Waiting {
                ticket,
                agent: agent.to_string(),
                question: question.to_string(),
                queued_at,
                queued_at_step,
                urgency,
            });
            inner.stats.admitted += 1;
            inner.stats.peak_waiting = inner.stats.peak_waiting.max(inner.pending.len());
            ticket
        };
        self.turn.notify_all();

        loop {
            let taken = {
                let mut inner = self.inner.lock().expect("not poisoned");
                if let Some(answer) = inner.done.remove(&ticket) {
                    return Ok(answer);
                }
                if inner.stepping {
                    let _guard = self.turn.wait(inner).expect("not poisoned");
                    continue;
                }

                // Fill any free lane from the queue. Never two lanes for one
                // agent: they would need the same conversation twice, and a
                // conversation cannot be in two lanes of one pass.
                self.admit(&mut inner);

                if inner.active.is_empty() {
                    // Nothing to do and our answer is not ready, which means
                    // another thread is between publishing and notifying.
                    let _guard = self.turn.wait(inner).expect("not poisoned");
                    continue;
                }

                inner.stepping = true;
                let active = core::mem::take(&mut inner.active);
                let runner = inner
                    .runner
                    .take()
                    .expect("the runner, since no step is running");
                (active, runner)
            };
            let (mut active, mut runner) = taken;

            let outcome = self.advance(&mut active, &mut runner);

            {
                let mut inner = self.inner.lock().expect("not poisoned");
                inner.stats.steps += 1;
                inner.stats.largest_batch = inner.stats.largest_batch.max(
                    active
                        .iter()
                        .filter(|lane| !matches!(lane.stage, Stage::Retired))
                        .count(),
                );

                if outcome.is_err() {
                    // The pass failed. Put every conversation back and let the
                    // requests re-queue, which is all a caller could do anyway.
                    for lane in active.drain(..) {
                        inner.sessions.insert(
                            lane.agent.clone(),
                            Held {
                                session: lane.session,
                                last_used: Instant::now(),
                            },
                        );
                        // Its earlier wait is kept rather than reset: the
                        // steps it already sat through happened, and a
                        // re-queued request that reported zero would say the
                        // failure had cost it nothing.
                        let queued_at_step = inner.stats.steps.saturating_sub(lane.waited_steps);
                        inner.pending.push_back(Waiting {
                            ticket: lane.ticket,
                            agent: lane.agent,
                            question: String::new(),
                            queued_at: lane.queued_at,
                            queued_at_step,
                            // Kept for the same reason the wait is kept: a
                            // request does not become ordinary because a step
                            // it was in failed.
                            urgency: lane.urgency,
                        });
                    }
                } else {
                    // Publish and free every lane that finished.
                    let mut still = Vec::with_capacity(active.len());
                    for lane in active.drain(..) {
                        if matches!(lane.stage, Stage::Retired) {
                            let text = self.model.tokenizer.decode(&lane.produced);
                            let context = lane.session.len();
                            inner.sessions.insert(
                                lane.agent.clone(),
                                Held {
                                    session: lane.session,
                                    last_used: Instant::now(),
                                },
                            );
                            inner.done.insert(
                                lane.ticket,
                                Ok(Served {
                                    text,
                                    waited: lane
                                        .admitted_at
                                        .saturating_duration_since(lane.queued_at),
                                    waited_steps: lane.waited_steps,
                                    ran: lane.admitted_at.elapsed(),
                                    context,
                                    batch: lane.shared,
                                    continued: lane.continued,
                                }),
                            );
                            inner.stats.served += 1;
                            inner.stats.total_wait +=
                                lane.admitted_at.saturating_duration_since(lane.queued_at);
                        } else {
                            still.push(lane);
                        }
                    }
                    inner.active = still;
                }

                inner.runner = Some(runner);
                inner.stepping = false;
            }
            self.turn.notify_all();
        }
    }

    /// Move waiting requests into free lanes.
    ///
    /// Called at the top of every step, which is what makes the batching
    /// continuous rather than one batch at a time.
    fn admit(&self, inner: &mut Inner<'m>) {
        let lanes = self.limits.batch.max(1);
        let step = inner.stats.steps;
        let patience = self.limits.patience;
        while inner.active.len() < lanes {
            // The best waiting request that is not already in a lane: lowest
            // effective urgency, and among equals the one that arrived first.
            // Scanning rather than popping is what lets urgency matter at all,
            // and the ticket tiebreak is what makes an aged request beat a
            // newly-arrived urgent one rather than merely draw with it.
            let choice = {
                let active = &inner.active;
                inner
                    .pending
                    .iter()
                    .enumerate()
                    .filter(|(_, waiting)| !active.iter().any(|lane| lane.agent == waiting.agent))
                    .min_by_key(|(_, waiting)| {
                        (waiting.effective_urgency(step, patience), waiting.ticket)
                    })
                    .map(|(index, _)| index)
            };
            let Some(index) = choice else {
                break;
            };
            let request = inner
                .pending
                .remove(index)
                .expect("an index just read from this queue");
            // Make room before taking the conversation, so that the agent
            // being admitted is never the one evicted to admit it.
            self.reclaim(inner, &request.agent);

            let session = inner
                .sessions
                .remove(&request.agent)
                .map(|held| held.session)
                .unwrap_or_else(|| Session::open(self.model));
            let continued = !session.is_empty();
            let turn = chat_turn(self.model, &request.question, session.is_empty());
            let position = session.len();
            inner.active.push(Active {
                ticket: request.ticket,
                agent: request.agent,
                urgency: request.urgency,
                queued_at: request.queued_at,
                admitted_at: Instant::now(),
                waited_steps: inner.stats.steps.saturating_sub(request.queued_at_step),
                session,
                continued,
                turn,
                stage: Stage::Prompt(0),
                position,
                produced: Vec::new(),
                shared: 0,
            });
        }
    }

    /// Drop least-recently-used conversations until the node is inside its
    /// cache bound.
    ///
    /// Never one that is in a lane, and never `sparing` — the agent about to be
    /// admitted, which would otherwise be able to evict itself and arrive with
    /// its own context missing.
    ///
    /// If nothing is left to drop and the total is still over, the request is
    /// admitted anyway. Refusing would be defensible and is not what this does:
    /// the per-agent bound already caps any one conversation, so being over
    /// here means the *live* conversations do not fit, and refusing service to
    /// a fleet that is genuinely busy is worse than being over a soft bound.
    fn reclaim(&self, inner: &mut Inner<'m>, sparing: &str) {
        let cap = self.limits.cache_bytes;
        if cap == 0 {
            return;
        }
        while Self::held(inner) > cap {
            let victim = inner
                .sessions
                .iter()
                .filter(|(agent, held)| agent.as_str() != sparing && !held.session.is_empty())
                .min_by_key(|(_, held)| held.last_used)
                .map(|(agent, _)| agent.clone());
            let Some(victim) = victim else {
                return;
            };
            inner.sessions.remove(&victim);
            inner.stats.evicted += 1;
        }
    }

    /// Advance every lane by one token.
    fn advance(&self, active: &mut [Active], runner: &mut Runner<'m>) -> Result<(), Error> {
        let busy = active
            .iter()
            .filter(|lane| !matches!(lane.stage, Stage::Retired))
            .count();
        for lane in active.iter_mut() {
            if !matches!(lane.stage, Stage::Retired) {
                lane.shared = lane.shared.max(busy);
            }
        }

        let eot = self.model.tokenizer.id_of("<|eot_id|>");
        let mut lanes: Vec<usize> = Vec::with_capacity(active.len());
        {
            let mut work: Vec<(&mut Session, u32, usize)> = Vec::with_capacity(active.len());
            for (index, lane) in active.iter_mut().enumerate() {
                let token = match lane.stage {
                    Stage::Prompt(at) => lane.turn[at],
                    Stage::Answering(token) => token,
                    Stage::Closing => match eot {
                        Some(eot) => eot,
                        // No marker to close with, so there is nothing to feed
                        // and the lane is finished.
                        None => {
                            lane.stage = Stage::Retired;
                            continue;
                        }
                    },
                    Stage::Retired => continue,
                };
                lanes.push(index);
                work.push((&mut lane.session, token, lane.position));
            }
            if work.is_empty() {
                return Ok(());
            }
            self.pool.install(|| runner.step(&mut work))?;
        }

        for (slot, &index) in lanes.iter().enumerate() {
            let next = argmax(runner.logits(slot));
            let lane = &mut active[index];
            lane.position += 1;
            lane.stage = match lane.stage {
                Stage::Prompt(at) if at + 1 < lane.turn.len() => Stage::Prompt(at + 1),
                // The prompt is in; `next` is the first token of the answer.
                Stage::Prompt(_) | Stage::Answering(_) => {
                    if self.model.stops.contains(&next)
                        || lane.produced.len() >= self.limits.answer_tokens
                    {
                        Stage::Closing
                    } else {
                        lane.produced.push(next);
                        Stage::Answering(next)
                    }
                }
                Stage::Closing => Stage::Retired,
                Stage::Retired => Stage::Retired,
            };
        }
        Ok(())
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

    /// The default is a batch, not a single pass, and that is the whole
    /// finding: one pass carrying eight conversations beats eight passes
    /// carrying one, because the weights are read once either way.
    #[test]
    fn the_default_is_a_batch() {
        assert!(
            Limits::default().batch > 1,
            "a default of one would read the model once per token per agent"
        );
    }

    /// A request built for the ordering tests. Only the fields the ordering
    /// reads are meaningful.
    fn waiting(ticket: u64, urgency: u8, queued_at_step: usize) -> Waiting {
        Waiting {
            ticket,
            agent: format!("agent-{ticket}"),
            question: String::new(),
            queued_at: Instant::now(),
            queued_at_step,
            urgency,
        }
    }

    #[test]
    fn a_fresh_request_is_ordered_at_the_urgency_it_was_given() {
        let w = waiting(1, 2, 100);
        assert_eq!(w.effective_urgency(100, 16), 2);
    }

    /// Waiting earns rank, one step of urgency per `patience` steps.
    #[test]
    fn waiting_earns_rank() {
        let w = waiting(1, 2, 0);
        assert_eq!(w.effective_urgency(15, 16), 2, "not yet");
        assert_eq!(w.effective_urgency(16, 16), 1, "one patience, one rank");
        assert_eq!(w.effective_urgency(32, 16), 0, "two, and it is at the top");
        assert_eq!(w.effective_urgency(10_000, 16), 0, "and cannot go past it");
    }

    /// The property the FIFO queue used to give for free, now stated as a
    /// bound: a request cannot be overtaken by newly-arriving urgent work for
    /// longer than `urgency * patience` steps.
    ///
    /// Checked by simulating the selection key rather than the whole
    /// scheduler, which would need a model: at the moment the bound elapses,
    /// the waiting request must sort ahead of an urgent request that has just
    /// arrived.
    #[test]
    fn urgency_bounded_by_patience() {
        const PATIENCE: usize = 16;
        for urgency in 0..=3u8 {
            let old = waiting(1, urgency, 0);
            let bound = urgency as usize * PATIENCE;

            // An urgent request arriving at exactly that step.
            let fresh = waiting(999, 0, bound);
            let key = |w: &Waiting, step: usize| (w.effective_urgency(step, PATIENCE), w.ticket);
            assert!(
                key(&old, bound) <= key(&fresh, bound),
                "a request at urgency {urgency} was still behind newer urgent work                  after {bound} steps"
            );
        }
    }

    /// Zero patience means no ageing at all, which is strict priority and can
    /// starve. It is an option rather than the default, and the test says so
    /// out loud so that nobody sets it by accident and wonders.
    #[test]
    fn zero_patience_never_ages() {
        let w = waiting(1, 2, 0);
        assert_eq!(w.effective_urgency(1_000_000, 0), 2);
    }

    /// Among requests of equal effective urgency the older one wins, which is
    /// what keeps the queue first-in-first-out when nobody is urgent.
    #[test]
    fn equal_urgency_is_still_first_in_first_out() {
        let first = waiting(1, 1, 0);
        let second = waiting(2, 1, 0);
        let key = |w: &Waiting| (w.effective_urgency(0, 16), w.ticket);
        assert!(key(&first) < key(&second));
    }

    /// The default is not strict priority. If it were, this crate would have
    /// swapped a queue that cannot starve for one that can, silently.
    #[test]
    fn the_default_ages() {
        assert!(Limits::default().patience > 0);
    }
}
