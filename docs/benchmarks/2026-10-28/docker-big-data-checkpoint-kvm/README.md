# Bigger data across a checkpoint and a fork: a 128 MiB volume and a 64 MiB bind mount

`tools/check-docker-sandbox-kvm.py --network` (19 cases), 3 runs in a row, all 19 of 19
(`summary.txt`). WSL2 Debian, KVM, Hyper-V-nested, commit `a11b2c55` plus the new case.

The new case writes 128 MiB of random data into a named volume and 64 MiB into a
bind-mounted host directory (`/srv/bind`) from a container, records both files'
sha256 in the guest, checkpoints, overwrites both with new random data (the digests
differ), restores, and requires the original digests; then forks and requires the same
digests in the fork. Each run's digests are in its `runN.json`.

## Not verified

- Anything over 128 MiB, or many files (these are single files); a volume driver other
  than `local`.
- Data written *while* the checkpoint is taken (the writes were synced first).
- A volume on a separate disk (`--disk-slot` with a real data disk); both paths here are
  on the sandbox's root filesystem.
