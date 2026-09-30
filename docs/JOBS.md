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

## What is verified

**`tools/e2e-jobs.sh`** passes 18 of 18 checks on Windows (Git Bash) and on Linux as an
unprivileged user:
- A job runs; its stdout, stderr and `HM_JOB_ID` are kept.
- A failing job records its exit code.
- A `gpu` job waits until a worker offering `gpu` takes it.
- A graceful stop creates its file, the program exits by itself, and the job is `cancelled`.
- A worker killed mid-job loses it, and another worker finishes it as attempt 2.
- The REST mirror submits, lists, cancels, streams logs, and refuses requests without its
  token.

**`hv2-jobs`** has 22 unit tests, including:
- the claim race and the cancel/claim race, run 15 times each on Windows without a failure;
- lease loss and a slow worker losing its lease;
- labels, and spec validation;
- graceful stops, both honoured and ignored;
- a worker pool draining a queue;
- IDs that try to reach outside the store.
