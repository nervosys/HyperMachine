# Awaited proxy admission for durable sandbox sharing

The proxy now awaits SandboxRoutes::admit_request before opening a backend route or waking a guest. The new default hook delegates to existing synchronous authorize_request implementations, preserving their API and behavior. A future control-plane sharing implementation can await owner-bound durable grant reads at this boundary without moving authorization after guest wakeup.

A real owned HTTP regression blocks inside admission and confirms the request remains pending with zero backend resolutions, then denies admission and confirms zero resolutions afterward. It covers browser HTTP 401/challenge and gRPC status 16/challenge. Existing control-plane private browser policies remain synchronous and work through the default delegate.

The isolated full API suite passes 1,070 tests; the control-plane integration suite passes 31, including private URL authentication/credential stripping over HTTP/1.1 and HTTP/2, identity headers and envd behavior. Those control-plane fixtures use owned HTTP nodes rather than real KVM guests. An initial missing test import caused compilation failure; the corrected full logs are retained. No KVM, binary release or performance improvement is claimed.

Only permitted crates/hv2-api/src/sandbox_proxy.rs changes. Existing synchronous trait implementations and direct callers need no changes. Root source stayed unchanged until tests completed; builds use the accepted isolated tree and target directory with --locked. Accepted isolated protected hashes were verified; protected root backend/boot files were not read, built, edited or staged. The single-file catalog is not complete build closure.

This is a prerequisite, not self-service sharing completion. Browser credentials and their scopes remain operator-administered. Remaining work: bounded durable grant reads; atomic owner/incarnation-bound sharing state and expiry in Memory/Redis; owner-only read/update/revoke APIs and CLI; fail-closed proxy enforcement before wakeup; deletion cleanup; fork exclusion and custom-domain rebinding; persistence/restart and real KVM/TLS verification. SSO and verified-email identity also remain open.

python3 verify.py checks archive hashes, source identities, exact passing regression names and complete suite counts. Review the retained source/tests to assess scope; green test logs alone do not prove owner-managed sharing or broader competitor parity.
