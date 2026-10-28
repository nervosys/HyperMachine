# Multi-stage Dockerfiles, built by `hm sandbox vm build` (2026-10-28)

**What this checks:** that `hm` builds a Dockerfile with several stages and
`COPY --from`, on real KVM, with no Docker anywhere. See
[`RUN_IMAGES.md`](../../../RUN_IMAGES.md#multi-stage-builds).

## Method

`tools/check-hm-multistage-kvm.py` (5 cases) against an owned `hv2-sandboxd` with a
guest kit, the real `hm` binary, and `alpine:3.20` pulled by the node. WSL2 Debian,
KVM, Hyper-V-nested. `hm` builds each earlier stage as a template of its own, starts a
sandbox of it, tars the paths with the guest's busybox, reads them back through exec in
pieces, and uploads them as it uploads a build context. No daemon change.

1. A two-stage Dockerfile builds. `COPY --from=build` takes a directory and a glob,
   `COPY --from=0` a file; the template runs the copied script (mode kept) and prints
   the copied files; the first stage's working directory is absent.
2. Building it again takes 1.7 to 2.0 s against 14.4 to 18.1 s the first time (two
   `sleep 4` steps and a pull): the files read out of a stage are the same bytes.
3. The stage template (`ms-app-stage0`) is gone after the build (`run --template` of
   it is a 404).
4. Three stages, each copying from the one before.
5. `COPY --from` a path the stage lacks fails the build naming the path
   (`nothing at /nope`), and `FROM` an earlier stage is refused.

## Result

5 of 5 cases on 3 runs in a row (`multistage/run1..3`), and the existing
`tools/check-hm-build-kvm.py` (6 cases, its multi-stage refusal case changed to
`COPY --from` an image, which is still refused) 6 of 6 on 3 runs (`build/run1..3`).
The `hm` binary is the release build of this commit's parent plus the change.

## Runs that did not pass

- The first run of `check-hm-build-kvm.py` failed its first case: `USER runner` printed
  `user=root`. The kit's guest agent was the one in `hm-prompts-initrd.cpio.gz`, which
  ignores the exec `user` field (`RUN_IMAGES.md` warns of old agents). With the
  October 23 static agent build in the kit it passed 6 of 6, and all runs above use that
  kit. The multi-stage check passed 3 of 3 on the first kit too (it does not read USER);
  those reports are not kept.
- The kit's agent was swapped by hand, not by a script in the repository.

## Not verified

- `FROM` an earlier stage (refused), `COPY --from=IMAGE` (refused), `RUN --mount`.
- Large trees: a copy is read through exec in 256 KiB pieces and limited to 64 MiB; only
  small files were tried.
- Symlinks and special files in a copied tree; ownership (`--chown` was not combined
  with `--from`).
- A build that fails in an early stage, or an interrupted build: whether a hidden stage
  template is deleted then. Only the success path was checked.
- Other base images than alpine; the stages' own `USER`, `ENV` and `ARG` crossing stages
  (Docker does not carry them either, but nothing here tests that).
