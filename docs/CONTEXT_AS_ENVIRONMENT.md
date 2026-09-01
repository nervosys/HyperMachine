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
| Runtime | `ContextRuntime` | Somewhere to compute over what was retrieved, so the answer comes back instead of the data. Two backends: `SandboxRuntime` confines every call, `ResidentRuntime` keeps a namespace alive across them. |

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
- **compute** — `exec(call)` runs a program under `hv2-sandbox`, or code in a
  resident interpreter, and returns only what it printed.
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

## Two runtimes, and what each trades away

`ContextRuntime` has two backends. Neither supersedes the other; they trade
against each other, and the trade is the same one either way — **per-call
confinement or a namespace that survives, not both.**

| | `SandboxRuntime` | `ResidentRuntime` |
|---|---|---|
| Process | one per call | one, alive across calls |
| Survives a call | files in the workspace | files **and variables** |
| Confined | every call, under `hv2-sandbox`, with a spec the caller may change per call | once, at spawn — a running process cannot be re-confined |
| Runs | any program | Python code, in the namespace it already holds |

`ResidentRuntime` is the shape Scroll describes: a kernel stays up, so a result
computed once is still an object later.

```
exec  rows = [json.loads(l) for l in open("result.jsonl")]    -> (nothing printed)
exec  print(sum(1 for r in rows if r["status"] == "FAILED"))  -> 58
```

The second call re-read nothing and neither call put a row in the context.

**Protocol.** The framing this repository already uses between a host and
something it drives (see `hv2-guest-agent`): a four-byte little-endian length,
then that many bytes of JSON, over the child's stdin and stdout. Both ends
refuse a length past the 4 MiB frame cap *before* allocating for it, because
the length is written by the other end. The kernel sends one `ready` frame
saying which limits it managed to apply to itself, then one `result` frame per
call.

**What is confined, and what is not.** Only two controls survive the move to a
resident process:

| Control | Resident kernel | How |
|---|---|---|
| Wall clock | enforced, per call | The host kills the kernel when a call overruns its deadline. |
| Memory | enforced where the interpreter can | The kernel lowers its own `RLIMIT_AS`, soft *and* hard, before reading its first frame; an unprivileged process cannot raise a hard limit back. No `resource` module (Windows) means not enforced, and it says so. |
| Network, filesystem, process isolation, no-new-privileges | **not enforced** | Applying these means starting the process inside a namespace or a job object, and `Sandbox::run` runs a program *to completion* — there is no way through it to start a long-lived confined child. This backend starts the interpreter itself and gets none of them. |

That is not hidden anywhere. `ResidentRuntime::spawn` **refuses** unless the
confinement asked for can actually be applied — the default spec asks for what
`SandboxRuntime` asks for, so the default refuses on every host and the refusal
names what is missing. A caller who wants it anyway sets
`ResidentSpec::best_effort`, and every `RuntimeOutput` then carries the
given-up controls in `unenforced` — on every call, not once at startup where it
can be missed.

**When the deadline fires, the namespace dies.** A resident interpreter cannot
have one call interrupted without interrupting the interpreter. So the call
returns an error, every later call says the namespace is gone, and the runtime
does *not* quietly start a new kernel: a fresh empty namespace handed back to an
agent still holding names is a worse failure than the one it hides.

## What is *not* built

`ResidentRuntime` is the paper's shape, not all of it. What still differs:

- **The interpreter is Python, and only Python.** `SandboxRuntime` runs any
  program; the resident backend runs code in the namespace it holds, and a call
  naming another program is refused rather than silently interpreted.
- **The confinement gap above.** Scroll's kernel and this one both keep state;
  a one-shot sandboxed call is the only thing here that gets network and
  filesystem isolation. Choosing the resident backend is choosing to give those
  up, which is why it has to be said out loud rather than defaulted into.
- **The namespace is not durable.** It lives in one process. A deadline, a
  crash or a restart ends it, and nothing is checkpointed or restored — the log
  and the workspace are the only things that outlive it.
- **A host with no Python 3 has no resident runtime.** `ResidentRuntime::available()`
  answers that before a session plans around it, naming every interpreter it
  tried and what each one did. `python3` is *run* and asked its version rather
  than merely found on `PATH`, because on Windows that name is usually a Store
  stub that prints an advertisement.
- **No streaming.** A call's output arrives when the call finishes, as with the
  guest agent, and for the same reason: this is the right shape for "compute
  this and tell me the answer" and the wrong one for an interactive session.

Two more, said plainly for the same reason:

- **The log is flushed, not fsynced.** A power loss can lose the tail. "Durable
  append-only record" invites the assumption that it cannot, and a log that
  quietly loses its last few entries is worse than one that says it might.
- **The event log is not reachable from inside either runtime** — not by
  policy, by not being there. A runtime is handed its workspace and nothing
  else, and the memory surface is mediated outside; a retrieved result gets in
  through the workspace or through the code of a call, and neither backend can
  call `search` or `expand` itself. Scroll's kernel can. Exposing the log to
  arbitrary code inside would need a filesystem policy the process backend does
  not implement, and a control that is not enforced must not be described as
  one.

## Confinement on Windows

`context.exec` defaults to no network and no new privileges. A Windows job
object can enforce neither, so the call is **refused** rather than quietly
downgraded — the same rule the rest of `hv2-sandbox` follows. Pass
`best_effort: true` to run anyway, and read `unenforced` in the result for
exactly what was given up.
