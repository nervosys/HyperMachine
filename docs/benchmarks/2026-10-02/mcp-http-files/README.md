# Bounded binary file tools over MCP HTTP

Operator-configured envd routing enables file_upload and file_download over MCP HTTP, for fourteen tools total. New sessions clone validated file configuration without copying another session's initialization state. The implementation reuses the stdio binary tools, including the 256-KiB transfer cap, guest credential lookup through the configured API role, query-path encoding, output scrubbing and pre-dispatch argument validation. Tool arguments cannot choose an endpoint. --envd-ca-cert adds a PEM trust root for private envd HTTPS without disabling certificate verification. File tools remain absent unless --envd-endpoint is supplied.

The isolated Linux CLI suite passed all 144 tests and strict Clippy. The new HTTP test verifies fourteen-tool discovery, rejected route-override arguments and invalid operator URL/domain refusal. Existing stdio binary, size, cancellation and admission regressions remain green.

The release CLI SHA is a9b9aeb4dc19a6eafe9c1d4b0877bf2434707582e541a9f79d89550bbb969748. The official Python MCP client 1.23.3 completed 25 operations with real one-vCPU, 1024-MiB KVM guests. It transferred bytes(range(256)) repeated 1024 times: an exact 262,144-byte binary roundtrip, including a filename with a quote and literal query punctuation. A 262,145-byte guest download was refused. Checkpoint restore, pause/resume, fork isolation, nonzero guest exit, missing-guest handling and deletion also passed. Access tokens were absent from tool outputs.

The official client used trusted MCP HTTPS, the CLI used verified control-plane TLS and a separately configured trusted envd HTTPS route, and the control plane used node mTLS. A bounded owned fixture proxy terminated envd TLS and forwarded to the owned loopback guest proxy; this is not a production proxy or end-to-end guest TLS claim. Missing credentials and invalid Origins were refused. Final sandbox inventory was empty; all four owned processes exited zero, with no cleanup errors and unchanged artifacts.

```sh
hm sandbox vm --endpoint https://api.example.test --api-ca-cert api-ca.pem mcp-http \
  --listen 127.0.0.1:3985 --allow-origin https://agent.example.test \
  --envd-endpoint https://proxy.example.test --envd-domain sandbox.example.test --envd-ca-cert envd-ca.pem
python -O tools/check-mcp-http-kvm.py --output /new/owned/run --daemon /path/node --control-plane /path/control --cli /path/hm --kernel /path/kernel --initrd /path/guest --files
```

Set HM_MCP_TOKEN and HV2_API_KEY separately in the process environment. source-context.json binds the isolated build; the three provisional boot files were excluded. Credentials, private certificates, guest data and binaries are excluded. Previous evidence remains immutable. OAuth/browser login, SSE/replay, remote multi-user isolation, production proxy operation, real-guest HTTP cancellation and performance leadership remain incomplete or unverified.
