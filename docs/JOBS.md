# Jobs: a durable queue of sandboxed host work

`hm jobs` queues programs and runs them on workers. Each job runs under the process sandbox,
with the same limits, network and filesystem policy as `hm sandbox run` (see
[SANDBOXES.md](SANDBOXES.md)). There is no VM, no daemon and no database. The queue is a
directory, and every client and worker that names the same directory shares it, across
processes, users and restarts.

```
hm jobs [--store DIR] submit SPEC.json|-        # prints the job's ID
hm jobs [--store DIR] list [--state S] [--json]
hm jobs [--store DIR] status ID [--json]         # --json adds the spec
hm jobs [--store DIR] logs ID [--follow] [--stderr]
hm jobs [--store DIR] cancel ID
hm jobs [--store DIR] worker [--labels gpu,cpu] [--concurrency N] [--lease-secs 30]
hm jobs [--store DIR] serve [--addr 127.0.0.1:7878] [--token T]   # REST, below
```

The store is `--store DIR`, then `$HM_JOBS_DIR`, then `~/.hypermachine/jobs`.

## A job spec

```json
{
  "name": "arena",
  "command": ["awake", "arena-loop", "--dir", "run", "--until", "70"],
  "workdir": "/work/awake",
  "env": {"RUST_LOG": "info"},
  "sandbox": {"memory": "24G", "cpu_time_secs": null, "wall_clock_secs": null,
              "max_processes": 256, "net": "deny", "fs": "host", "ro": [],
              "isolate_processes": false, "no_new_privileges": false, "strict": false},
  "labels": ["gpu"],
  "max_attempts": 3,
  "graceful_stop": {"create_file": "run/STOP", "grace_secs": 900}
}
```

**Fields:**
- **`command`** runs directly, not through a shell.
- **`not_before_ms`** optionally sets the earliest start as an unsigned Unix epoch
  timestamp in milliseconds, through both `hm jobs submit` and the REST API.
  Omit it for immediate eligibility. The timestamp is stored with the job and
  survives worker/client restarts; future jobs remain queued without using an
  attempt or blocking eligible jobs. Workers use their host wall clock and poll
  once per second by default, so this is an earliest start, not a deadline.
  Cancellation works before the start time. Synchronize clocks across worker
  hosts. This schedules a single host process; recurring schedules and VM jobs
  are not implemented.
- **`env`** is added over a minimal base (`PATH`, `HOME`, `TEMP` and, on Windows,
  `SystemRoot`). Nothing else crosses from the worker's own environment.
- **`HM_JOB_ID`** is always set, so a program can record which job produced its output.
- **`labels`**: only a worker that offers every label takes the job.
- **`max_attempts`** counts starts, including restarts after a worker is lost. It is 1 by
  default, so a program that is not safe to run twice is not run twice unless the spec says so.
- **`graceful_stop`**:
  - On `cancel`, the worker creates `create_file` (relative paths resolve against `workdir`).
  - The program then has `grace_secs` to exit by itself before the whole tree is killed.
  - A graceful stop that ends in exit 0 is still recorded as `cancelled`.
- Unknown fields are refused, so a misspelled limit is an error rather than a job that runs
  without it.

## States

A job is `queued`, then `running`, then one of three final states:

| State | When |
|---|---|
| `succeeded` | It exited 0. |
| `failed` | Any other exit, a limit kill, or a program that could not start. |
| `cancelled` | A cancel was asked for, whatever the exit was. |

`status` records:
- **attempts, worker and times:** the attempt number, the worker (`name#attempt`), and when it
  started, last renewed its lease and finished;
- **how it ended:** the exit code or signal, and the limit that killed it;
- **what it ran without:** controls dropped because the host could not enforce them;
- **`message`:** why a job failed without running to an exit, or why it was requeued.

## How it stays correct

Every decision is an **exclusive file creation**: starting attempt N, cancelling before attempt
N, and requeueing a lost attempt N. A file create with `create_new` (`O_EXCL`, `CREATE_NEW`)
succeeds for exactly one of any number of racing processes.

A rename is not used, because it does not work for this on Windows. Rust's `rename` there works
through a handle to the source, so two workers that both opened `queue/<id>` before either
moved it both "succeed". A test with eight workers racing for twenty jobs claimed them 64
times before this was changed. It now passes on Windows and Linux, and so does a cancel racing
a claim.

