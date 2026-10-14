# A port of yours, reached from inside a sandbox

A program in a sandbox can reach a port on the caller's machine, as with
`ssh -R`. It connects to its own loopback, and the connection comes out on
the caller's side:

```sh
# On your machine: a dev server on 8080, reachable in the sandbox as 127.0.0.1:7000.
hm sandbox vm reverse $SBX --port 7000 --to 127.0.0.1:8080

# In the sandbox:
curl http://127.0.0.1:7000/
```

A machine does the same, with its name in place of the sandbox's ID:

```sh
hm sandbox vm machine reverse web-01 --port 7000 --to 127.0.0.1:8080
```

`--to` is any address your machine can reach. `--waiting N` (default 4, at
most 32) is how many connections are kept waiting for the guest, and so how
many it can make at once without one being turned away. Ctrl-C stops it.
The other direction, a sandbox's port reached from your machine, is
`hm sandbox vm tcp`.

## How it works

No network is involved. A sandbox without a network interface reaches your
port all the same, and nothing else can reach it.

- **`GET /sandboxes/{id}/ports/{port}/reverse`,** upgraded to
  `hv2-reverse/1` as the [TCP tunnel](TCP_STDIO.md) is to `hv2-tcp/1`, is
  one connection waiting. The guest agent listens on the guest's
  `127.0.0.1:{port}`. When a program there connects, the caller is sent one
  byte, `+`, and then the program's bytes, both ways.
- **One upgrade carries one connection.** A caller wanting more keeps
  several waiting, as `hm` does, and opens another as each is taken. Every
  waiting connection for a port shares the guest's one listener.
- **The guest's port is closed** 5 seconds after the last connection
  waiting on it goes. A program connecting after that is refused.
- **Through the control plane:** a sandbox's reverse port is forwarded
  like the TCP tunnel. A machine's is served by its node directly, as it
  is not forwarded by the control plane yet.
- **At most 64 connections waiting** for a sandbox, across its ports
  (429 past that). A sandbox's waiting and carried connections close when
  it pauses or ends.
- It needs the guest agent from this release or later; an older one cannot
  listen (502).

Evidence: [real KVM checks](benchmarks/2026-10-13/reverse-port-kvm/README.md).
