# `hm sandbox vm run -v` (KVM)

`tools/check-hm-run-kvm.py` gained a case; an owned `hv2-sandboxd` with a guest kit, no Docker.
The check now has 9 cases. **3 of 3 runs passed all 9** (`report-run1..3.json`, `run1..3.log`);
no failed runs.

What the case checks:

- `run -v runvol:/data alpine sh -c 'echo persisted > /data/f'` creates the volume `runvol` (it is
  listed by `GET /volumes` afterwards).
- A second run with `-v runvol:/mnt/elsewhere` reads `persisted` from `/mnt/elsewhere/f`, so the volume
  outlives the run and may be mounted at another path.
- A run without `-v` does not see `/data/f`.
- `-v /srv/data:/data` is refused with the volume-name error before anything is sent.

## Not verified

- Several `-v` in one run, and a volume shared by two sandboxes at the same time.
- A volume written by an image's non-root `USER` (ownership of the mount point).
- `ro` suffixes (`NAME:/path:ro` is refused as a path), and host directories (refused by design).
- A detached run (`-p`) with `-v` (the same code, no case).
- `--network` and `-it` are still not done.
