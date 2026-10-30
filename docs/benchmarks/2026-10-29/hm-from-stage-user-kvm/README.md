# `USER` and `ENTRYPOINT` through `FROM <earlier stage>` (KVM)

`tools/check-hm-multistage-kvm.py` gained an eighth case; an owned `hv2-sandboxd` with a guest kit,
no Docker. **3 of 3 runs passed all 8 cases** (`report-run1..3.json`, `run1..3.log`); no failed runs.

The first stage (`alpine:3.20`) runs `adduser -D -u 1234 app`, sets `USER app`, an `ENTRYPOINT` that
prints `ep:<uid> <args>` and `CMD ["default"]`. The second stage is `FROM` that stage with a `RUN true`.
The built template run with no command prints `ep:1234 default`, and run with `given` prints
`ep:1234 given`: the user, the ENTRYPOINT and the CMD all came through, and a given command replaces
CMD only.

## Not verified

- A later stage that sets its own `USER` or `ENTRYPOINT` over an inherited one.
- A numeric or `uid:gid` `USER` in a stage (a group is ignored with a warning).
- Inheritance through a chain of three stages (the `ENV`/`WORKDIR`/`CMD` case used one hop).
