# `FROM <earlier stage>` in `hm sandbox vm build` (KVM)

`tools/check-hm-multistage-kvm.py`: an owned `hv2-sandboxd` with a guest kit, no Docker. The
case "FROM an earlier stage is refused" was replaced by one that builds on a stage, and the
failing-`COPY --from` case lost its refusal half. **3 of 3 runs passed all 6 cases**
(`report-run1..3.json`, `run1..3.log`); no failed runs.

The new case builds `FROM alpine AS base` (writes `/base.txt`), `FROM base AS mid` (writes
`/mid.txt`), then `FROM mid` (`cat /base.txt /mid.txt > /top.txt`). The final template's
`/top.txt` holds both lines, so each stage was built on the one before it, and no
`-stage` template is left behind.

How it works: `hm` builds an earlier stage the later one names as a template and sends the
node `fromTemplate` instead of `fromImage` for the stage after it; the stage templates are
deleted at the end of the build.

## Not verified

- Whether the later stage inherits the earlier stage's `ENV`, `WORKDIR`, `USER`, `CMD` and
  `ENTRYPOINT` (the files carry over; the image config was not checked).
- A `FROM` that names both a stage and a real image (a stage wins, as in Docker; no test).
- The step cache across a stage-based build.
- Registry credentials for a stage-based build (not used for a template base).
