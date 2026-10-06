# Full-stack UDP with node mutual TLS

Nine checks pass through shipped CLI/control-plane/Redis/daemon/KVM processes with client-facing API HTTPS and mutually authenticated control-plane-to-node TLS. An owned CA signs distinct API, node and control certificates; the node identity is hv2-node and the control certificate carries clientAuth. A trusted peer lacking a client certificate cannot obtain an HTTP response from the node. The configured control-plane client identity succeeds.

Existing API trust/hostname refusal, key refusal, empty/binary/maximum-size payloads, oversized-frame refusal/recovery, two-peer CLI isolation and pause/resume/delete handling all pass. Input hashes stay unchanged, all owned processes are reaped and guest inventory is empty. Credentials and keys are temporary and not archived.

Reproduce check-udp-cluster-kvm.py with --tls --mtls and the recorded input paths. This uses the pre-existing mTLS implementation; no TLS runtime code changed. Redis is owned local plaintext. Rogue-client certificates, wrong node identity, live certificate rotation, idle expiry specifically under mTLS and performance/load remain outside this run. Public CA deployment, IPv6 and native public UDP are not established. No competitor superiority is claimed.
