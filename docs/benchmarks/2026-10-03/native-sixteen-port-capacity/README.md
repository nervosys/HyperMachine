# Sixteen published UDP ports through real KVM

Two owned Redis/mTLS/KVM profiles publish sixteen distinct UDP reservations for one actual CLI-created VM, using the shipped expose/list/remove commands and existing owner-aware production binaries. IPv4 and IPv6 ingress each pass twenty-six checks. Sixteen guest services on ports 19000–19015 prepend their own two-byte destination port to replies, so a gateway that incorrectly routes all public ports to one echo service fails. Each public port has its own source socket and worker. A sixteen-worker barrier precedes 100 exact 512-byte round trips per port (1,600 total), with no retries during those transfers. Full source address and destination tag are verified. Readiness alone permits bounded retries.

The allocation window has seventeen available ports, so refusing the seventeenth VM reservation with HTTP 409 establishes the sixteen-per-VM cap rather than mere pool exhaustion. All sixteen allocated public ports are unique and owner listing matches them. Guest services and native UDP peers remain live together during the concurrent test. Afterward one tagged reservation is replaced by the original both-protocol port 18082, retaining sixteen reservations: fifteen tagged UDP destinations and one UDP/TCP destination. Pause proves all sixteen UDP sockets are unbound by exclusive bind probes while exact owner reservations remain; resume verifies the same list and all sixteen destinations again. Actual deletion clears the global allocation index and unbinds every UDP listener. The ordinary native TCP, maximum UDP payload, owner/role refusal, key rotation, fork and lifecycle checks also pass. All owned processes are reaped and zero guests remain.

Gateway fixture limits are explicitly raised to 32 namespace reservations and `peer_count + 18` aggregate sessions (20 for IPv4/two baseline peers, 26 for IPv6/eight baseline peers). Per-listener UDP peer limits retain the baseline peer count. This exercises a configured capacity path, not an optimization with identical resource bounds. Guest inventory reports one vCPU and 1,024 MiB. Ingress remains owned loopback; guest UDP destinations remain IPv4. These tests establish sixteen simultaneous UDP publications, not sixteen simultaneous TCP/both-protocol destinations, fleet capacity, Internet availability, resource efficiency or a competitor latency/throughput win.

The only new guest fixture option is `--tag-port`; normal echo remains unmodified in behavior. Tagged payloads are limited to 65,505 bytes so the two-byte tag keeps replies within 65,507 bytes. Four owned helper modes verify normal/tagged IPv4/IPv6 responses and cleanup. Normal IPv4 65,507-byte echo and tagged 65,505-byte replies pass; tagged oversize input is dropped without preventing subsequent valid replies. IPv6 helper checks cover empty/binary/1,024-byte payloads. Static compilation uses `-O2 -Wall -Wextra -Werror`. A fresh deterministic image retains the exact accepted base/agent/TCP fixture and replaces only the UDP helper; image-report.json records all hashes. Real KVM default echo checks still verify empty/binary/maximum-size untagged traffic before capacity work.

The accepted owner API host binaries and final CLI are unchanged from their preceding archives. host-source-context.json is copied from that accepted context; no new Rust production code or protected modified root core is read, built or staged here. The runtime reports verify executable, kernel, new image and exact checker hashes before/after. Helper source/checker/image builder snapshots, raw reports/logs and image/helper reports are covered by manifest.json. No certificate private keys or policy files are archived. Generated API/cluster credentials and the CLI-created workload token are checked absent from runtime logs.

Reproduction requires the guest image with the tagged helper and a fresh output directory:

```sh
python3 tools/check-udp-cluster-kvm.py --daemon /path/hv2-sandboxd \
  --control-plane /path/hv2-control-plane --cli /path/hm \
  --kernel /path/bzImage --initrd /path/tagged-fixture.cpio.gz \
  --native-gateway /path/hv2-native-gateway --tls --mtls --owner-context \
  --owner-port-api --owner-port-cli --native-capacity --output /fresh/results
```

Add `--local-ipv6 --peer-count 8` for the second profile. Guest reboot, public DNS/firewall/Internet operation, sixteen TCP publications, central organization identity and competitor performance remain incomplete or unverified. The broader competitive goal remains open.