**Leases.** A running job's worker renews its lease every 5 seconds. A job whose lease is older
than `--lease-secs` (at least 15) has lost its worker, and the next store operation that
notices requeues it, or fails it once `max_attempts` is spent.

The lease names the attempt. A worker that was only slow, and finds its job requeued, kills
its own run rather than running alongside its successor.

On Windows, the sandbox's job object has kill-on-close, so a worker that dies takes its job's
processes with it. On Linux, the process group is killed by the deadline, a cancel or a lost
lease.

**State files.** Every state change is a whole file renamed into place, so a reader never sees
half of one.

## REST

`hm jobs serve` mirrors the CLI under `/api/v1/jobs`:

| Request | Result |
|---|---|
| `POST /api/v1/jobs` with a spec | `201 {"id": ...}` |
| `GET /api/v1/jobs[?state=running]` | the job states, oldest first |
| `GET /api/v1/jobs/{id}` | `{"state": ..., "spec": ...}` |
| `GET /api/v1/jobs/{id}/logs?stream=stdout\|stderr[&follow=true]` | the log as text; with `follow`, streamed until the job ends |
| `POST /api/v1/jobs/{id}/cancel` | `202` with the job's state |

Submitting a job runs a program on the workers' hosts, so the endpoint is remote execution.
With `--token` (or `HM_JOBS_TOKEN`), every request must carry
`Authorization: Bearer <token>`, compared in constant time. `serve` refuses to listen anywhere
but loopback without a token.

## VM scheduling implementation requirements

Interval publication is available through the CLI:

```text
hm jobs --store DIR schedule create NAME interval.json
hm jobs --store DIR schedule list --limit 100
hm jobs --store DIR schedule status NAME
hm jobs --store DIR schedule publish NAME --limit 100
hm jobs --store DIR schedule watch NAME --limit 100 --poll-ms 1000
hm jobs --store DIR schedule cancel NAME
hm jobs --store DIR schedule occurrences NAME --after-ms 1234567890000 --limit 100
```

An interval spec contains `first_ms`, `every_ms`, optional `missed_policy`
(`catch_up` or `coalesce`), and `job` containing a job spec. `create` also accepts
`-` for stdin. These commands print JSON. `publish` defaults to the current wall
clock; `--now-ms` supplies an explicit Unix millisecond horizon for replay or
testing. Limits must be 1-1024. Names are immutable: duplicate creation fails.
Publication stores occurrence records and does not enqueue or execute jobs.
The existing `hm jobs worker` does not consume these records.

`schedule watch` publishes due occurrences repeatedly using the host wall clock,
with one bounded batch per tick and compact JSON output per successful tick.
It reloads progress on each tick, retries competing-writer conflicts, and
preserves progress across invocations. `--ticks N` stops after N positive ticks;
otherwise Ctrl+C stops the loop after any accepted publication operation finishes.
Polling must be 1-60000 milliseconds. Output is a publication receipt, not a job
result. The loop still does not execute commands or schedule VM work.

`schedule cancel` permanently stops future committed publication for that name.
Cancellation competes atomically with a batch commit; a batch that wins first
stays in history. Already committed records remain readable. Repeated
cancellation succeeds, status includes `cancelled`, and `watch` exits on observing
cancellation. This does not cancel guest work. Names cannot be reactivated or
reused. Interrupted publishers may leave uncommitted records, which committed
occurrence reads exclude. Under sustained competing commits, cancellation can
return a conflict after 32 attempts and must be retried.

The same publication operations are served by `hm jobs serve`, with its existing
bearer-token requirement applied to every schedule route:

| Request | Result |
|---|---|
| `POST /api/v1/schedules/{id}` with an interval spec | `201 {"id": ...}`; duplicate IDs return `409` |
| `GET /api/v1/schedules?after=NAME&limit=100` | Schedule names in lexical order after an exclusive name cursor |
| `GET /api/v1/schedules/{id}` | Schedule and `publication_through_ms` |
| `POST /api/v1/schedules/{id}/publish` with `{"limit":100}` | One bounded batch of occurrence records; optional `now_ms` overrides the wall clock |
| `GET /api/v1/schedules/{id}/occurrences?after_ms=...&limit=100` | A page of committed records after an exclusive cursor |
| `POST /api/v1/schedules/{id}/cancel` | `200 {"id": ..., "cancelled": true}`; preserves history |

