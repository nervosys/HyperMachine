# UDP datagram transport codec foundation

Added a shared guest-agent datagram codec: big-endian u16 payload length followed by exactly that many bytes, limited to 65,507 bytes (IPv4 UDP payload without IP options). Empty datagrams remain messages. EOF before a prefix is clean termination; truncated prefixes/payloads return UnexpectedEof. Oversize lengths fail before payload allocation/read, and oversize writes fail before emitting a prefix.

The guest-agent library and binary test suites pass in the accepted isolated checkout. Three new codec tests verify concatenated empty/binary/maximum-size boundaries, truncation distinction and oversize refusal without payload I/O. Reproduce with `cargo test --offline --locked -p hv2-guest-agent`.

This is a protocol foundation only. It is not yet wired to guest UDP sockets, agent forwarding, authenticated node/control-plane upgrades or a CLI listener. UDP access remains absent in the capability matrix. Socket lifecycle/cancellation, peer isolation, idle deadlines, buffering, guest execution and end-to-end payload verification remain required before any UDP capability claim.
