# Real UDP malformed-frame refusal and recovery

Eight full-stack HTTPS/KVM checks pass. The added check opens an authenticated UDP upgrade, sends an oversized length prefix and requires session closure within the socket deadline. A fresh authenticated session then returns exact binary bytes, proving the guest service remains usable after refusal. Existing TLS trust/hostname, key refusal, two-peer empty/binary/maximum-size payloads and pause/resume/delete checks remain passing. All owned services are reaped and guest inventory is empty; input hashes are unchanged.

Reproduce with check-udp-cluster-kvm.py --tls and the input paths recorded in report.json. This checks oversized-frame refusal at the real guest relay. It does not test truncated frames, sustained malformed traffic, idle expiration, memory accounting or throughput. Node traffic remains HTTP with cluster credentials; mTLS and IPv6 remain unverified. No competitor win is claimed.
