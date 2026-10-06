# Guest UDP relay foundation

Added ForwardUdp to the shared protocol and a guest-side dispatcher/relay. It binds a private IPv4 loopback UDP socket, connects to the requested nonzero guest port, acknowledges socket setup, then preserves length-framed datagrams in both directions. UDP acknowledgement does not prove a listener exists. Host bytes already buffered after the request are retained. A connected socket accepts replies only from its selected peer.

Stream reads have a 30-second inactivity timeout, stream/socket writes a five-second timeout, and UDP receives poll every 100 ms. Transport EOF/error closes the stream, stops receiving and joins the sender thread. Buffers are bounded by the datagram limit (plus one byte to detect oversized receives) and the existing request bound. Individual read timeouts are not an absolute session lifetime.

All 18 protocol-library tests and three guest-binary tests pass in the accepted isolated checkout. The new owned socket test verifies early-buffer binary delivery, empty and binary round trips with exact boundaries, and EOF cleanup. Reproduce with `cargo test --offline --locked -p hv2-guest-agent`.

This is not yet a user-facing UDP capability: host AgentVM/GuestAgent callers, authenticated node/control-plane routes, CLI peer handling and a rebuilt KVM guest image still need wiring and verification. Socket tests use UnixStream as the transport, not AF_VSOCK or real KVM. IPv6, full-size guest socket transfers, malformed-frame relay cleanup and lifecycle races remain unverified. No feature-parity or performance win is claimed.
