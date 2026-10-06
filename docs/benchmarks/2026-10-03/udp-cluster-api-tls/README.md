# Full-stack UDP over verified control API HTTPS

Seven checks pass through the shipped CLI, separately launched control plane with API TLS, private Redis, real daemon and actual KVM guest UDP socket. CLI trust refusal and trusted wrong-hostname refusal both fail as expected. With the owned CA, two peers receive exact empty/binary/65,507-byte datagrams through HTTPS upgrades. API-key refusal, pause closure, resumed-session payload and deletion closure also pass. All owned processes are reaped and guest inventory is empty; input binary/image hashes are unchanged.

Reproduce the full fixture with the same input arguments as udp-cluster-kvm and add `--tls`. The fixture generates a private Ed25519 CA and distinct localhost leaf, enables --api-tls-cert/--api-tls-key, uses verified urllib HTTPS and --api-ca-cert for the CLI. Private certificates, keys, tokens and snapshot files are removed, not archived.

Initial readiness failed because Python's strict chain verification requires the CA's certificate-signing key usage. The fixture CA was corrected to include keyCertSign/cRLSign; verification was not disabled and production code did not change. The final successful run is archived.

HTTPS is verified from CLI/client to control API only. The node hop remains HTTP with a configured cluster token; Redis is local plaintext. Node mTLS, public CA operation, IPv6, native public UDP exposure, idle expiration and performance/load/loss remain unverified. The accepted isolated source context is described by udp-cluster-kvm; current root API TLS differences remain outside a full-root claim. No competitor superiority is asserted.
