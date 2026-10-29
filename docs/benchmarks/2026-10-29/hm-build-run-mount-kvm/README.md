# `RUN --mount` in `hm sandbox vm build` (KVM)

`tools/check-hm-build-kvm.py` gained an eleventh case; an owned `hv2-sandboxd` with a guest kit, no
Docker. **3 of 3 runs passed all 11 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The Dockerfile uses `type=bind,source=src,target=/mnt/in` (a file from the context is read inside the
RUN), `type=tmpfs,target=/scratch` and `type=cache,target=/cache` (each written to inside the RUN).
In the built template the results the RUNs copied out are there, and `/mnt/in`, `/scratch` and
`/cache` no longer exist. A RUN that exits 5 inside a mount fails the build at its step, and
`type=secret` is refused by name. The cache type prints a warning.

How it works: nothing is mounted. A bind source is copied to the target first (a `COPY` step, or from
a stage with `from=`), tmpfs and cache targets are made if missing, the command runs in a group, and
the targets are removed afterwards with the command's exit status kept.

## Not verified

- A bind target that already exists in the image: it is removed after the RUN (data loss in the
  layer). Only targets the check created were tried.
- A cache that persists between builds (not provided), `type=bind,from=<stage>` (unit-tested only),
  `rw` (writes are discarded with the target), `size=`/`mode=` options (ignored).
- `type=secret`, `type=ssh` and `--network` are refused, not implemented.
