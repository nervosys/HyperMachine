# Native-port KVM verification after Redis preflight fix

Fresh development binaries for the node, control plane and native gateway were built from the isolated checkout after the Redis stored-record preflight fix. The shipped CLI, kernel, tagged guest image and checker remain the same inputs as the preceding capacity archive. Both IPv4/two-peer and IPv6/eight-peer ingress pass 26 checks: sixteen simultaneous distinct guest UDP destinations each exchange 100 tagged round trips, a seventeenth reservation is refused, pause unbinds all sixteen sockets while retaining allocations, resume restores delivery and deletion removes them. Each run ends with zero guests and all owned processes reaped. Runtime reports identify every executable and fixture by SHA-256 and check input stability.

The control-plane integration suite also passes all 30 HTTP tests. The preceding [preflight regression archive](../native-port-row-preflight/README.md) retains 86 passing library tests and the explicit owned Redis corruption/AOF test. This archive connects that source fix to fresh real KVM binaries; old archives remain unchanged.

source-context.json records 133 matching permitted root/isolated files and accepted isolated core hashes. Protected modified root core sources are excluded. store.rs is the exact current source snapshot. Build and HTTP logs, checker snapshot and both raw KVM reports/logs are covered by manifest.json. Certificate private keys and generated credentials are not archived.

Reproduce with freshly built binaries using the command in [the capacity archive](../native-sixteen-port-capacity/README.md), retaining --owner-context --owner-port-api --owner-port-cli --native-capacity and adding --local-ipv6 --peer-count 8 for the second run. Use fresh output directories. Guest image construction and fixture scope are documented there.

This establishes post-fix UDP capacity and lifecycle behavior on owned loopback with IPv4 guest UDP destinations. Sixteen simultaneous TCP services, guest reboot persistence, public Internet operation and competitor performance remain unverified. No timed comparison or optimization claim is made.
