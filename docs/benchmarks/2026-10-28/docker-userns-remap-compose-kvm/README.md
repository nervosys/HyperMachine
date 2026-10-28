# A compose project under `--userns-remap`, across a checkpoint, a restore and a fork

`tools/check-docker-sandbox-kvm.py --network` (19 cases), 3 runs in a row, all 19 of 19
(`summary.txt`). WSL2 Debian, KVM, Hyper-V-nested, commit `a11b2c55` plus this change.
The `--userns-remap` case now goes on after the uid-map check: with the daemon remapped
(`dockremap`, uid 100000), it brings up the two-service compose project (a server and a
client that fetches from it into a named volume), waits for the client's file to hold
`hello-from-compose`, checkpoints, runs `docker compose down`, restores, and requires
both containers running again with the volume's data; then forks and requires the same
in the fork. The detail of each run is in `runN.json`.

## Failed attempts, kept

Before these three runs, the first version of the new stage failed **3 of 3 runs** and one
more diagnostic run (4 attempts), all in the same place and for the same reason: the check
read the volume with `docker compose exec client cat`, and the `hvbox` image has only
`/bin/busybox`, so `cat` was "not found" (the diagnostic run showed both containers up).
That was a mistake in the check, not in Docker or the remap. Those four logs were not
copied into this directory.

## Not verified

- A build (`docker compose build`) under the remap, across a checkpoint (the compose
  project here uses an imported image).
- Bind mounts under the remap, and a published port under it.
- Data written while the checkpoint is taken.
