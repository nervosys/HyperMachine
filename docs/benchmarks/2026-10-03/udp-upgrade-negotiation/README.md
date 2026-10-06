# UDP HTTP upgrade negotiation

Added hv2-udp/1 negotiation alongside hv2-tcp/1. Both require bodyless HTTP/1.1 GET and Connection: Upgrade. UDP refuses the TCP protocol identifier, non-GET methods, HTTP/2 and nonzero content length. The accepted upgrade preserves its selected identifier while using the existing bounded bidirectional stream transport; datagram validation remains at the guest relay.

All three focused transport tests pass: malformed TCP negotiation, new UDP negotiation cases and the existing real HTTP-upgrade TCP binary/EOF round trip. 1,065 other API library tests were filtered out, not run. Reproduce with `cargo test --offline --locked -p hv2-api --lib tunnel::` in the accepted isolated checkout. Its API library manifest/source context differs from current root API TLS additions; this is transport verification, not a full root API validation.

UDP-specific upgraded data round trips, authenticated node/control-plane routes, lifecycle cleanup, CLI peers, TLS and real KVM remain unverified at this step. Node wiring is subsequent work. No end-to-end UDP capability or competitor advantage is claimed.
