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

Remote authenticated MCP remains a gap. A proper implementation needs the HTTP message/lifecycle contract, Origin validation, protocol-version handling, authentication, trusted transport and authorization bound to the configured API role. The [MCP 2025-11-25 transport specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) permits JSON responses without SSE, requires an accepted notification to receive an empty 202 response, and defines session-ID handling for stateful servers. An HTTP GET may return 405 when SSE is unavailable. The legacy JSON API is not evidence that these requirements are met.

See the [current feature comparison](../../PLATFORM_PARITY.md) and the preserved [stdio cancellation evidence](../../benchmarks/2026-09-30/mcp-stdio-cancellation.json).
