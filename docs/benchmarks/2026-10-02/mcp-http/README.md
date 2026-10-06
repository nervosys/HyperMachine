# Authenticated MCP HTTP transport: local verification

The new hm sandbox vm mcp-http command exposes twelve sandbox lifecycle/exec/checkpoint tools over JSON-response HTTP, using the existing configured upstream API role. It requires a separate operator-managed Bearer credential and binds only to loopback for an operator TLS proxy. Exact Origin allowlisting, protocol header validation, random session IDs, ten-minute session expiry, a 64-session bound, bounded message bodies/read deadlines and explicit cancellation are implemented. OAuth, browser login, native TLS, SSE, response replay and HTTP binary file tools remain absent.

The isolated clean source tree passed all 139 Linux CLI library tests, including five HTTP tests and existing stdio/file/cancellation regressions. HTTP tests operate real loopback listeners and check authentication, rejected origins/versions, initialization gates, empty notification acknowledgements, twelve-tool discovery, termination/expiry, duplicate headers, malformed IDs, oversized bodies and cancellation during a delayed upstream call. The cancellation test also verifies overlapping requests are refused and the session works after cancellation. Strict Clippy passed with the existing too_many_arguments exception. A locked offline compile passed after aligning the existing cluster manifest to the current lockfile; the change adds only uuid to hm-cli dependencies.

source-context.json binds the isolated source tree. The three provisional worktree boot files were excluded; recorded hashes match the previously accepted clean sources. Tests used fixtures, not KVM guests or an HTTPS proxy. No production deployment, browser interoperability, multi-user isolation or performance win is established.

```sh
cargo test --offline --locked -p hm-cli --lib
cargo clippy --offline --locked -p hm-cli --lib -- -D warnings -A clippy::too_many_arguments
```

See ../../../src/ai/mcp-server.md for configuration and limits. This archive preserves local development evidence; real KVM/TLS verification remains required.
