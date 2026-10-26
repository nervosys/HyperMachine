# A fork in the middle of `docker build` (2026-10-26)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, as root, the setup
in [2026-10-23](../../2026-10-23/docker-sandbox-kvm/README.md). All 18 cases
pass in the 3 runs kept here (`runs/run1..3.json`, with each run's log and the
daemon's log; `report.json` is the last). The new case is the 17th:

- A `docker build` was started whose `RUN sleep 30` step had begun (checked from
  the build's log, with no image yet). The sandbox was forked there. In the fork
  and in the parent the build **finished by itself** and the image, run with
  `--network none`, printed `built-after-resume` in both.

**One failed run, not counted above.** Of the 10 runs made with this version of
the script, one failed: the existing case "a published port reaches the
container, from the guest and through the node proxy" (the 6th case, 36 s into
the run) did not pass, and the other 9 runs passed all 18 (3 more runs, with logs kept, were clean). The report records no
error text and the run's log was overwritten, so the cause is **unknown**: it
is not in the new case, which had not started. The 3 runs kept here are the first 3
made after the runner was changed to keep each run's log (the 3 later clean runs are not kept). They are
not a claim that the failure cannot recur.

NOT VERIFIED: why that run failed; a BuildKit build (`docker compose build`)
forked or checkpointed mid-step; a build that is pulling layers when forked.
