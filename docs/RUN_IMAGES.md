# Running a container image: `hm sandbox vm run`

A replacement for `docker run` that has no daemon and no shared kernel. The
image is pulled by the node from its registry (no Docker, no `docker
export`), made into a template once, and each run is a VM of its own that is
deleted afterwards.

```
hm sandbox vm run alpine:3.20 echo hello
hm sandbox vm run -e MODE=test python:3.12-slim python -c 'import os; print(os.environ["MODE"])'
hm sandbox vm run curlimages/curl --version      # ENTRYPOINT kept, your arguments after it
hm sandbox vm run --keep golang:1.22-alpine sh   # leave the sandbox, print its ID
```

It prints the command's output and exits with the command's own exit code.

## What it does

1. Names a template from the image reference (`oci-<readable>-<hash>`). If the
   node has none, it asks for one: `POST /templates {"image": ...}`, which pulls
   the image's layers, verifies their digests, and builds the template. The
   node needs `--guest-kit` for that.
2. Creates a sandbox from the template and runs the image's `ENTRYPOINT`
   followed by your command, or by the image's `CMD` if you gave none. Your
   command replaces `CMD` and leaves the `ENTRYPOINT`, as in Docker.
3. Applies the image's `ENV`, `WORKDIR` and `USER`.
4. Deletes the sandbox (not with `--keep`).

| Option | |
|---|---|
| `-e NAME=VALUE` | A variable for the command; repeatable |
| `--keep` | Keep the sandbox and print its ID on stderr |
| `--timeout SECS` | How long the command may run (60) |
| `--build-wait SECS` | How long to wait for a pull and build (600) |
| `--cpus N`, `--memory-mb N` | Sizes of its own |

## What was checked

On a real KVM guest, with the real `hm` binary against an owned node, no
Docker anywhere ([evidence](benchmarks/2026-10-19/hm-run-kvm/README.md)):

- `alpine:3.20`: output, exit code (7 stays 7), streams, `-e`, and no sandbox
  left. The first run, including the pull, the template build and the boot,
  took 5.3 s; a second run of the same image 0.25 s.
- `curlimages/curl`: its `ENTRYPOINT` stays under a given command, its `CMD`
  runs with none, and its `USER` applies (`id -u` is 100, not 0).
- `golang:1.22-alpine`: its `WORKDIR` is the working directory (`/go`).
- A tag that does not exist fails with the registry's 404 and leaves no
  sandbox.
- `--keep` leaves the sandbox running.

## Not done

- **Private registries**: the node's template API takes a username and
  password, but `hm run` has no flag for them yet.
- **A moved tag**: a template is kept by the reference, so `python:3.12` is
  not pulled again when the registry's tag moves. Delete the template to
  refresh it.
- **stdin, `-it`, port publishing (`-p`), volumes (`-v`), `--network`
  options**: not in `run`. Sandboxes have ports, volumes and networks of
  their own through their own commands.
- **`HEALTHCHECK`, `EXPOSE`, `LABEL`, `STOPSIGNAL`**: ignored. **A numeric
  or `uid:gid` USER** was not tried. Only `linux/amd64` images.
- **An image without a shell** was not tried: the command is run through
  `/bin/sh`, and the guest kit adds a static busybox when the image has none.
- **`docker compose`**: not built; it needs sandbox networks, which are
  only partial (see [BOAT_PARITY.md](BOAT_PARITY.md)).
- **Old guest agents**: a template built before this change has an agent that
  ignores the exec `user` field, so the command would run as root. Rebuild
  such a template.

## Building from a Dockerfile: `hm sandbox vm build`

```
hm sandbox vm build -t myapp ./context            # prints "myapp"
hm sandbox vm build -t myapp --build-arg MODE=test -f ci/Dockerfile .
hm sandbox vm run --template myapp                # ENTRYPOINT + CMD
hm sandbox vm run --template myapp other args     # replaces CMD
```

`hm` parses the Dockerfile, packs the context (honouring `.dockerignore`, in a
reproducible tar) and uploads it once, then the node pulls the `FROM` image
and runs the steps in a VM. Supported: `FROM`, `ARG`, `ENV`, `WORKDIR`,
`COPY`, `RUN`, `USER`, `ENTRYPOINT`, `CMD`. The node caches by the whole step
prefix, so a repeat build is quick; `--no-cache` runs every step again. A
failing step ends the build at that step with a non-zero exit and no template.

Checked on KVM, 6 cases on 3 runs
([evidence](benchmarks/2026-10-19/hm-build-kvm/README.md)). Not done:
multi-stage builds and `COPY --from` (refused up front), `ADD` of a URL or
archive, `SHELL`, `ONBUILD`; `LABEL`, `EXPOSE`, `HEALTHCHECK` and `VOLUME` are
accepted with a warning and ignored. See the evidence's "Not verified" list.