Publication and page limits must be 1-1024. Unknown publish fields are refused.
Listing includes cancelled schedules, excludes interrupted publication temporary
files, and scans the schedule directory while retaining at most one page of names
in memory. The CLI takes the same exclusive cursor as `schedule list --after NAME`.
Pages are not a frozen snapshot during concurrent creation; refresh from the
beginning to discover names inserted before a previous cursor.
Schedule filesystem operations run in blocking tasks. These routes provide no
job execution or schedule update yet. Automatic publication
currently runs through `schedule watch`.

VM scheduling remains unimplemented. Inspection on 2026-10-01 found that
`hv2-jobs/src/worker.rs::run_job` launches a local `ProcessSandbox`; its
cancellation and lease-loss watcher controls that local process. Launching
`hm sandbox vm exec` from this worker would not transfer those guarantees to
the guest. The daemon's `/exec` response contains output, exit status and a
timeout flag, but no process handle for later reconciliation.

The existing `hv2-api/src/envd_process.rs` service offers guest process start,
connection, PID/tag selection and SIGTERM/SIGKILL. Its running-process registry
is host memory and removes processes after exit. A scheduler must therefore
add durable reconciliation and completion records rather than treating this
registry as persistent job state.

The `hv2-jobs::schedule` module now provides immutable interval schedules and
stable occurrence records keyed by schedule ID and scheduled Unix milliseconds.
`Store::create_interval_schedule`, `interval_schedule` and
`record_interval_occurrence` are library APIs; the CLI supports explicit bounded
publication and an automatic publication loop, but no worker dispatch consumes them yet.
Interval arithmetic stays anchored to the first
timestamp and checks overflow. An occurrence stores the job configuration with
its earliest start set to that occurrence's time.

The persisted `missed_policy` is `catch_up` by default for existing records, or
`coalesce`. `IntervalSchedule::due_occurrences(after_ms, now_ms, limit)` plans
at most 1-1024 occurrences after an exclusive processed-through watermark.
Catch-up selects the oldest due occurrences first; coalescing selects only the
latest due occurrence and deliberately skips older ones. Future occurrences
are excluded. The caller must persist the work represented by its watermark before
advancing its watermark. Planning alone does not persist progress or dispatch
work, and repeated planning can return the same occurrences for reconciliation.

`Store::interval_progress` recovers a separate occurrence-publication watermark.
`advance_interval_progress(id, expected, through_ms)` commits progress only when
all selected records exist and match the immutable schedule. Catch-up commits
cannot skip required records and are limited to 1024 occurrences; coalescing
requires its latest selected record. An immutable chain of exclusive commits
prevents competing writers from replacing a winner. Stale writers receive a
conflict and must reload progress. Reads currently traverse the entire chain;
compaction and long-running schedule scalability remain unverified. This
watermark acknowledges record publication, not guest dispatch or completion.

`Store::materialize_interval(id, now_ms, limit)` combines planning, immutable
record publication and progress commit for one bounded batch. It reuses records
left by an interrupted publication. A concurrent progress conflict requires
reloading and retrying; records from a failed batch must not be treated as
dispatched jobs. A successful commit followed by process loss also requires a
future dispatcher to recover from persistent records, rather than relying on
the returned in-memory batch. This operation does not enqueue or execute jobs.

`Store::committed_interval_occurrences(id, cursor, limit)` reads a bounded page
from the committed chain, using an exclusive scheduled-time cursor. It excludes
uncommitted records and older records skipped by a committed coalescing step,
and validates returned records against the schedule. A dispatcher can recover
these records after losing an in-memory batch. This read neither claims work
nor acknowledges dispatch; a durable dispatcher and execution receipts are
still required. Page reads also traverse the chain from its beginning.

Publication writes and syncs a temporary file before creating an exclusive hard
link to its final name. Competing publishers cannot replace the winner or expose
partial JSON. A crash before publication can leave an unreferenced temporary
file. Filesystems without hard-link support return an error; there is no weaker
fallback. Directory durability across power loss is not established. Schedule
updates, cron/timezones, dispatch reconciliation and guest-job cancellation
and guest execution remain to be implemented.

The implementation must cover these requirements together:

