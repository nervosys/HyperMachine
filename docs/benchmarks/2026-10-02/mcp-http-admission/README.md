# Bounded MCP HTTP admission

MCP HTTP admission now permits at most sixteen simultaneous body reads and sixteen tool/protocol workers per process. Requests refused by either admission gate receive HTTP 503. Cancellation notifications bypass worker admission, so saturation does not prevent cancellation. Body permits are released before execution; operation permits remain with detached workers until they finish or are explicitly cancelled. These bounds apply to local reads and waits, not upstream operations already accepted by the API.

Invalid supplied Origins now receive 403 before authentication is checked. Missing or invalid credentials receive 401 with a Bearer challenge. Sessions and per-session overlap limits remain unchanged.

The full isolated Linux CLI suite passed 143 tests. Four new regression tests cover body-admission refusal/recovery without session allocation, cancellation under exhausted worker admission, Origin precedence/Bearer challenge, and sixteen live delayed HTTP calls. In the live saturation test, a seventeenth request receives 503, cancellation succeeds, the original wait receives 408, admission recovers, and remaining calls complete. Strict Clippy passed with the existing too_many_arguments exception.

The updated release binary repeats the official-client HTTPS/KVM lifecycle fixture. report.json records the outcome, binary identity, 21 guest lifecycle operations, additional unauthenticated Origin refusal, inventory and process cleanup. source-context.json binds the compiled isolated sources. Prior evidence archives remain immutable. Provisional worktree boot files, credentials, private certificates, guest data and binaries are excluded.

```sh
cargo test --offline --locked -p hm-cli --lib
cargo clippy --offline --locked -p hm-cli --lib -- -D warnings -A clippy::too_many_arguments
python -O tools/check-mcp-http-kvm.py --output /new/owned/run --daemon /path/node --control-plane /path/control --cli /path/hm --kernel /path/kernel --initrd /path/guest
```

This establishes bounded local admission and functional regression behavior. It provides no throughput/latency ranking, production proxy SLA, OAuth/browser login, multi-user isolation or remote-work rollback guarantee.
