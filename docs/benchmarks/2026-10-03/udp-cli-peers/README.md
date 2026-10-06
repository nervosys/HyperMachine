# UDP CLI peer forwarding

Added `hm sandbox vm udp ID --port PORT --listen 127.0.0.1:0 --max-peers 64`. The listener is loopback-only, uses the authenticated hv2-udp/1 upgrade and gives each local SocketAddr its own upstream session. Eight queued datagrams per peer bound buffering; full queues, oversize local messages and new peers over the configured cap are dropped. Peer sessions have a 30-second outbound idle wait, 35-second complete-frame receive wait and five-second write waits. Ctrl-C aborts and joins active session tasks. UDP losses remain possible.

The shipped CLI owned HTTP-upgrade fixture verifies two separate upstream sessions, exact isolated replies for empty, four-byte binary and 65,507-byte payloads, refusal of a third peer at a two-peer cap, and successful Ctrl-C process reaping. Twelve existing VM CLI tests pass. Reproduce with `python3 tools/check-udp-cli.py --cli /path/to/hm` and `cargo test --offline --locked -p hm-cli --test sandbox_vm_client`.

The fixture is a protocol echo server, not the real control plane, sandbox daemon or guest socket. This does not verify real KVM, TLS, pause/delete cancellation, idle expiration, malformed replies or repeated failed-handshake cleanup. Those checks remain required before UDP feature parity is claimed.
