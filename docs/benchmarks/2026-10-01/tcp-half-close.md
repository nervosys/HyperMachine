# TCP forwarding prerequisite: directional EOF

The virtio socket device previously ignored SHUTDOWN direction flags and
closed both directions immediately. The host TCP relay also closed the
guest stream when its TCP reader reached EOF. This prevented a client from
finishing its request with EOF and then receiving a response.

The device now tracks host and peer shutdown directions separately. Host
write shutdown sends only the SEND flag, prohibits subsequent host writes,
and keeps guest reads available. Peer SEND shutdown produces read EOF after
buffered data has been consumed while permitting host writes. Peer RECEIVE
shutdown prohibits host writes. A full peer shutdown retains buffered input
and defers its reset acknowledgment until that input is consumed.

Both host and guest TCP relays propagate orderly EOF to the corresponding
write half. Copy errors abort both directions. Sandbox pause/stop retains
the existing explicit full-close path. Direction flags follow the
[virtio socket specification](https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html),
section 5.10.6.5.

Validation: all 24 socket-device tests passed on Windows and Linux; strict
Linux core and agent Clippy passed; Windows agent and sandbox daemon checks
passed. A Linux guest-relay regression test uses a real loopback TCP socket
and a Unix stream standing in for the vsock file descriptor: after the TCP
server sends EOF, the client receives EOF and can still send a request that
the server reads intact. That test passed. It tests the relay, not KVM or
the Linux virtio socket driver.
Strict Linux guest-agent and sandbox-daemon Clippy also passed for all targets.

This is a prerequisite repair, not a completed public raw TCP feature.
Authenticated node and control-plane tunnel endpoints, the CLI, a rebuilt
guest image, and real KVM lifecycle verification remain required. Existing
archived benchmark executables and guest images are unchanged; no new
performance or competitor-parity claim follows from these checks.
