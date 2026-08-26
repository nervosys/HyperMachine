# Running commands inside a guest

`execute_script` evaluates a Rhai script **on the host** against a read-only
view of a VM. It has never run anything inside a guest, and the module that
implements it now says so. This document describes the mechanism that does:
`hv2-guest-agentd`, a small program that runs in the guest and answers the host
over virtio-vsock.

## What this is made of

| Piece | Where | What it does |
| --- | --- | --- |
| `GuestQueue` | `hv2-core/src/devices/virtio_queue.rs` | Split virtqueues read out of guest memory, so a real driver and the device see the same bytes |
| `VirtioMmioTransport` | `hv2-core/src/devices/virtio_mmio.rs` | The virtio-mmio v2 register file a guest driver probes |
| `VsockDevice` | `hv2-core/src/devices/virtio_vsock.rs` | The socket device: connections, credit, packet encoding |
| `hv2-guest-agent` | `crates/hv2-guest-agent` | The wire protocol, plus the in-guest binary |
| `GuestAgent` | `hv2-agent/src/guest_agent.rs` | The host-side client |
| `AgentVM::exec_in_guest` | `hv2-agent/src/agent_vm.rs` | The operation an agent calls |

## Four things have to be true

None of them can be arranged from inside the host-side code alone, and each
fails differently on purpose:

1. **The VM has a vsock device.** `AgentVM::attach_guest_channel(cid)` or
   `VM::attach_vsock(cid)`. Without it `exec_in_guest` returns an error saying
   there is no channel — not a timeout, because "the host never attached a
   device" and "the guest never answered" send you to different places.
2. **The guest kernel was told where to look.** virtio-mmio has no
   enumeration. Put `AgentVM::guest_kernel_args()` on the kernel command line;
   it renders as `virtio_mmio.device=4K@0xd0000000:5`. Without it the window is
   mapped and no driver ever reads it.
3. **The guest is running.** The device only moves bytes when the guest driver
   kicks a queue.
4. **`hv2-guest-agentd` is running inside it.** A running guest with no agent
   looks exactly like a running guest with one, until something asks — which is
   what `AgentVM::ping_guest` is for.

## Building the guest binary

The binary is a normal Rust program that links only `libc`, so it
cross-compiles from any host with the Linux target installed:

```sh
rustup target add x86_64-unknown-linux-gnu     # once
cargo build --release -p hv2-guest-agent \
    --target x86_64-unknown-linux-gnu
# target/x86_64-unknown-linux-gnu/release/hv2-guest-agentd
```

For a guest with a different libc, or no libc at all, build against musl and
get a static binary that runs in any userspace including a bare initramfs:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release -p hv2-guest-agent \
    --target x86_64-unknown-linux-musl
```

Note the caveat the rest of this repo has learned the hard way: `cargo check`
does not link. Build, do not check, before shipping a guest image.

## Putting it in a guest image

Copy the binary in and start it from the guest's init system. Nothing in the
agent daemonises or restarts itself — an init system already knows how to do
both, and knows where its logs go.

A systemd unit:

```ini
[Unit]
Description=HyperMachine guest agent
After=network.target

[Service]
ExecStart=/usr/local/bin/hv2-guest-agentd
Restart=always

[Install]
WantedBy=multi-user.target
```

For an initramfs with no init system, one line in the init script is enough:

```sh
/usr/local/bin/hv2-guest-agentd &
```

The vsock transport module is `vmw_vsock_virtio_transport`. Stock distribution
kernels load it automatically once the device is probed; a minimal kernel needs
`CONFIG_VSOCKETS=y` and `CONFIG_VIRTIO_VSOCKETS=y`.

## Trust

The agent runs whatever the host asks, as whatever user started it, and does no
authentication of its own. The channel is the boundary: only the host can open
a connection to the guest's vsock port. That is the same trust model as a
serial console with a shell on it, and it is worth stating rather than
implying — do not run this in a guest whose host you do not trust with the
account it runs as.

## What the protocol does not do

There is no streaming. A command runs to completion and its output comes back
in one response, capped at 1 MiB per stream with a `truncated` flag when it was
cut. That is right for "run this and tell me what happened" and wrong for an
interactive shell. Saying so is deliberate: describing this as more than it is
would repeat exactly the defect it was built to fix.

A response also keeps an exit code and a terminating signal apart. A program
killed by SIGKILL did not exit 0, and an API that flattened the two would
report a crash as a success.

## Checking a guest image

`tools/vsock_probe.py` speaks the protocol to a running agent over a real
`AF_VSOCK` socket and tells the three failure modes apart — no agent, a version
mismatch, or an agent that is not answering:

```sh
sudo modprobe vsock_loopback      # only for a local run
./hv2-guest-agentd &
python3 tools/vsock_probe.py 1
```

It pings, runs a command, checks the output and exit code come back intact, and
confirms a mismatched protocol version is refused rather than misread.

## What is and is not verified

**The agent half works.** Built and run on Linux 6.18, it binds `AF_VSOCK` port
1024 (confirmed with `ss --vsock`), accepts a connection, answers a ping,
executes `/bin/sh -c` and returns stdout with the program's real exit code, and
refuses a mismatched protocol version. That is the daemon exercised through the
kernel, not through a test double.

**The device half is not.** No kernel has booted against `VsockDevice`. Its
tests lay rings out in guest memory and publish descriptors exactly as a driver
would, so what they support is "the device implements the protocol", not "a
Linux guest driver talked to it". Note that the two halves have never met: the
agent was proven over the kernel's own vsock stack, and the device was proven
against tests. Joining them is the first real boot, which needs a machine with a
usable hypervisor — see the handoff.
