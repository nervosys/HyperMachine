# A command running at a pause, in the command history, on KVM (2026-10-18)

**What this checks:** what the history says about a command that is still
running when its sandbox pauses. [`COMMAND_HISTORY.md`](../../../COMMAND_HISTORY.md)
said it "is kept as ended there"; the code did not do that.

## Method

`tools/check-sandbox-history-kvm.py` gained a case. In a sandbox on a real
guest it starts two commands that outlast the pause: `echo process-started;
sleep 120` through the E2B SDK (envd) and `echo exec-started; sleep 120`
through `POST /exec`. When both read `running` (and the envd one has printed),
it pauses the sandbox, reads both back, resumes, waits 3 s for a late
completion, and reads them again.

## Result

- **Before** (the binary without the fix, `runs/before.json`): the case failed.
  Both commands stayed `state: running`, `endedAt: null`, after the pause.
  The 6 older cases before it passed.
- **After** (`runs/after-1..3.json`): all 10 cases passed on 3 runs in a row.
  At the pause both commands read `lost`, with `endedAt` set and
  `error: "the sandbox paused while it ran"`; the envd command kept
  `process-started\n` and the exec command had no output (an exec command's
  output is kept when it ends). After the resume and the wait they were
  unchanged.

The fix is `history::pausing`, called by `pause_sandbox` before the history is
copied into the store: it ends each running command as `lost`. The unit test
`a_command_running_at_a_pause_is_ended_and_stays_so` covers it without a guest.

## Not verified

- A command running at a pause that resumes on **another node** through the
  store: the pause code that ends it runs before the copy, so the store holds
  it ended, but the check resumed on another node only in the older case,
  which has no command running.
- A command that finishes in the guest *during* the pause window, between the
  last poll and the suspend: it is recorded `lost` although it exited.
- Whether the guest process survives the pause and resume (the docs say it
  does; this check did not look).
- `hm sandbox vm history` and the SDKs against this build.
