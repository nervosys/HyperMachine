# Authenticated raw TCP forwarding

`hm sandbox vm tcp SANDBOX_ID --port 5432 --listen 127.0.0.1:15432`
forwards a local TCP listener to the running sandbox's loopback guest port.
Each local connection gets its own guest connection. Binary data and EOF in
either direction are preserved. The guest must run a service on that port;
no guest NIC or public guest IP is needed.

Use `--endpoint https://sandbox-api.example.com` and `HV2_API_KEY` with a
control plane. For an operator CA, add `--api-ca-cert /path/to/ca.pem`.
Certificate verification remains enabled. The CLI reports one JSON object
with its bound `listen` address, sandbox ID and guest port after its first
authenticated guest connection succeeds. Its default listener is
`127.0.0.1:0`, and it accepts only loopback bind addresses. Ctrl-C stops it.
`--max-connections` defaults to 64 (range 1–1024); excess accepted local
connections are closed. `--request-timeout` bounds handshakes, not the
lifetime of an established stream.

The node and control plane expose
`GET /sandboxes/{id}/ports/{port}/tcp`, negotiated with a bodyless HTTP/1.1
request containing `Connection: Upgrade` and `Upgrade: hv2-tcp/1`. A
successful response is `101 Switching Protocols` with that protocol, followed
by raw bytes. A guest connection is established before acceptance. Ports
are 1–65535. The control-plane route requires the existing `sandboxes` or
`admin` API-key scope; `inventory` cannot open it. The node checks its existing
cluster token and configured mTLS identity. Client API keys are not sent to
the node or guest. Paused sandboxes must be resumed explicitly.

The shipped control-plane binary supports API TLS with
`--api-tls-cert CERT.pem --api-tls-key KEY.pem`; these are separate from its
existing proxy `--tls-cert/--tls-key` flags. A tunnel-specific HTTP/1 client
retains the normal node CA, client identity, certificate validity/signature
checks and shared node-name verification. Its supplied rustls configuration
advertises only HTTP/1.1, since reqwest preserves ALPN in preconfigured TLS.
Embedded control planes using custom clients can use
`ControlPlane::with_clients` to supply separate API and HTTP/1 tunnel clients
with matching trust/identity policies. `with_client` reuses the supplied
client for both, so it must support HTTP/1 upgrades.

Registration holds the sandbox lifecycle lock through guest connection setup.
Pause/delete therefore cannot miss a connection that finishes opening during
the transition. Both HTTP port forwarding and the new raw tunnel use that
registration rule. Pause/delete closes active guest streams. The CLI listener
can remain running after a sandbox transition, but a subsequent connection
must pass a fresh handshake and running-sandbox check.
An activity guard remains held for the stream's lifetime, preventing automatic
idle pause or idle eviction of an open tunnel. The CLI keeps one interrupt
subscription across its accept loop so loop iterations cannot lose Ctrl-C.

## Verification

The complete KVM run used a real 1 vCPU / 1024 MiB guest, its static guest
agent, an explicit bounded loopback test service, an owned Redis store, and
the shipped node, control-plane and CLI binaries. The connection path was:
local TCP → CLI → verified control-plane API TLS → node mTLS plus cluster
token → vsock → guest loopback TCP.

All 15 final checks passed in `tcp-e2e-attempt8.json`:

| Check | Evidence |
| --- | --- |
| Wrong API key / inventory-only key | 401 / 403 before accepting a tunnel |
| Invalid port / unavailable guest port | 400 / 502 |
| Wrong cluster token / no node client certificate | 401 / rejected before HTTP |
| Direct node mTLS binary stream | 65,536 exact bytes |
| CLI TLS binary stream after client EOF | Two connections, 262,144 exact bytes each |
| Repeated graceful CLI interrupt | 12 separate 262,144-byte transfers and zero-exit interruptions |
| Server EOF preserves client write | Guest SHA-256 verifies 262,144 subsequent request bytes |
| CLI API certificate trust | Missing operator CA fails before reporting a listener |
| Open tunnel across automatic idle window | 30-second configured window, held 35 seconds: pre-guard node pauses; final node stays running |
| Pause / resume | Active stream closes; paused handshake gets 409; resumed listener transfers 262,144 exact bytes |
| Fork | Forked guest listener transfers 262,144 exact bytes |
| Delete | Active stream closes; next handshake gets 404 |

No sandboxes remained. Every owned CLI, node, control-plane and Redis process
stopped. The normal guest image and scored release daemon were unchanged.
The functional host binaries use the debug profile; this run is not a
performance benchmark or competitor comparison.

The archived failed attempts are retained too. Attempt 1 tested no guest
cases: Python's stricter certificate checks rejected the test CA's missing
key-usage extension. Attempt 2 passed its first three checks, then exposed
the preconfigured-mTLS ALPN bug (400 rather than the expected guest-port 502).
The tunnel TLS configuration was repaired and a real mTLS/HTTP-upgrade
regression added. Attempt 3 passed its first ten checks, then the test sent
an empty pause body under JSON content type. Attempt 4 supplies the pause
API's required JSON object and completes the initial 13 lifecycle checks.
Review then found the missing idle-activity guard. Attempts 5 and 6 exposed
intermittent CLI interrupt timeouts; cleanup masked the observed idle state
in attempt 5. Attempt 6 passed the idle check but timed out interrupting the
CLI after its fork transfer. Neither is treated as a full pass.

The CLI now retains a single interrupt subscription. Attempts 7 and 8 use
the same coordinator, CLI, control plane, kernel and guest image, with the
daemon as the only differing artifact. Attempt 7 records `paused` after
35 seconds across the 30-second idle window. Attempt 8 records `running`
and passes all 15 checks. Both pass the 12 repeated transfer/interrupt checks.
This establishes the idle-activity repair for the tested local configuration;
it is not a latency or throughput comparison.

Every attempt has its exact coordinator and artifact hashes. All four
compiled revisions and their build metadata are retained locally; selected
compiled source bytes are archived alongside the reports. Two separate
fixture images are retained: the later one extends its socket timeout from
30 to 90 seconds so the idle-window check is not ended by the service itself.

Protocol tests verify 256 KiB binary transfer and client EOF through an actual
HTTP upgrade. Cluster fixtures verify routing, credential separation,
authorization, both half-closes and failure statuses. The mTLS regression
verifies HTTP/1 ALPN, mutual authentication, buffered upgrade bytes and a
reply after TLS write shutdown. The eight shipped CLI fixture tests also
pass, including multiple connections and a tunnel that outlives its handshake
deadline. Windows and Linux cluster fixtures (16), cluster unit tests (31),
protocol tests (2), and CLI fixtures (8) pass. Daemon tests pass on Windows
(32) and Linux (36), and strict all-target Clippy passes on both platforms.

UDP, SSH by name and managed public TCP addresses remain absent. A
[matched native transfer comparison and API socket fix](tcp-api-buffering.md) is now recorded;
concurrent tunnel load and TLS/control-plane performance remain unverified.
This change closes a
raw TCP access gap; it does not establish networking parity across every
compared product or an across-the-board performance win.

To reproduce with the fixture image:

```sh
python3 tools/e2e-tcp-tunnel.py \
  --daemon /path/to/hv2-sandboxd --control-plane /path/to/hv2-control-plane \
  --cli /path/to/hm --kernel /path/to/bzImage --initrd /path/to/guest-tcp.cpio.gz \
  --output /path/to/tcp-e2e.json
```

The archived build/image coordinators document the exact static agent and
fixture compilation and image repacking. Test certificates and private keys
are generated inside an owned temporary directory and removed with it.
