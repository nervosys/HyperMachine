# Official-client HTTP cancellation with real KVM work

The real-guest cancellation check found an interoperability failure in the initial HTTP transport. Explicit cancellation closed the original POST with HTTP 408. The official Python MCP client 1.23.3 treated this as an unhandled transport error and terminated its task group. The first report, sanitized client traceback and exact pre-fix HTTP source remain in this archive. CLI/control/Redis stopped gracefully; the node required forced termination after the client could not complete guest cleanup. This failed cohort is not counted as a pass.

The corrected transport ends the cancelled POST with an empty text/event-stream response and no JSON-RPC result. The cancellation notification is acknowledged only after the worker releases its protocol lock, so the session can immediately accept another request. The cancellation-map lock is released before waiting for the protocol lock, avoiding lock-order deadlock. Normal tool responses remain JSON; GET server-push streams and replay remain absent. This follows the cancellation requirement to avoid a response to the cancelled RPC while preserving the HTTP request transport contract:

- https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation
- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports

The final release SHA is fd55361af11bf5dd10d6ffa84a81d6b9200a66079bb4ece2c72a73ff13b55ed1. It passed all 144 isolated Linux CLI library tests and strict Clippy. Tests verify an empty terminal SSE response and immediate session reuse after cancellation acknowledgement, including saturated admission. The three provisional worktree boot files were excluded; source-context.json binds the isolated compilation.

The official client completed 27 operations through verified MCP HTTPS, control-plane TLS and node mTLS with real one-vCPU, 1024-MiB KVM guests. An independent authenticated TLS API probe first confirmed the guest command had written its start marker. After sending cancellation, the same MCP session ping succeeded, and an independent probe observed the command's finish marker. No cancelled RPC response arrived, including after remote completion. This proves release of the server wait and continued accepted guest work, not rollback or automatic cancellation of a caller's application future. The ping timing is a fixture observation, not a latency ranking.

The final cohort also passed the fourteen-tool binary roundtrip and oversized-download check, checkpoint restore, pause/resume, fork isolation, error handling and deletion. Its sandbox inventory was empty, all four owned processes exited zero, no cleanup errors occurred and all artifacts remained unchanged. The first failure used file tools disabled; the final regression additionally enabled file tools. Neither is a performance comparison.

```sh
python -O tools/check-mcp-http-kvm.py --output /new/owned/run --daemon /path/node --control-plane /path/control --cli /path/hm --kernel /path/kernel --initrd /path/guest --check-cancellation --files
```

Credentials, private certificates, guest data and binaries are excluded. Earlier archives remain immutable. OAuth/browser login, multi-user isolation, general event streaming/replay, production proxy operation and competitor performance leadership remain incomplete or unverified.
