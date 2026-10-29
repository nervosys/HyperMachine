# `FROM <earlier stage>` in `hm sandbox vm build` (KVM)

`tools/check-hm-multistage-kvm.py`: an owned `hv2-sandboxd` with a guest kit, no Docker. The
case "FROM an earlier stage is refused" was replaced by one that builds on a stage, the
failing-`COPY --from` case lost its refusal half, and a case for inherited config was added.
**3 of 3 runs passed all 7 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs. An
earlier set of 3 runs, before the config case existed, also passed all 6 cases; those logs were
replaced by these.

- `FROM alpine AS base` (writes `/base.txt`), `FROM base AS mid` (writes `/mid.txt`), then
  `FROM mid` (`cat /base.txt /mid.txt > /top.txt`): the final template's `/top.txt` holds both
  lines, so each stage was built on the one before it, and no `-stage` template is left behind.
- A base stage with `ENV X=one`, `WORKDIR /w` and `CMD ["sh","-c","echo $X; pwd"]`, then
  `FROM base` + `RUN true`: running the final template with no command prints `one` and `/w`.

How it works: `hm` builds an earlier stage the later one names as a template and sends the
node `fromTemplate` instead of `fromImage` for the stage after it; the stage templates are
deleted at the end of the build.

## Not verified

- `USER` and `ENTRYPOINT` inheritance (only `ENV`, `WORKDIR` and `CMD` were checked).
- A `FROM` that names both a stage and a real image (a stage wins, as in Docker; no test).
- The step cache across a stage-based build.
- Registry credentials for a stage-based build (not used for a template base).
