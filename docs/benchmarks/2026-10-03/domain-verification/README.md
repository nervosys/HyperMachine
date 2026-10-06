# DNS ownership verification

The control plane can require short-lived DNS TXT proof before a custom-domain claim or port update. The challenge binds namespace, sandbox, hostname and expiry using an operator-held HMAC key. API authorization and atomic store ownership still apply. Missing/expired/transferred proofs cannot mutate bindings; resolver failures are retryable. Without the policy, operator-managed behavior remains available. Existing bindings are not retroactively revalidated. Automatic certificate issuance/ACME remains absent.

Release build and strict cluster/CLI lint pass. All 52 cluster library tests, 26 existing control-plane HTTP integration tests and 145 CLI tests pass, including six DNS policy, exact-response, TLS transport and real HTTP API tests plus CLI argument coverage. Seven actual-binary startup checks refuse malformed, oversized, insecure or mismatched policies without printing the signing key.

The DNS-enabled real-KVM fixture uses an owned HTTPS DNS JSON resolver with a validated local certificate, an isolated Redis store and actual guest HTTP services. It verifies proof refusal, successful HTTPS routing, failed revalidation preserving the existing port, control-plane restart, auto-resume, deletion and refusal of a prior sandbox's proof for its replacement. All owned services stop and guest inventory returns to zero. A separate legacy fixture verifies the same binary without DNS policy. Input hashes remain unchanged.

This establishes local feature behavior, not public DNS propagation, managed resolver availability, DNSSEC validation, multi-tenant isolation, automatic TLS or a performance win. DNS proofs are checked on claims/updates; expiry or TXT removal does not revoke an established route. See [operator setup](../../../CUSTOM_DOMAINS.md) for these limits.

Build inputs use the accepted source reconstruction plus the checksummed overlays in source-overlays. The context records all compiled source hashes, including pre-existing CLI/private-proxy overlays needed to test the current tree; the three provisional core files were never used. Initial lock/overlay/fixture/command failures remain recorded alongside final results.
