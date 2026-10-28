# Docker check, 20 runs in a row, every log kept

`tools/check-docker-sandbox-kvm.py` (18 cases) run 20 times back to back on the
WSL2 Debian host (KVM, Hyper-V-nested), commit `d647ea50`. `summary.txt` has one
line per run.

**Result: 19 of 20 runs passed all 18 cases. Run 11 failed (16 of 18 ok).**

## The failure (run 11)

- Case: `a fork in the middle of a docker build finishes the build in the fork and the parent`.
- What the daemon log shows (`run11.daemon.log`, lines near the end): the fork was
  snapshotted and restored (`restored from ...fork-695dee2e....snap`), the restored VM
  started 2 vCPU tasks, and 30 s later `guest never became ready: Timeout: no answer
  from a guest agent on vsock port 1024 within 15s`. The restored VM made only 2 and
  3 exits in that time; the parent made hundreds.
- The checker then died with `KeyError: 'sandbox'` (the fork call returned an error
  object, not a sandbox), so the traceback in `run11.log` is a symptom, not the cause.
- It is **not** the published-port failure seen earlier (that case passed in all 20).

## What is known and not known

- 1 failure in 20 runs of this case here; the earlier 10-run fork evidence
  (`2026-10-26/docker-build-fork-kvm`) had 10 of 10 passing. Together: 1 in 30.
- **Not known:** why the restored vCPUs ran so little. A restore that does not resume
  execution, only under load from a mid-build snapshot, is the leading description,
  and it is a description, not a cause.
- **Not verified:** whether it happens without Docker (a plain fork of a busy sandbox),
  on another host, or on a bare-metal kernel.
- Run 11's report (`run11.json`) lists the 16 cases that passed before the crash.
