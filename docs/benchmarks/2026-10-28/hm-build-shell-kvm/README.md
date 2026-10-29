# `SHELL` in `hm sandbox vm build` (KVM)

`tools/check-hm-build-kvm.py` gained a seventh case; an owned `hv2-sandboxd` with a guest kit,
no Docker. **3 of 3 runs passed all 7 cases** (`report-run1..3.json`, `run1..3.log`); no failed
runs.

The case builds `FROM alpine:3.20` + `RUN false; touch /marker` twice. With the default shell
the build finishes and `/marker` exists in the template. With `SHELL ["/bin/sh", "-ec"]` first,
the build fails at the `RUN` step ("build failed at step") and makes no template.

Unit tests also cover `CMD`/`ENTRYPOINT` shell form and that exec-form `RUN` ignores `SHELL`.

## Not verified

- A shell other than `/bin/sh` (bash in an image that has it).
- `SHELL` across stages (each stage is parsed on its own, so it starts with the default, as in Docker; no test).
- `CMD`/`ENTRYPOINT` shell form under `SHELL` at run time (unit test only, no KVM run).
- `ONBUILD`, `ADD` of a URL or archive, heredocs and `RUN --mount` are still refused.
