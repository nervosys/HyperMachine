# Node proxy authentication (O3), checked on KVM

The fix for O3 in [SECURITY_AUDIT_2026-11-16.md](../../../security/SECURITY_AUDIT_2026-11-16.md):

- **On the node:** a node with a cluster token accepts a sandbox-proxy connection only if it begins
  with `HV2-PROXY <hmac> <nonce> <unix-time>`. The HMAC is HMAC-SHA256 under the cluster token.
- **On the control plane:** it sends that line on every connection it relays.

Linux under WSL2 with nested KVM, toolchain 1.100.0, release build. The node's sha256 is
`71d55a4850fa3f4b1f2b2961d8c90cca341ea2ca575c832783cec51e7d2171d0`, with the replay fix described
below.

## Direct connections to a node's proxy ([negative.txt](negative.txt))

A node was started with a cluster token. Raw HTTP requests were then sent straight to its proxy
port:

| Case | Sent | Result |
|---|---|---|
| A1 | plain HTTP, no line | connection reset |
| A2 | a line made with the wrong token | connection reset |
| A3 | a line an hour old | connection reset |
| A4 | a valid line | `HTTP/1.1 404 Not Found` (the proxy answered; no such sandbox) |
| A5 | the same line again | connection reset |
| A6 | a fresh valid line | `HTTP/1.1 404 Not Found` |
| B1 | plain HTTP to a node with `--proxy-open` | `HTTP/1.1 404 Not Found` |
| B2 | a valid line to a node with `--proxy-open` | `HTTP/1.1 404 Not Found` |

**A replay was first accepted.** Case A5 at first returned 404. The node had built a new verifier,
with an empty nonce memory, for every connection. The unit tests all reuse one verifier, so they did
not catch this. The node now builds the verifier once. A5 is refused on the binary above.

## Through a control plane ([owner-sharing/report.json](owner-sharing/report.json))

[`verify-owner-sharing-kvm.py`](../../../../tools/verify-owner-sharing-kvm.py) passed all 11 cases,
both before and after the replay fix. Its setup:

- A node with a cluster token and mTLS.
- A control plane over Redis.
- Owners' web sharing, reached through the control plane's proxy to a real guest. This is the path
  that now carries the line.

That check last passed on 2026-10-05. At first it failed here with a 503 on its first request. The
cause was a race in the check, not in the fix:

- The node publishes its template to the store about 20 ms before its API listens.
- The check took the template as the sign that the node was ready.

It now also waits for the node's API port.

## NOT VERIFIED here

- **Platform in the log:** the owner-sharing check logs at `warn`, so its log does not record the
  platform. A debug run of the same setup on the same machine minutes earlier logged
  `Detected hypervisor platform: Kvm`.
- **Repeats:** one run of each check after the fix.
- **O7:** the volume disk guard and quotas were not exercised on KVM. That is unit-tested only.
