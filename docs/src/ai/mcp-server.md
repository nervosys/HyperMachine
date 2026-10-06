# MCP and agent HTTP interfaces

`hm sandbox vm mcp` implements MCP 2025-11-25 over newline-framed stdio, backed by the configured sandbox API. It exposes 12 lifecycle, exec and checkpoint tools. Two binary file tools are available when an operator supplies the envd route. Configure `HV2_API_KEY` separately for the API role the agent should receive.

```sh
hm sandbox vm --endpoint https://sandbox.example.test \
  --api-ca-cert /secure/operator/api-ca.pem mcp
```

An MCP client launches that command and exchanges JSON-RPC messages on stdin/stdout. Initialization precedes `notifications/initialized`, tool discovery and calls. For example, these are separate newline-delimited client messages:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"example-client","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sandbox_list","arguments":{}}}
```

Use `mcp --envd-endpoint https://proxy.example.test --envd-domain sandbox.example.test` to configure the optional file tools. Endpoints come from operator configuration, not tool arguments. API authorization still applies to lifecycle and guest operations. Credentials in returned sandbox descriptions are scrubbed. Cancellation releases the client wait; an operation already accepted by the remote API may continue and is not rolled back.

The separate `hm serve --rest-port 8080` command exposes a legacy JSON-over-HTTP agent API backed by the local VM manager. Its `/mcp/tools` discovery and `/mcp/call` invocation routes use a custom interface. They do not implement the standardized MCP HTTP transport. The command binds `0.0.0.0`; `HM_API_KEY`, when configured separately, controls Bearer authentication on its protected routes. It provides no native TLS. The claimed `hm mcp serve`, `--tls-cert`, `--tls-key`, `--api-key` and `HM_MCP_PORT` setup shown in older versions of this guide is not implemented by the current CLI. A `/mcp/stream` WebSocket endpoint is also absent.

`hm sandbox vm mcp-http` adds a JSON-response HTTP transport at `/mcp`, backed by the same configured sandbox API role. Supply a separate `HM_MCP_TOKEN` in the process environment (32-4096 visible ASCII bytes); clients send it as a Bearer credential. This operator-managed credential is not OAuth or browser login. The listener accepts only loopback addresses. Put it behind an operator HTTPS proxy for remote use, protect the local port, and configure the proxy to preserve authentication, Origin and MCP headers. The command itself provides no TLS.

```sh
hm sandbox vm --endpoint https://sandbox.example.test \
  --api-ca-cert /secure/operator/api-ca.pem mcp-http \
  --listen 127.0.0.1:3985 --allow-origin https://agent.example.test
```

Origins are exact HTTPS origins. Requests without Origin are accepted for native clients; a supplied Origin must match the configured allowlist. All methods require authentication. POST requires JSON content and Accept support for both JSON and event streams. Initialization returns a cryptographically random session ID; subsequent messages require it. Sessions expire ten minutes after creation, with at most 64 registered sessions. The supported negotiated protocol is 2025-11-25. An unsupported protocol header is refused; absent headers use the known session version. Accepted notifications return empty 202 responses; GET returns 405 because server-push SSE is unavailable; DELETE ends the session. Bodies are limited to 1 MiB and a bounded read deadline. Process-wide admission permits at most sixteen concurrent body reads and sixteen workers. Saturated admission returns 503; cancellation notifications bypass the worker limit. These limits bound local waits and do not limit remote effects already accepted by the API. Invalid Origins receive 403 before credential checks; a missing or invalid credential receives 401 with a Bearer challenge.

The HTTP transport exposes twelve lifecycle/exec/checkpoint tools. Add `--envd-endpoint` and optional `--envd-domain` to enable the same bounded binary upload/download tools as stdio, for fourteen tools total. The route is operator configuration, never a tool argument. For a private HTTPS trust root, supply `--envd-ca-cert /secure/operator/envd-ca.pem`; certificate verification remains enabled. Transfers are limited to 256 KiB each. One request executes per session; overlapping normal requests receive 429 rather than queueing. Cancellation notifications bypass that gate and release the matching client wait; a cancelled original request ends with an empty `text/event-stream` response and no JSON-RPC result. Already accepted upstream effects can continue. Disconnect alone does not cancel the worker. There is no response replay, event stream, OAuth discovery, token rotation, multi-user authorization, or browser SSO. The [official-client HTTPS/KVM lifecycle fixture](../../benchmarks/2026-10-02/mcp-http-kvm/README.md) verifies checkpoint restore, pause/resume, fork isolation and cleanup through an owned HTTPS proxy, control-plane TLS and node mTLS. It does not establish production proxy operation or browser login.

The [MCP 2025-11-25 transport specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) permits JSON responses without SSE, requires an accepted notification to receive an empty 202 response, and defines session-ID handling for stateful servers. An HTTP GET may return 405 when SSE is unavailable. The legacy JSON API is not evidence that these requirements are met.

See the [current feature comparison](../../PLATFORM_PARITY.md) and the preserved [stdio cancellation evidence](../../benchmarks/2026-09-30/mcp-stdio-cancellation.json).

[Admission verification](../../benchmarks/2026-10-02/mcp-http-admission/README.md) includes sixteen live delayed HTTP calls, cancellation during saturation, 143 CLI tests and the updated release passing the official-client HTTPS/KVM lifecycle fixture.

[HTTP binary file verification](../../benchmarks/2026-10-02/mcp-http-files/README.md) passes a 256 KiB guest roundtrip and oversized-download refusal through trusted envd HTTPS, alongside 25 official-client KVM operations, 144 CLI tests and strict Clippy.

[Real HTTP cancellation verification](../../benchmarks/2026-10-02/mcp-http-cancellation/README.md) retains the initial 408 interoperability failure and the corrected release passing 27 official-client HTTPS/KVM operations. The session remains usable and independently observed accepted guest work completes without a cancelled RPC response.

[Observer role verification](../../benchmarks/2026-10-02/mcp-http-observer/README.md) confirms that an observer role caps even an admin scope through a separately configured MCP process: inventory succeeds while thirteen mutation, detail/checkpoint and file operations are refused. Separate processes and credentials inherit separate configured API roles; this does not provide per-user roles or tenant isolation within one endpoint.
