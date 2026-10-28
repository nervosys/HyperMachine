# Forks of a busy sandbox, no Docker

`tools/check-fork-busy-kvm.py`: a sandbox (2 vCPU, 2048 MiB, the Docker kernel and
initrd, no network) forked 40 times in a row; each fork must answer `echo alive` within
30 s and is then deleted. "Busy" parent: two CPU spin loops and a loop rewriting
64 MiB of `/dev/shm`. "Idle" parent: the control. WSL2 Debian, KVM, Hyper-V-nested,
commit `69faff0e` plus this tool. Why: run 11 of the 20-run Docker chase
(`../docker-flake-chase/`) restored a mid-build fork that never answered on vsock.

## Result

- Six repeats of busy + idle: 480 of 480 forks answered (240 busy, 240 idle), each
  within 0.2 s. Every report is kept (`run1` to `run6`, `summary.txt`).
- Two earlier runs of the same tool (logs not kept, outputs overwritten by the next
  run): busy 40 of 40, and idle 40 of 40. Total: 560 of 560 forks.
- **The restored-fork failure from run 11 did not reproduce**: 0 in 560 forks of a
  busy or idle parent without Docker.

## One failure that is not a fork

In the second of the two earlier runs, the idle pass died before its first fork: the
first command on a **freshly booted** sandbox returned `503 guest never became
ready: Timeout: no answer from a guest agent on vsock port 1024 within 15s`. The
daemon's dump in that error (quoted from the console; the log file itself was
overwritten and is NOT kept) said: both vCPUs `run_state=Halted` at the same RIP, `LVT_TIMER`
in TSC-deadline mode with a deadline set, 0 exits in the first 0.5 s and 0 and 1 in all, `EINTR=1`.
So a cold boot stalled with both CPUs halted and a timer armed. It did not recur in
the 12 cold boots of the six repeats above (1 failure in 14 observed boots).

## Not verified

- Whether run 11's restored fork and this cold-boot stall are the same fault. Both
  show almost no vCPU exits; that is the only link, and it is a guess.
- The cause of either. The next step would be to catch one with the daemon log and the
  KVM per-VM counters sampled during the stall (it is rare, so this needs a long loop).
- Forks with Docker running a build; that is `../docker-flake-chase/` (1 in 20).
- Other hosts.
