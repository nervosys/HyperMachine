# `hm sandbox vm run`, a container image in a VM of its own, on KVM (2026-10-19)

**What this checks:** that the real `hm` binary, against an owned
`hv2-sandboxd` with a guest kit, pulls an image from its registry with no
Docker, builds a template, runs the image's command in a fresh VM sandbox, and
cleans up. See [`RUN_IMAGES.md`](../../../RUN_IMAGES.md).

## Method

`tools/check-hm-run-kvm.py` (7 cases) drives `hm sandbox vm run` against real
Docker Hub images: `alpine:3.20`, `curlimages/curl` (ENTRYPOINT and USER),
`golang:1.22-alpine` (WORKDIR), a tag that does not exist, and `--keep`. The
node runs on real KVM (WSL2, nested) with the registry reached over the
Internet from the node; the guest has no network.

## Result

7 of 7 cases passed on 3 runs in a row (`runs/run3..5.json`). Two earlier runs
failed and led to fixes: the image's `USER` was not applied (a plain exec ran
as root, so `id -u` printed 0; the exec operation now takes an optional
`user`), and `hm`'s own `--version` flag swallowed `--version` meant for the
container. Neither is in the passing runs.

| | run 3 | run 4 | run 5 |
|---|---|---|---|
| first `alpine` run, including pull, template build, boot, command, delete | 5.29 s | 7.02 s | 5.12 s |
| second run of the same image | 0.25 s | 0.62 s | 0.18 s |

Observed: output `hello`, exit code 7 preserved, `-e` reached the command,
`id -u` is 100 in the curl image (its USER), `curl --version` through its
ENTRYPOINT, `pwd` is `/go` in the golang image, a missing tag fails with the
registry's `404 Not Found` and leaves no sandbox, and `--keep` leaves one.

## Not verified

- The times are the check's own stopwatch on a busy host, and include a
  registry round trip: a figure for a small image, not a benchmark.
- Private registries, a numeric `uid:gid` USER, images without a shell,
  multi-platform images, stdin and port publishing: not tried.
- A tag that moved after its template was built (the template is kept).
- The guest agent used was built from this tree's source, static; a template
  whose agent predates the `user` field runs commands as root.
- Windows and macOS clients: the CLI code is shared, but only a Linux `hm`
  was run.
