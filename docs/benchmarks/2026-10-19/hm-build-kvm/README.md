# `hm sandbox vm build`, a Dockerfile built into a template, on KVM (2026-10-19)

**What this checks:** that the real `hm` binary, against an owned
`hv2-sandboxd` with a guest kit, builds a Dockerfile (no Docker anywhere) into
a template, and that `hm sandbox vm run --template` then runs it. See
[`RUN_IMAGES.md`](../../../RUN_IMAGES.md).

## Method

`tools/check-hm-build-kvm.py` (6 cases) writes a build context and drives
`hm sandbox vm build` and `run --template` against `alpine:3.20`, pulled by
the node. The node runs on real KVM (WSL2, nested); the guest has no network.

1. A Dockerfile with ARG, ENV, WORKDIR, COPY of a file and of a directory, two
   RUNs, USER, ENTRYPOINT and CMD builds. The run prints the arguments, the
   environment, the user, the working directory, the ARG default a RUN wrote,
   and the copied directory's contents, all as expected.
2. A command after the template replaces CMD and keeps the ENTRYPOINT.
3. `--build-arg GREETING=howdy` changes what the build wrote.
4. `.dockerignore` keeps `*.log` and a directory out of the guest, and
   `COPY . ` copies the rest.
5. Building the same Dockerfile again is quicker (a step that sleeps 4 s).
6. A failing RUN fails the build at its step with a non-zero exit, and a
   multi-stage Dockerfile is refused before anything is sent.

## Result

6 of 6 cases passed on 3 runs in a row (`runs/run1..3.json`).

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| the case-1 build, including the pull | 7.78 s | 11.32 s | 9.88 s |
| slow Dockerfile, first build | 5.00 s | 4.97 s | 4.99 s |
| the same, built again (node step cache) | 0.73 s | 0.93 s | 0.76 s |

Earlier runs that failed, and what they led to: `run --template` rejected a
real step-built template because `GET /templates` does not list those (named
templates are no longer checked against the list); and the first version of
the cache case compared two builds that both hit the cache of earlier cases
(0.72 s against 0.73 s), which proved nothing, so the case now builds a context
no other case builds. None of these is in the passing runs.

## Not verified

- The times are the check's own stopwatch on a busy host: a figure for a small
  image, not a benchmark.
- Refused up front: a second `FROM` and `COPY --from` (multi-stage), `ADD` of
  a URL or an archive, `SHELL`, `ONBUILD`, here-documents. `LABEL`, `EXPOSE`,
  `HEALTHCHECK`, `VOLUME` and `STOPSIGNAL` are accepted with a warning and do
  nothing. Only the multi-stage refusal was exercised by the check; `ADD` of a
  plain file (treated as `COPY`) and the warnings are covered by unit tests,
  not by a KVM run.
- Private registries, images without a shell, a numeric `uid:gid` USER:
  not tried.
- Symlinks in the context are skipped. A Windows context loses the exec bit
  (use `RUN chmod`).
- Only a Linux `hm` was run. The client code is shared, and the Windows
  `cargo check --all-targets` and the 174 `hm-cli` unit tests pass there.
- The guest agent was built from this tree; a template whose agent predates
  the exec `user` field runs commands as root.
