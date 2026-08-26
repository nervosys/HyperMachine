# Context as an environment

An agent's history is normally kept by putting it back in the prompt. That has
two costs. Every past turn competes with the current one for the same budget,
and — the expensive one — the decision about what matters has to be made at
*write* time. When a tool returns 40 MB, something has to choose what to keep
before anyone knows what the next question will be, and whatever it discards is
gone.

`hv2-context` implements the alternative from Scroll
([arXiv:2608.21690](https://arxiv.org/abs/2608.21690)): keep the history outside
the context, as something the agent queries. Nothing is summarised away at write
time. What the agent sees next is chosen at read time, by the agent, from a
record that stayed complete.

## The invariant

**Eviction changes the view and never the record.**

Everything else follows from that. Anything that leaves the working view is
still in the log, at the same address, byte for byte what it was. There is no
operation anywhere in the crate that edits or deletes an event — not
"discouraged", not offered.

This is why a *headline* is not a summary. A summary replaces what it describes:
once the turns are gone it is all there is, and every detail it dropped is
unrecoverable. A headline sits beside what it describes and carries its address,
so it is a table of contents. Thin or wrong is survivable; the events it points
at are still exact.

## The three layers

| Layer | Type | What it holds |
|---|---|---|
| Event log | `EventLog` | Append-only ground truth. Every event has a `Seq` — an address that never changes and never repeats. |
| Payload store | `PayloadStore` | Payloads over 8 KiB, held outside the log behind a handle so a scan stays cheap. The log keeps a 240-byte preview. |
| Confined runtime | `ContextRuntime` | Somewhere to compute over what was retrieved, so the answer comes back instead of the data. |

`WorkingView` is what the model actually sees, and it is bounded. `SessionEnvironment` ties
the three together and is what a caller uses.

## The four operations

Everything an agent does with its own history is one of these:

- **locate** — `search(query, k, filter)`, BM25-ranked, with session/kind/role/time
  filters. Returns addresses and previews, never content: a search that returned
  payloads would fill the context with the thing it was asked to find a way
  around.
- **materialize** — `expand(from, to)` returns exactly what was recorded,
  externalized payloads included.
- **compute** — `exec(call)` runs a program under `hv2-sandbox` and returns only
  what it printed.
- **expose** — `observe` puts something in the view; `compact` takes it out
  again when the budget says so.

## What `compact` does, in order

Each step is cheaper than the next is destructive:

1. **Persist.** Every unpersisted entry goes to the log first. Nothing can be
   evicted before it is addressable — this ordering is the whole safety
   argument, so `evict` does it itself rather than trusting the caller to have.
2. **Protect.** The active turn, the last `tail` entries, and the newest tool
   result if it is still in the recent half of the view.
3. **Fold.** Unprotected tool payloads are replaced by their addresses. A folded
   entry keeps its place in the order and says where its content went, which is
   strictly less lossy than removing it.
4. **Evict.** Only if folding was not enough: the oldest unprotected span
   leaves, and the headline the caller supplies goes into the eviction index.

If the view still does not fit, `within_budget` comes back `false`. That is a
real outcome, not a bug — a view whose protected tail alone exceeds the budget
cannot be brought under it, and saying so beats evicting the tail and pretending.

### Why the newest tool result is only *conditionally* protected

Protecting it unconditionally means one tool result at the head of a long
session pins every entry behind it, and eviction silently stops doing anything.
A result in the older half of the view is not what the session is working on any
more, so protection lapses.

## The eviction index

One flat list of headlines grows without limit, which puts the index on the same
path as the context it was meant to relieve. So it is tiered: each tier holds at
most `k` blocks, and when one overflows, all but its newest block collapse to a
line apiece and merge upwards. After `n` evictions that is `O(k log_k n)` blocks
rather than `O(n)`.

Recent history keeps its detail. Distant history coarsens. **Every entry at
every tier still carries its `Seq` span**, so nothing stops being reachable — it
only stops being described at length.

## MCP tools

| Tool | Capability | What it does |
|---|---|---|
| `context.search` | `ContextMemory` | Find addresses, ranked. |
| `context.expand` | `ContextMemory` | Read a span back exactly. `into_view: false` reads without spending context. |
| `context.record` | `ContextMemory` | Append without showing. |
| `context.exec` | `ContextMemory` + `HostExec` | Compute over what was retrieved, confined. |
| `context.compact` | `ContextMemory` | Persist, fold, evict; leave a headline. |
| `context.view` | `ContextMemory` | The view, with the index of what has left it. |
| `context.status` | `ContextMemory` | Size of the record, cost of the view, whether a runtime exists. |

`context.exec` needs `HostExec` **as well as** `ContextMemory`: being able to
read the record is not a reason to be able to run code on the machine holding
it. `ContextMemory` is itself gated because a shared log holds everything every
session on this machine has recorded.

With no host installed, all seven refuse. There is no in-memory fallback on
purpose — a record that accepts every write and loses it is worse than none,
because the agent is told it succeeded.

## The workflow this is for

```
context.search  "the vsock refusal"        -> #412, #1180  (addresses, ~240 bytes each)
context.expand  from=1180 into_view=false  -> the whole 4 MB result, not in the view
context.exec    grep -c FAILED  <stdin>    -> "58"
context.record  role=tool kind=finding     -> #1181, kept whole, searchable whole
context.compact task=... state=... status= -> 30 entries evicted, index entry left behind
```

Every number in the middle column is an address, and every address still works.

## What is *not* built

The runtime is a **confined process with a durable workspace**, not a resident
namespace. Scroll keeps a Python kernel alive across model calls so a tool
result stays a live object a later cell can operate on. Here every call is a
fresh process: **files persist, variables do not**.

That decides how an agent should use it — state that must survive goes to the
workspace or the log, and everything else is gone when the process exits.
`ContextRuntime` is a trait so a resident-kernel backend can be added behind it
without changing anything above.

Two smaller ones, said plainly for the same reason:

- **The log is flushed, not fsynced.** A power loss can lose the tail. "Durable
  append-only record" invites the assumption that it cannot, and a log that
  quietly loses its last few entries is worse than one that says it might.
- **The event log is not reachable from inside the sandbox** — not by policy,
  by not being there. The runtime is handed its workspace and nothing else, and
  the memory surface is mediated outside. Exposing the log to arbitrary code
  inside would need a filesystem policy the process backend does not implement,
  and a control that is not enforced must not be described as one.

## Confinement on Windows

`context.exec` defaults to no network and no new privileges. A Windows job
object can enforce neither, so the call is **refused** rather than quietly
downgraded — the same rule the rest of `hv2-sandbox` follows. Pass
`best_effort: true` to run anyway, and read `unenforced` in the result for
exactly what was given up.
