# `hm sandbox vm run -p` (KVM, one cluster node)

`tools/check-compose-ports-cluster-kvm.py` (the compose ports check, extended): owned Redis, one
`hv2-sandboxd` node, `hv2-control-plane` over TLS and node mTLS with `--native-port-range`,
`hv2-native-gateway`. No Docker. Nine cases, **3 of 3 runs passed** (`report-run1..3.json`,
`run1..3.log`). The six compose cases from `../../2026-10-23/compose-ports-cluster-kvm` still pass,
plus three new ones:

- `run -p 9001:8080 alpine:3.20 sh -c 'nc -ll -p 8080 -e /bin/cat'` starts the image detached,
  keeps the sandbox, prints `{"sandboxID", "lifetimeSeconds"}` on stdout and
  `container port 8080/tcp is public port N` on stderr; a TCP client of that public port is echoed.
- Deleting that sandbox leaves no sandbox behind.
- `run -p` with a key that has no principal fails naming the port and leaves no sandbox.

## Failed runs

Run 1 of the first attempt failed an *old* case: `compose up` with a key that has no principal
died with a 403 from the new network join ("configured creator identity required") before it
reached the port step, which is where that case expects the failure. That was a regression from
the compose network join (`8ac5477b`, already pushed): a principal-less key could no longer
`compose up` at all on a cluster. Fixed by skipping the join, with a note on stderr, on that 403.
The 3 of 3 above are after the fix.

## Design and what differs from `docker run -p`

`-p` implies detached (`docker run -d -p`): the command starts in the background, no exit code or
output is returned, and the sandbox is kept for `--lifetime` seconds (default 3600) because a
published port is only reachable while it runs. Delete it with `hm sandbox vm delete`/`hm delete`.
The host side of `8080:80` is ignored; the gateway picks the public port.

## Not verified

- UDP and `both` through `run -p`; several `-p` flags at once (parsed and unit-tested only).
- A standalone node (no control plane/gateway): the expose fails and the sandbox is deleted; only
  the code path, not a real standalone node, was run.
- `-it`, stdin, `-v` and `--network` on `run` are still not implemented.
- Timing: none claimed.
