# Real KVM host-bound secret lifecycle

The separate verification daemon and HTTPS client guest passed seven owned KVM
checks with seven observed upstream requests. Curl verifies certificates using
the guest trust bundle; no insecure TLS option is used. A temporary owned CA
signs a separate server leaf for two fixture names. The daemon explicitly trusts
the CA through --egress-upstream-ca, and the guest also trusts it for unmodified
pass-through requests. The listener binds a local host IP and ephemeral port;
egress allows only that address, explicitly granted as a reserved /32.

The checks observe placeholders before scope attachment, substitutions in a
header, Basic auth, query and JSON with correct Content-Length, unchanged tokens
for the other hostname, rotation, child fork exclusion, pause/resume scope
reattachment, and removal/revocation. Scope policy is keyed by exact sandbox ID;
network policy inheritance does not copy operator-held secrets to a fork.

This test exposed a routing gap: address-authorized connections did not inspect
TLS when only a secret store was configured. gateway-mod.rs now includes a
configured secret store in the sniffing decision. All 188 network tests pass.
The matching Rust sources and complete isolated checkout hash catalog are
archived. Protected workspace core sources were neither read nor built.

Early fixture attempts were excluded: host address inventory contained an empty
record; the initial OpenSSL CA certificate was mistakenly used as a server leaf;
cold-boot mode did not support the prepared lifecycle pause. The final fixture
uses a prepared base template and a separate CA/server leaf. Failed runs cleaned
up owned processes. Raw API responses, guest tokens, secret policy, keys and CA
account files are not archived. The final checker reaps its daemon and listener,
deletes its guests and removes the private temporary tree.

The accepted benchmark daemon SHA remains
2e9e53dace2ae5e2bb60228d4b784182eb252aad3c6bf76c81049dfc0540a70f,
and the accepted initrd remains
1fcc60fa58a9826b0e299c76dfc1e51aae0ce524b540efb7d51144c0d3510c9c.
The result records separate verification inputs. No performance win or managed
competitor parity is established. Each curl invocation creates a TLS connection;
concurrent lifecycle/reload races, organization scope and additional body formats
remain unverified through KVM.

Reproduce with the archived changes overlaid in the isolated accepted-core
checkout: cargo test -p hv2-net; cargo build -p hv2-sandboxd; then run
`python3 tools/check-secret-substitution-kvm.py --daemon BIN --kernel KERNEL
--initrd HTTPS_CLIENT_IMAGE --output NEW.json`. Linux KVM, root, OpenSSL, ip and
the separately built client fixture are required. The output must not exist.
