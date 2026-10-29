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

## Private registries

`run` and `build` take `--registry-user NAME` and `--registry-password-env VAR`.
Both are needed together; the password is read from the environment variable,
never from an argument, so it stays out of the process list and shell history.
An unset or empty variable fails before anything is sent. The node does not
store or echo the password. [Checked on KVM](benchmarks/2026-10-19/hm-registry-auth-kvm/README.md)
against a Basic-auth registry on localhost (6 cases, 3 runs); a real registry
is not tested.

## Compose

`hm sandbox vm compose -f compose.yaml [-p NAME] up` starts each service of the
file in a VM of its own, detached, in `depends_on` order; `down` deletes them.

```yaml
services:
  db:
    image: alpine:3.20
    command: ["sh", "-c", "echo up; sleep 600"]
  app:
    build: ./app          # or {context: ./app, dockerfile: Dockerfile}
    depends_on: [db]
    environment: {GREETING: hello}
```

Sandboxes are named `PROJECT-SERVICE` and carry the metadata
`hm.compose.project` and `hm.compose.service`, which `down` finds them by. The
project defaults to the compose file's directory. `up` refuses a project that
already has sandboxes, and deletes what it started if a service fails to
start. A command's output goes to `/tmp/hm-compose.log` in the guest; `compose logs [SERVICE] [--tail N]` prints its tail.
`ports:` (short syntax) reserves a public port per container port through the
control plane's `--native-port-range`; the gateway picks the public number, which
`up` prints, so a requested host port is not honoured, and the command fails
(and deletes what it started) when the control plane has no port range.
`ports:` is [checked on KVM](benchmarks/2026-10-23/compose-ports-cluster-kvm/README.md)
through a control plane and the native gateway for TCP: `up` prints the public
port, a client of it reaches the service's listener, and `down` closes it. UDP and
`both` through compose are NOT verified. The key needs a `principal_id`, and a
**team-scoped key is refused** (403) at the template build that an `image:`
service starts with, so a team needs an operator to have built the template. A top-level
`volumes:` declares named volumes (created as `PROJECT-NAME`, 9P mounts, they
survive `down`; delete one with `hm sandbox vm volume delete`); a service mounts
one with `data:/path`. Bind mounts, `:ro` and options are refused, volumes need a
Linux host, and unknown keys are refused rather than ignored, and `depends_on` is start order
only. [Checked on KVM](benchmarks/2026-10-19/hm-compose-kvm/README.md) (8 cases,
3 runs).

On a node that is part of a cluster with private networks (`--network`, mTLS), `up` also
joins every service to a private network named after the project and gives the guest a
search domain, so a service reaches a sibling by its service name (`nc server 8080`);
see [PRIVATE_VM_NETWORKS.md](PRIVATE_VM_NETWORKS.md) and the
[KVM check](benchmarks/2026-10-28/compose-network-cluster-kvm/README.md) (6 cases, 3 runs, one
node). On a node without that, the services run with no network between them and `up` says
so. A project name with `_` gets no network. Two nodes: NOT VERIFIED.

A service may have a `healthcheck:` (`test` as `[CMD, ...]`, `[CMD-SHELL, "..."]` or a string, `interval` like
`5s` or `1m30s` (default 30s), `retries` (default 3), `disable`), and another may wait for it with
`depends_on: {db: {condition: service_healthy}}`. `up` runs the test in the guest, `interval` apart,
until it exits 0, and fails (taking back what it started) after `retries` failures in a row; a
`service_healthy` wait on a service with no health check is refused. Unlike Docker there is no
`start_period` or `timeout`, and health is checked only during `up`: nothing watches it afterwards.
[KVM check](benchmarks/2026-10-28/compose-health-cluster-kvm/README.md) (6 cases, 3 of 3 runs).
`restart:` is still refused; `condition: service_completed_successfully` is refused.

## Publishing a port from `run`

`hm sandbox vm run -p 8080:80 IMAGE [CMD]` (like `docker run -d -p`) starts the image
detached, keeps the sandbox for `--lifetime` seconds (default 3600) and prints the
public port; the command's output and exit code are not returned. The host side is
ignored, the gateway picks the public port. It needs a control plane with a native port
range and a key with a principal. [Checked on KVM](benchmarks/2026-10-28/hm-run-publish-cluster-kvm/README.md)
(3 cases added to the compose ports check, 9 in all, 3 runs); UDP is NOT VERIFIED.

## Not done

- **A moved tag**: a template is kept by the reference, so `python:3.12` is
  not pulled again when the registry's tag moves. Delete the template to
  refresh it.
- **stdin, `-it`, volumes (`-v`), `--network` options**: not in `run`. Sandboxes have ports, volumes and networks of
  their own through their own commands.
- **`HEALTHCHECK`, `EXPOSE`, `LABEL`, `STOPSIGNAL`**: ignored. **A numeric
  or `uid:gid` USER** was not tried. Only `linux/amd64` images.
- **An image without a shell** was not tried: the command is run through
  `/bin/sh`, and the guest kit adds a static busybox when the image has none.
- **`compose` beyond a first slice**: no bind mounts or `restart:`, and
  health is checked only during `up`; see
  [Compose](#compose).
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
`ADD` of a URL or archive, `SHELL`, `ONBUILD`; `LABEL`, `EXPOSE`,
`HEALTHCHECK` and `VOLUME` are accepted with a warning and ignored. See the
evidence's "Not verified" list.

### Multi-stage builds

```
FROM golang:1.22 AS build
RUN go build -o /out/app ./...
FROM alpine:3.20
COPY --from=build /out/app /usr/local/bin/app
```

Every `FROM` starts a stage, named with `AS name`. `COPY --from=NAME` (or the
stage's number) takes files out of an earlier stage: a file, a directory or a
glob, as `COPY` takes them from the context. `hm` builds each earlier stage the
last one reads as a template of its own, starts a sandbox of it, tars the paths
with the guest's busybox, reads them back and uploads them as it uploads a
context; the stage templates are deleted when the build is done. The same files
make the same bytes, so the node's step cache is hit on a repeat build. The
stages the last one does not read are not built.

Refused, by name: `FROM` an earlier stage (building on a stage's result),
`COPY --from=IMAGE` (copying out of an image that is not a stage), and a path the
stage does not have (the build fails, naming the path). A copy is limited to 64
MiB, and is read through the guest's exec in pieces, so it is slow for
large trees. [Checked on KVM](benchmarks/2026-10-28/hm-multistage-kvm/README.md):
5 cases on 3 runs.