| Area | Required behavior and verification |
|---|---|
| VM execution | Bind each job to a sandbox ID and authenticated operator-configured connection profile; resume a paused VM and verify guest execution. Keep connection secrets outside submitted specs and job logs. Reject host-only limits on VM jobs rather than silently ignoring them. |
| Recurrence | Persist interval and cron schedules, timezone and next occurrence. Define missed-occurrence and overlap policy explicitly. Test clock boundaries, daylight-saving transitions, restart and competing scheduler processes. |
| Occurrence identity | Give each scheduled occurrence a stable ID derived from schedule ID and scheduled time. Publish it atomically so racing schedulers and restart recovery cannot enqueue duplicates. Cancellation prevents future occurrences without erasing past results. |
| Guest reconciliation | Persist the occurrence/attempt identity before dispatch. Recover an interrupted start without blindly launching a second guest command. Retain terminal result and bounded logs across guest completion and daemon restart. A PID alone cannot prove identity after PID reuse. |
| Leases and cancellation | Fence obsolete attempts before permitting a replacement. Confirm termination of the guest process tree on cancellation or timeout. A dropped HTTP connection or killed local client does not establish guest termination. Test worker loss during dispatch, execution and result recording. |
| Retry semantics | Expose at-least-once behavior and stable occurrence identity to jobs. Retry an uncertain execution only under the declared policy; do not promise exactly-once external side effects. Preserve attempt history and distinguish dispatch failure, guest failure and unknown outcome. |
| Interfaces and evidence | Ship submit/list/status/logs/cancel and schedule create/list/update/delete through CLI and authenticated API. Verify recurring jobs on real KVM through the control plane, paused-VM wake, restart recovery, competing workers, no overlap under the selected policy, and complete cleanup. |

The scheduling feature remains **Partial** in `PLATFORM_PARITY.md` until this
behavior is implemented and checked. Existing delayed host jobs do not satisfy
these acceptance criteria.

## Existing host-job verification

`cargo test -p hm-cli --test jobs_schedule` passes on Windows and Linux.
It invokes the shipped binary in separate processes to verify schedule creation,
duplicate rejection, bounded publication, persisted status, occurrence pages,
invalid-limit/path rejection, and an empty runnable job queue.
The automatic-publication test verifies two bounded ticks and a second
invocation continuing from persistent progress on both Windows and Linux.
The suite has three passing CLI tests on Windows and four on Linux. A live
publisher test cancels the schedule from a second process, waits for a successful
exit within five seconds, and checks committed records afterward. Linux also
tests SIGINT delivered to the running publisher, a successful bounded exit and
preserved records. These tests wait for a published batch before shutdown; they
do not force interruption at every filesystem instruction. Windows console
Ctrl+C delivery remains untested.

**`tools/e2e-jobs.sh`** passes 18 of 18 checks on Windows (Git Bash) and on Linux as an
unprivileged user:
- A job runs; its stdout, stderr and `HM_JOB_ID` are kept.
- A failing job records its exit code.
- A `gpu` job waits until a worker offering `gpu` takes it.
- A graceful stop creates its file, the program exits by itself, and the job is `cancelled`.
- A worker killed mid-job loses it, and another worker finishes it as attempt 2.
- The REST mirror submits, lists, cancels, streams logs, and refuses requests without its
  token.

**`hv2-jobs`** has 36 unit tests, passing on Windows and Linux, including:
- interval boundary/overflow checks and competing schedule/occurrence publishers,
  with immutable records preserved after reopening the store;
- bounded missed-occurrence batches, coalescing, restart planning with an
  exclusive watermark, backward clock movement and timestamp exhaustion;
- progress commits gated on complete occurrence records, restart recovery and
  one winning commit among eight concurrent writers;
- interrupted batch publication reconciled after reopening, followed by
  bounded continuation without republishing prior occurrence times;
- committed occurrence pages recovered after reopening, including exclusion
  of uncommitted records and records skipped under coalescing;
- schedule API authentication on every route, duplicate conflicts, bounded
  publication and pages, invalid input rejection and no runnable-job creation;
- durable schedule cancellation, preserved history, cancellation/publication
  races, and authenticated rejection of publication after cancellation;
- schedule discovery pagination, temporary-file exclusion and cancelled-name
  retention, plus authenticated API and CLI list operations;
- the claim race and the cancel/claim race, run 15 times each on Windows without a failure;
- lease loss and a slow worker losing its lease;
- labels, and spec validation;
- graceful stops, both honoured and ignored;
- a worker pool draining a queue;
- IDs that try to reach outside the store.
