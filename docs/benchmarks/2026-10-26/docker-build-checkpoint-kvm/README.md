# A checkpoint in the middle of `docker build`, with a compose project running (2026-10-26)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, as root, the setup
in [2026-10-23](../../2026-10-23/docker-sandbox-kvm/README.md). All 17 cases
pass in 3 runs in a row (`runs/run1..3.json`; `report.json` is the last). The
new case is the 16th:

- A two-container compose project was up, and a `docker build` was started whose
  `RUN sleep 30` step had begun (checked from the build's log, with no image
  yet). The sandbox was checkpointed there. The build was then let finish, and
  its image removed and the project taken down, so that only a restore could
  bring the unfinished build back.
- After the restore the image was absent (the checkpoint's state), then the
  build **finished by itself** and the image, run with `--network none`, printed
  `built-after-resume`. The compose project's two services were back.

This Docker's `docker build` is the legacy builder (no `--progress`), which the
first attempt of the case tripped on.

NOT VERIFIED: a BuildKit build (`docker compose build`) checkpointed mid-step;
a build that is pulling layers when it is checkpointed; a fork (not a restore)
of a sandbox mid-build; the default kernel; a published port reached from
outside the node.
