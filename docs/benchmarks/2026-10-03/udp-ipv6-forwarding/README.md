# IPv6 loopback UDP forwarding

HyperMachine now explicitly selects IPv6 guest loopback with hm sandbox vm udp ID --port 5353 --guest-ipv6. Optional --listen '[::1]:0' selects a local IPv6 listener independently. Defaults retain IPv4 guest/local loopback. IPv6 uses an authenticated /sandboxes/{id}/ports/{port}/udp6 route, hv2-udp6/1 upgrade and ForwardUdp6 guest operation. The IPv6 destination is ::1 with no IPv4 fallback. Both families retain two-byte big-endian frames and a 65,507-byte payload bound, including empty datagrams. Peer budgets, bounded queues, timeouts and lifecycle locking remain shared.

Four fresh control API HTTPS/node mTLS/Redis/KVM fixtures use the same immutable CLI/node/control/kernel/image inputs:

| Fixture | Result |
|---|---|
| IPv4 local peers to IPv6-only guest service | 10 checks pass |
| Eight IPv6 local peers to IPv6-only guest service | 11 checks pass; 1,000 exact tagged 4 KiB replies per peer |
| Default IPv4 local/guest compatibility | 9 checks pass |
| IPv6 node credential refusal with valid mTLS client | 11 checks pass; missing and wrong cluster credentials rejected |

The guest echo service explicitly binds ::1 and enables IPV6_V6ONLY, so IPv4 fallback cannot satisfy the IPv6 checks. Every peer preserves empty, binary and 65,507-byte datagrams. Wrong API keys, untrusted control certificates/wrong hostnames, absent node client certificates, IPv4 upgrade protocol on the IPv6 route and oversized frames are refused. Pause closes live sessions, resume opens fresh sessions and delete closes streams and clears inventory. Owned services are reaped; input hashes remain unchanged. The first three reports use initial-checker.py; node-refusal uses the current checker with direct node credential checks.

Regression logs record 21 guest library/binary tests, 15 focused host-agent tests, 1,069 API library tests, 146 CLI library tests plus 12 VM client tests, 27 existing control integration tests, 46 daemon tests and a new IPv6 control integration test. The new test rejects wrong/inventory/observer keys, checks cluster-token forwarding without API-key leakage, preserves framed bytes through real HTTP upgrades and covers node/paused/deleted errors. API tests refuse family protocol mismatch; host tests select ForwardUdp6, propagate guest refusal and reject port zero before contacting the guest.

Reproduce with the archived checker, recorded inputs, --tls --mtls and a fresh output. Add --guest-ipv6 for IPv6 guest selection and --local-ipv6 for IPv6 client sockets. For eight peers add --peer-count 8 --concurrent-samples 1000 --concurrent-payload-bytes 4096. Compile the owned echo fixture statically and build a fresh image with recorded base/agent/echo hashes. Build and test in the accepted isolated checkout. All eleven IPv6 runtime/test implementation files and six crate manifests match root bytes. Source context also records nineteen unrelated root/isolated differences in host-agent helpers, daemon helpers and one API GPU test; these root versions were not incorporated into this run. Isolated core remains the accepted immutable version and its lockfile differs from root. Root protected core modifications were not read or executed. These results therefore verify the IPv6 implementation in the recorded accepted context, not the entire root worktree.

This implements IPv6 loopback UDP through the authenticated stream tunnel. Native public UDP exposure, other guest-kernel compatibility, sustained loss/churn, production durability and competitor performance remain unverified. Concurrent samples establish correctness characterization, not a matched performance comparison; previous matched optimization results concern separately preserved binaries.
