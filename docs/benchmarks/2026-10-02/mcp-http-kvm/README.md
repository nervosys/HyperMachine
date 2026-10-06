# Official MCP client over HTTPS with real KVM guests

The release CLI was built offline with the current lockfile in isolated clean sources. The official Python MCP client 1.23.3 connected through a trusted HTTPS fixture proxy to the MCP loopback endpoint. The CLI used verified control-plane TLS; the control plane used node mTLS plus its cluster token. Redis-backed registration supplied a prepared one-vCPU, 1024-MiB KVM template.

Both retained runs completed 21 lifecycle operations: creation, guest exec, inspect/list, checkpoint save/list/restore/delete, pause/resume, fork, independent parent/child writes, nonzero guest exit handling, missing-guest error handling and deletion. Tool output scrubbed access tokens. Missing MCP credentials and an invalid Origin were refused through HTTPS. Final sandbox inventories were empty.

The first run failed the overall cleanup gate because the fixture sent SIGTERM to processes with SIGINT graceful-shutdown handlers; CLI and control plane exited -15, while node and Redis exited zero. The corrected coordinator used SIGINT. The final run passed and all four owned processes exited zero. Both reports and exact coordinator versions are preserved. No failed lifecycle operation is omitted.

The accepted daemon SHA is 2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f. The control plane is the previously verified reserved-alias binary; its SHA is recorded in each report. The new CLI SHA is 662e45b5c6045b093695d1cd02b7d25f6d00b9dc033a5bc82ed467c87f8e3123. source-context.json and the frozen Rust sources bind its compilation; the provisional worktree boot files were excluded. The accompanying library suite passed 139 tests and strict Clippy in the preceding mcp-http archive.

```sh
python -O tools/check-mcp-http-kvm.py --output /new/owned/run \
  --daemon /path/to/accepted/node --control-plane /path/to/accepted/control \
  --cli /path/to/new/hm --kernel /path/to/kernel --initrd /path/to/compatible/guest
```

Requires Linux KVM, Redis, OpenSSL and the official MCP Python client. The HTTPS reverse proxy is a bounded fixture, not a production proxy implementation. Credentials, private certificates, guest data and binaries are excluded. This establishes operator-authenticated HTTP lifecycle functionality in the owned fixture; browser login/OAuth, remote multi-user authorization, HTTP file tools, SSE/replay, real-guest HTTP cancellation and performance rankings remain unverified or absent.
